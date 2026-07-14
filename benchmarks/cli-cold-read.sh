#!/usr/bin/env bash
# CLI cold-read profiling for TokenZero (bead tokenzero-f1z)
#
# Measures cold vs warm `tokenzero read` startup tax and per-component cost.
# Cold boundary: removes ~/.tokenzero/recovery-cache.json before each run.
# Warm boundary: cache is left in place.
# Outputs a markdown table with p50/p90/p99 wall times (ms).
#
# Components:
#   1. process start  -> `tokenzero --help`            (pure binary launch floor)
#   2. store open     -> `tokenzero mem`               (recovery-cache init)
#   3. first read     -> `tokenzero read --end-line 1 <small-file>`
#   4. first expand   -> `tokenzero expand <ref>`      (first ref expansion)
#
# Startup tax = cold first_read p50 - cold process_start p50.
#
# Usage:
#   ./benchmarks/cli-cold-read.sh
#   TOKENZERO_BIN=/path/to/tokenzero ./benchmarks/cli-cold-read.sh
#   RUNS=100 ./benchmarks/cli-cold-read.sh
#
# Hyperfine is preferred. Falls back to /usr/bin/time if absent.
# Do NOT commit results; fill benchmarks/cli-cold-read-table.md with output.

set -euo pipefail

WARMUP="${WARMUP:-3}"
RUNS="${RUNS:-50}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SMALL_FILE="${SMALL_FILE:-$REPO_ROOT/Cargo.toml}"
HARNESS=(python3 -m benchmarks.harness)

log()  { printf '[cli-cold-read] %s\n' "$*" >&2; }
fail() { printf '[cli-cold-read] ERROR: %s\n' "$*" >&2; exit 1; }

BIN="$("${HARNESS[@]}" resolve_bin)" || fail "tokenzero binary not found"
log "binary: $BIN"
log "small file: $SMALL_FILE"
log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"

REF="$("$BIN" read --end-line 1 "$SMALL_FILE" 2>/dev/null \
  | grep -oE 'tz://local/[^[:space:]]+' | head -n1 || true)"
[[ -z "$REF" ]] && REF="tz://local/0000000000000000"
log "expand ref: $REF"

run_cell() {
  local label="$1" cold="$2" cmd="$3"
  local flags=(--runs "$RUNS" --warmup "$WARMUP")
  [[ "$cold" == "1" ]] && flags+=(--cold)
  "${HARNESS[@]}" measure_cell "$label" "$cmd" "${flags[@]}"
}

read -r help_cold_p50 help_cold_p90 help_cold_p99 \
  <<< "$(run_cell 'process_start (cold)' 1 "${BIN} --help")"
read -r help_warm_p50 help_warm_p90 help_warm_p99 \
  <<< "$(run_cell 'process_start (warm)' 0 "${BIN} --help")"
read -r mem_cold_p50 mem_cold_p90 mem_cold_p99 \
  <<< "$(run_cell 'store_open (cold)' 1 "${BIN} mem")"
read -r mem_warm_p50 mem_warm_p90 mem_warm_p99 \
  <<< "$(run_cell 'store_open (warm)' 0 "${BIN} mem")"
read -r read_cold_p50 read_cold_p90 read_cold_p99 \
  <<< "$(run_cell 'first_read (cold)' 1 "${BIN} read --end-line 1 \"${SMALL_FILE}\"")"
read -r read_warm_p50 read_warm_p90 read_warm_p99 \
  <<< "$(run_cell 'first_read (warm)' 0 "${BIN} read --end-line 1 \"${SMALL_FILE}\"")"
read -r exp_cold_p50 exp_cold_p90 exp_cold_p99 \
  <<< "$(run_cell 'first_expand (cold)' 1 "${BIN} expand \"${REF}\"")"
read -r exp_warm_p50 exp_warm_p90 exp_warm_p99 \
  <<< "$(run_cell 'first_expand (warm)' 0 "${BIN} expand \"${REF}\"")"

startup_tax=$((read_cold_p50 - help_cold_p50))

cat <<'HDR'
| Component | cold p50 (ms) | cold p90 (ms) | cold p99 (ms) | warm p50 (ms) | warm p90 (ms) | warm p99 (ms) |
|---|---:|---:|---:|---:|---:|---:|
HDR

printf '| `process_start` (`--help`) | %s | %s | %s | %s | %s | %s |\n' \
  "$help_cold_p50" "$help_cold_p90" "$help_cold_p99" \
  "$help_warm_p50" "$help_warm_p90" "$help_warm_p99"
printf '| `store_open` (`mem`) | %s | %s | %s | %s | %s | %s |\n' \
  "$mem_cold_p50" "$mem_cold_p90" "$mem_cold_p99" \
  "$mem_warm_p50" "$mem_warm_p90" "$mem_warm_p99"
printf '| `first_read` (`read`) | %s | %s | %s | %s | %s | %s |\n' \
  "$read_cold_p50" "$read_cold_p90" "$read_cold_p99" \
  "$read_warm_p50" "$read_warm_p90" "$read_warm_p99"
printf '| `first_expand` (`expand`) | %s | %s | %s | %s | %s | %s |\n' \
  "$exp_cold_p50" "$exp_cold_p90" "$exp_cold_p99" \
  "$exp_warm_p50" "$exp_warm_p90" "$exp_warm_p99"
printf '| **Startup tax** = cold first_read p50 − process_start p50 | **%s ms** | — | — | — | — | — |\n' \
  "$startup_tax"

log "done. Pipe stdout into benchmarks/cli-cold-read-table.md to record results."
