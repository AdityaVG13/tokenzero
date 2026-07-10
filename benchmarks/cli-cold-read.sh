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

# ---------- config ----------
WARMUP="${WARMUP:-3}"
RUNS="${RUNS:-50}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SMALL_FILE="${SMALL_FILE:-$REPO_ROOT/Cargo.toml}"

# ---------- helpers ----------
log()  { printf '[cli-cold-read] %s\n' "$*" >&2; }
fail() { printf '[cli-cold-read] ERROR: %s\n' "$*" >&2; exit 1; }

# Resolve tokenzero binary: env > cargo target > installed fallback
resolve_bin() {
  if [[ -n "${TOKENZERO_BIN:-}" && -x "${TOKENZERO_BIN}" ]]; then
    echo "${TOKENZERO_BIN}"; return
  fi
  local cand="$REPO_ROOT/target/release/tokenzero"
  if [[ -x "$cand" ]]; then echo "$cand"; return; fi
  cand="${HOME}/.tokenzero/bin/tokenzero"
  if [[ -x "$cand" ]]; then echo "$cand"; return; fi
  fail "tokenzero binary not found. Build with 'cargo build --release --bin tokenzero' \
or set TOKENZERO_BIN=/path/to/tokenzero"
}

# Cold-run cache clear: remove only the recovery-cache index.
clear_cache() {
  rm -f "${HOME}/.tokenzero/recovery-cache.json"
}

# Obtain a real ref to expand. Falls back to a zeroed placeholder ref.
resolve_ref() {
  local ref
  ref="$("${BIN}" read --end-line 1 "$SMALL_FILE" 2>/dev/null \
    | grep -oE 'tz://local/[^[:space:]]+' | head -n1 || true)"
  if [[ -z "$ref" ]]; then
    ref="tz://local/0000000000000000"
  fi
  printf '%s\n' "$ref"
}

# p50/p90/p99 (ms) from hyperfine export-json.
percentiles_from_json() {
  local json_file="$1"
  python3 - "$json_file" <<'PY' || printf '0\t0\t0\n'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    times = d.get("results", [{}])[0].get("times", [])
    if not times:
        raise ValueError("no times")
    times.sort()
    n = len(times)
    def p(q):
        return int(round(times[min(n - 1, int(q * (n - 1)))] * 1000))
    print(f"{p(0.5)}\t{p(0.9)}\t{p(0.99)}")
except Exception:
    print("0\t0\t0")
PY
}

# p50/p90/p99 (ms) from a list of wall times (fallback runner).
percentiles_from_times() {
  local times_file="$1"
  python3 - "$times_file" <<'PY' || printf '0\t0\t0\n'
import sys
xs = [float(x.strip()) for x in open(sys.argv[1]) if x.strip()]
if not xs:
    print("0\t0\t0"); raise SystemExit
xs.sort()
n = len(xs)
def p(q):
    return int(round(xs[min(n - 1, int(q * (n - 1)))] * 1000))
print(f"{p(0.5)}\t{p(0.9)}\t{p(0.99)}")
PY
}

# Run one benchmark cell. Args: label, cold(0|1), command-string
run_cell() {
  local label="$1" cold="$2" cmd="$3"
  local tmp bench_log prep_cmd
  tmp="$(mktemp -d -t tz-cli-cold.XXXXXX)"
  bench_log="$tmp/hyperfine.json"

  if [[ "$cold" == "1" ]]; then
    prep_cmd="rm -f ${HOME}/.tokenzero/recovery-cache.json"
  else
    prep_cmd="true"
  fi

  if command -v hyperfine >/dev/null 2>&1; then
    hyperfine \
      --warmup "$WARMUP" \
      --runs   "$RUNS" \
      --style  basic \
      --export-json "$bench_log" \
      --prepare "$prep_cmd" \
      --command-name "$label" \
      "$cmd" >/dev/null 2>&1 || true
    percentiles_from_json "$bench_log"
  else
    local times_file="$tmp/times.txt"
    : > "$times_file"
    if [[ "$cold" == "1" ]]; then
      ( eval "$prep_cmd" >/dev/null 2>&1; bash -c "$cmd" >/dev/null 2>&1 ) || true
    fi
    local i
    for ((i = 0; i < RUNS; i++)); do
      ( eval "$prep_cmd" >/dev/null 2>&1
        /usr/bin/time -f '%e' bash -c "$cmd" >/dev/null 2>>"$times_file"
      ) || true
    done
    percentiles_from_times "$times_file"
  fi

  rm -rf "$tmp"
}

# ---------- main ----------
BIN="$(resolve_bin)"
log "binary: $BIN"
log "small file: $SMALL_FILE"
log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"

REF="$(resolve_ref)"
log "expand ref: $REF"

# Run each component cold + warm.
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

# ---------- emit table ----------
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
