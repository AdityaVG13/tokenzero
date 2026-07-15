#!/bin/sh
# TokenZero benchmark: raw tool output vs TokenZero-visible output, measured
# with TokenZero's own tokenizer on both sides (same-tokenizer, recovery-
# preserving comparison; every TokenZero row keeps exact tz:// refs).
#
# Usage: scripts/benchmark_tokens.sh [path-to-tokenzero-binary]
# Runs from the repo root. Emits one JSON object per workload plus a totals
# row on stdout; everything else goes to stderr.
set -eu

TZ_BIN="${1:-$HOME/.tokenzero/bin/tokenzero}"; ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"; WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Benchmark in an isolated cache so session/TTL state starts cold.
BENCH_CACHE="$WORK/recovery-cache.json"

count_tokens() {
    # Token count of stdin via TokenZero's own accounting.
    "$TZ_BIN" ingest --stdin --json --cache-path "$WORK/count-cache.json" 2>/dev/null \
        | jq '.accounting.raw_tokens'
}

visible_of() {
    jq '.accounting.visible_tokens'
}

row() {
    # row <name> <raw_tokens> <visible_tokens>
    jq -cn --arg name "$1" --argjson raw "$2" --argjson visible "$3" \
        '{workload: $name, raw_tokens: $raw, tokenzero_visible_tokens: $visible,
          savings_pct: (if $raw > 0 then (100.0 * ($raw - $visible) / $raw | floor) else 0 end)}'
}

TOTAL_RAW=0; TOTAL_VISIBLE=0
add_totals() {
    TOTAL_RAW=$((TOTAL_RAW + $1)); TOTAL_VISIBLE=$((TOTAL_VISIBLE + $2))
}

echo "benchmarking with $TZ_BIN in $ROOT" >&2

# --- 1. Large file read ---------------------------------------------------
FILE=crates/tokenzero-mcp/src/lib.rs; RAW=$(count_tokens < "$FILE")
VIS=$("$TZ_BIN" read "$FILE" --json --cache-path "$BENCH_CACHE" | visible_of); row "read large source file ($FILE)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- 2. Repeat read: session dedup through the MCP server ------------------
# Response-gated, NOT timing-gated: id=3 is sent only after id=2's response
# has landed, so the second read is guaranteed to find the first's recorded
# serve and dedup. (A bare `sleep` raced the 4-worker pool and made this row
# unreproducible.) The server's single-flight gate also enforces this, but
# the harness must not depend on that to measure the steady-state behaviour.
MCP_OUT="$WORK/mcp-dedup.jsonl"; MCP_IN="$WORK/mcp-in.fifo"
mkfifo "$MCP_IN"
"$TZ_BIN" mcp-server --allowed-root "$ROOT" --cache-path "$BENCH_CACHE" \
    < "$MCP_IN" > "$MCP_OUT" &
MCP_PID=$!
# Hold the FIFO open for writing on fd 3 so the server does not see EOF
# between sends.
exec 3> "$MCP_IN"; printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"bench","version":"0"}}}' >&3
printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}' >&3; printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{\"path\":\"$FILE\"}}}" >&3
# Wait for id=2's response before issuing id=3.
waited=0
while ! grep -q '"id":2' "$MCP_OUT" 2>/dev/null; do
    sleep 0.05; waited=$((waited + 1))
    [ "$waited" -gt 600 ] && { echo "timed out waiting for id=2 response" >&2; break; }
done
printf '%s\n' "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"read\",\"arguments\":{\"path\":\"$FILE\"}}}" >&3; waited=0
while ! grep -q '"id":3' "$MCP_OUT" 2>/dev/null; do
    sleep 0.05; waited=$((waited + 1))
    [ "$waited" -gt 600 ] && { echo "timed out waiting for id=3 response" >&2; break; }
done
exec 3>&-; wait "$MCP_PID" 2>/dev/null || true
rm -f "$MCP_IN"; SECOND=$(jq -r 'select(.id==3) | .result.content[0].text' "$MCP_OUT")
VIS=$(printf '%s' "$SECOND" | count_tokens); row "re-read same file (seen-set dedup)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- 3. Repo-wide grep ------------------------------------------------------
RAW=$( (rg -n --no-heading --color=never 'fn ' crates/ || true) | count_tokens); VIS=$("$TZ_BIN" grep 'fn ' crates --json --max-files 200 --cache-path "$BENCH_CACHE" | visible_of)
row "repo-wide grep ('fn ' across crates/)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- 4. Test-suite run ------------------------------------------------------
RAW=$( (command cargo test -p tokenzero-filters 2>&1 || true) | count_tokens); VIS=$("$TZ_BIN" run --json --cache-path "$BENCH_CACHE" -- cargo test -p tokenzero-filters | visible_of)
row "cargo test run (tokenzero-filters suite)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- 5. Directory tree ------------------------------------------------------
RAW=$( (find crates -type f || true) | count_tokens); VIS=$("$TZ_BIN" tree crates --depth 3 --json --cache-path "$BENCH_CACHE" | visible_of)
row "directory listing (find vs tree, depth 3)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- 6. Re-find content: recall vs re-running the search --------------------
RAW=$( (rg -n --no-heading --color=never 'fn ' crates/ || true) | count_tokens); VIS=$("$TZ_BIN" recall 'fn main' --max-hits 10 --json --cache-path "$BENCH_CACHE" | visible_of)
row "re-find stored content (recall vs re-grep)" "$RAW" "$VIS"; add_totals "$RAW" "$VIS"

# --- totals ------------------------------------------------------------------
jq -cn --argjson raw "$TOTAL_RAW" --argjson visible "$TOTAL_VISIBLE" \
    '{workload: "TOTAL", raw_tokens: $raw, tokenzero_visible_tokens: $visible,
      savings_pct: (if $raw > 0 then (100.0 * ($raw - $visible) / $raw | floor) else 0 end)}'
