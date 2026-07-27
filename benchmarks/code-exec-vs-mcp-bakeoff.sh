#!/usr/bin/env bash
# Code-execution vs MCP-schema token bake-off (bead tokenzero-da8.1)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"; H=(python3 "$ROOT/benchmarks/harness.py")
BIN="$("${H[@]}" resolve_bin)" || { echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2; exit 1; }; CORPUS_FILE="${ROOT}/README.md"; CARGO_FILE="${ROOT}/Cargo.toml"
EDIT_TMP=$(mktemp $ROOT/.tz-bakeoff-edit.XXXXXX); CAP_FILE=$(mktemp /tmp/tz-bakeoff-cap.XXXXXX); SHELL_TMP=$(mktemp /tmp/tz-bakeoff-shell.XXXXXX); CACHE_FILE=$(mktemp /tmp/tz-bakeoff-cache.XXXXXX); rm -f "$CACHE_FILE"
trap 'rm -f $EDIT_TMP $CAP_FILE $SHELL_TMP $CACHE_FILE' EXIT
log() { printf '[bakeoff] %s\n' "$*" >&2; }; tok() { printf '%s' "$1" | "${H[@]}" tok; }
now_ms() { "${H[@]}" now_ms; }; emit_row() { printf '| `%s` | `%s` | %s | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6" "$7"; }
cap_start=$(now_ms); "$BIN" capabilities --json > "$CAP_FILE" || { echo "ERROR: capabilities --json failed" >&2; exit 1; }; MCP_WALL=$(( $(now_ms) - cap_start ))
mcp_tok() { "${H[@]}" mcp_schema_tokens "$CAP_FILE" "$1"; }; quality_check() { printf '%s' "$2" | "${H[@]}" quality "$1"; }

# v1.4.0 capsules keep bodies behind refs (and session dedup may answer
# "unchanged:"), so quality is checked against the EXPANDED bytes when a ref
# is present, falling back to the payload text otherwise.
quality_of() {
  local task="$1" payload="$2" ref expanded q
  # Payload first: CodeMode plans return expanded bodies inline, so their
  # payload already contains the content under test.
  q=$(quality_check "$task" "$payload")
  if [[ "$q" == PASS ]]; then printf 'PASS'; return; fi
  # Fallback for CLI capsules: bodies live behind refs, so expand and re-check.
  ref=$(printf '%s' "$payload" | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
except Exception:
    print(""); raise SystemExit
v=d.get("value")
print(d.get("detail_ref") or d.get("ref") or (v.get("ref","") if isinstance(v,dict) else ""))' 2>/dev/null)
  if [[ -n "$ref" ]] && expanded=$("$BIN" expand "$ref" --cache-path "$CACHE_FILE" 2>/dev/null); then
    quality_check "$task" "$(printf '%s' "$expanded" | python3 -c 'import sys,json; print(json.dumps({"visible":{"text":sys.stdin.read()}}))')"; return
  fi
  printf '%s' "$q"
}

cli_one() {
  local task="$1" cmd_str="$2"; shift 2; local start end out status=0
  start=$(now_ms); out=$("$@") || status=$?; end=$(now_ms); local out_tok; out_tok=$(printf '%s' "$out" | "${H[@]}" accounting 2>/dev/null || echo 0)
  local q; if [[ $status -ne 0 ]]; then q=FAIL; elif [[ $task == none ]]; then q=PASS; else q=$(quality_of "$task" "$out"); fi; printf '%s\t%s\t%s\t%s\t%s\n' "$(tok "$cmd_str")" "$out_tok" 1 "$((end-start))" "$q"
}
sum_rows() {
  local in_t=0 out_t=0 turns=0 wall=0 q=PASS i o t w qu
  while IFS=$'\t' read -r i o t w qu; do
    in_t=$((in_t+i)); out_t=$((out_t+o)); turns=$((turns+t)); wall=$((wall+w)); [[ "$qu" == PASS ]] || q=FAIL
  done
  printf '%s\t%s\t%s\t%s\t%s\n' "$in_t" "$out_t" "$turns" "$wall" "$q"
}
CM_BIN="${TOKENZERO_CODEMODE_BIN:-$ROOT/target/release/tokenzero-codemode}"
cm_call() {
  local plan="$1"
  { printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"bakeoff","version":"0"}}}' '{"jsonrpc":"2.0","method":"notifications/initialized"}'; python3 -c 'import json,sys; print(json.dumps({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"tz_execute_code","arguments":{"plan":sys.argv[1],"root":sys.argv[2]}}}))' "$plan" "$ROOT"; } | "$CM_BIN" 2>/dev/null | python3 -c 'import sys,json
for line in sys.stdin:
    try: d=json.loads(line)
    except Exception: continue
    if d.get("id")==2:
        result=d.get("result",{})
        print(json.dumps(result.get("structuredContent") or result))
        break'
}
run_codemode() {
  local task="$1" plan="$2" start end out status=0 attempt
  start=$(now_ms)
  if [[ -x "$CM_BIN" ]]; then
    # CodeMode JS sandbox lives only in the tokenzero-codemode stdio artifact;
    # `tokenzero codemode` on the MCP-surface CLI returns typed "unavailable".
    # The codemode machine permit is a global single-slot lock shared with any
    # other live agent sessions on this host, so a busy (retryable) reply or a
    # failed server cold-boot (empty/non-JSON output) is retried rather than
    # reported as a benchmark failure.
    for attempt in 1 2 3 4 5; do
      out=$(cm_call "$plan") || status=$?
      if printf '%s' "$out" | python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
except Exception:
    raise SystemExit(1)  # empty/garbled output: transient boot failure, retry
err=d.get("error") or {}
if err:
    raise SystemExit(1)  # any error envelope: permit contention is transient, retry'; then break; fi
      if [[ $attempt -lt 5 ]]; then sleep 2; fi
    done
  else
    out=$("$BIN" codemode --json "$plan") || status=$?
  fi; end=$(now_ms)
  local out_tok turns q; out_tok=$(printf '%s' "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("value",{}).get("raw_tokens", d.get("telemetry",{}).get("raw_tokens",0)))' 2>/dev/null || echo 0)
  turns=$(printf '%s' "$out" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("telemetry",{}).get("logical_ops",1))' 2>/dev/null || echo 1)
  if [[ $status -ne 0 ]]; then q=FAIL; else q=$(quality_of "$task" "$out"); fi
  emit_row "$task" CodeMode "$(tok "$plan")" "$out_tok" "$turns" "$((end-start))" "$q"
}
emit_cli() { local task="$1"; shift; local r i o t w q; r=$("$@"); IFS=$'\t' read -r i o t w q <<<"$r"; emit_row "$task" CLI "$i" "$o" "$t" "$w" "$q"; }
setup_edit() { printf 'alpha\nbeta\ngamma\n' > "$EDIT_TMP"; }

log "binary: $BIN"; log "corpus: $CORPUS_FILE , $CARGO_FILE"; printf '# Code-Exec vs MCP-Schema Bake-off Results\n\n'
printf '> Bead: tokenzero-da8.1 — generated by `benchmarks/code-exec-vs-mcp-bakeoff.sh`\n'; printf '> Baseline commit: %s\n\n' "$("${H[@]}" git_commit --short)"
printf '| Task | Approach | input_tokens | output_tokens | turns | wall_ms | quality |\n'; printf '|---|---|---:|---:|---:|---:|---|\n'

# --- tasks (same commands/quality contracts as original) ---
task=read_file; emit_row "$task" MCP-schema "$(mcp_tok read)" 0 1 "$MCP_WALL" simulated
emit_cli "$task" cli_one "$task" "$BIN read $CARGO_FILE --end-line 20 --json" "$BIN" read "$CARGO_FILE" --end-line 20 --json --cache-path "$CACHE_FILE"; run_codemode "$task" 'const f = await zero.read("'"$CARGO_FILE"'", { end_line: 20 }); return await zero.expand(f.ref, { max_visible_tokens: 100000 });'

task=search_filter; emit_row "$task" MCP-schema "$(mcp_tok find,grep)" 0 1 "$MCP_WALL" simulated
emit_cli "$task" cli_one "$task" "$BIN find TokenZero $CORPUS_FILE --json" "$BIN" find TokenZero "$CORPUS_FILE" --json --cache-path "$CACHE_FILE"; run_codemode "$task" 'const h = await zero.find("TokenZero", "'"$CORPUS_FILE"'"); return await zero.expand(h.ref, { max_visible_tokens: 100000 });'

task=edit_verify; emit_row "$task" MCP-schema "$(mcp_tok edit,read)" 0 1 "$MCP_WALL" simulated
setup_edit; row=$(
  cli_one none "$BIN edit --edits-json [{find:beta,replace:BETA}] --json $EDIT_TMP" \
    "$BIN" edit --edits-json '[{"find":"beta","replace":"BETA"}]' --json "$EDIT_TMP" --cache-path "$CACHE_FILE"
  cli_one "$task" "$BIN read $EDIT_TMP --json" "$BIN" read "$EDIT_TMP" --json --cache-path "$CACHE_FILE"
)
IFS=$'\t' read -r i o t w q <<<"$(printf '%s' "$row" | sum_rows)"; emit_row "$task" CLI "$i" "$o" "$t" "$w" "$q"
# CodeMode has no zero.edit binding (sandbox: mutation only via zero.shell), so the
# CodeMode path performs the edit via shell and verifies with read+expand.
setup_edit; run_codemode "$task" "const e = await zero.shell(\"sed -i '' s/beta/BETA/ $EDIT_TMP\"); const f = await zero.read(\"$EDIT_TMP\"); return await zero.expand(f.ref, { max_visible_tokens: 100000 });"

task=multi_step_nav; emit_row "$task" MCP-schema "$(mcp_tok tree,read,find)" 0 1 "$MCP_WALL" simulated
row=$(
  cli_one none "$BIN tree $ROOT --depth 2 --json" "$BIN" tree "$ROOT" --depth 2 --json --cache-path "$CACHE_FILE"; cli_one none "$BIN read $CARGO_FILE --json" "$BIN" read "$CARGO_FILE" --json --cache-path "$CACHE_FILE"
  cli_one "$task" "$BIN find workspace $CARGO_FILE --json" "$BIN" find workspace "$CARGO_FILE" --json --cache-path "$CACHE_FILE"
)
IFS=$'\t' read -r i o t w q <<<"$(printf '%s' "$row" | sum_rows)"; emit_row "$task" CLI "$i" "$o" "$t" "$w" "$q"
run_codemode "$task" 'const t = await zero.tree("'"$ROOT"'", { depth: 2 }); const f = await zero.read("'"$CARGO_FILE"'"); const hits = await zero.find("workspace", "'"$CARGO_FILE"'"); return await zero.expand(hits.ref, { max_visible_tokens: 100000 });'

task=shell_expand; emit_row "$task" MCP-schema "$(mcp_tok run,expand,read)" 0 1 "$MCP_WALL" simulated
$BIN run --json --cache-path "$CACHE_FILE" -- find . -maxdepth 1 -name Cargo.toml > $SHELL_TMP; SHELL_REF=$("${H[@]}" first_blob_ref "$SHELL_TMP"); [[ -n "$SHELL_REF" ]]
row=$(
  cli_one none "$BIN run --json -- find . -maxdepth 1 -name Cargo.toml" \
    "$BIN" run --json --cache-path "$CACHE_FILE" -- find . -maxdepth 1 -name Cargo.toml
  cli_one "$task" "$BIN expand $SHELL_REF" "$BIN" expand "$SHELL_REF" --cache-path "$CACHE_FILE"
)
IFS=$'\t' read -r i o t w q <<<"$(printf '%s' "$row" | sum_rows)"; emit_row "$task" CLI "$i" "$o" "$t" "$w" "$q"
run_codemode "$task" 'const out = await zero.shell("find . -maxdepth 1 -name Cargo.toml"); const f = await zero.expand(out.stdout_ref, { max_visible_tokens: 100000 }); return f;'

printf '\n_Legend: quality = PASS/FAIL for executed approaches; `simulated` = MCP schema load only (no task execution)._\n'; log done.
