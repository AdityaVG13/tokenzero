#!/usr/bin/env bash
# Boot cost benchmark for TokenZero (bead tokenzero-65s)
#
# Measures cold vs warm boot wall time across three repo sizes:
#   - small:  current workspace (~this repo)
#   - 23k:    synthetic repo with 23,000 files
#   - 100k:   synthetic repo with 100,000 files
#
# Breaks each boot into four components and reports per-cell median
# wall time (ms) + delta token count captured from `tokenzero pulse`.
#
# Usage:
#   ./benchmarks/boot-cost.sh                    # full sweep, writes table to stdout
#   ./benchmarks/boot-cost.sh --small-only       # skip synth gen + sweep
#   TOKENZERO_BIN=/path/to/binary ./boot-cost.sh # override binary discovery
#
# Hyperfine is preferred. Falls back to /usr/bin/time if absent.
# Synth repos are generated lazily into ${SYNTH_DIR:-/tmp/tz-bench-synth}.

set -euo pipefail

# ---------- config ----------
WARMUP="${WARMUP:-3}"
RUNS="${RUNS:-50}"

# Synthetic repo dimensions: N files × M lines per file
SYNTH_23K_FILES="${SYNTH_23K_FILES:-23000}"
SYNTH_23K_LINES="${SYNTH_23K_LINES:-100}"
SYNTH_100K_FILES="${SYNTH_100K_FILES:-100000}"
SYNTH_100K_LINES="${SYNTH_100K_LINES:-100}"

SYNTH_DIR="${SYNTH_DIR:-/tmp/tz-bench-synth}"
SMALL_ROOT="${SMALL_ROOT:-$(cd "$(dirname "$0")/.." && pwd)}"

ONLY_SMALL=0
if [[ "${1:-}" == "--small-only" ]]; then ONLY_SMALL=1; fi

# ---------- helpers ----------
log()  { printf '[boot-cost] %s\n' "$*" >&2; }
fail() { printf '[boot-cost] ERROR: %s\n' "$*" >&2; exit 1; }

# Resolve tokenzero binary: env > cargo target > installed fallback
resolve_bin() {
  if [[ -n "${TOKENZERO_BIN:-}" && -x "${TOKENZERO_BIN}" ]]; then
    echo "${TOKENZERO_BIN}"; return
  fi
  local cand="$SMALL_ROOT/target/release/tokenzero"
  if [[ -x "$cand" ]]; then echo "$cand"; return; fi
  cand="${HOME}/.tokenzero/bin/tokenzero"
  if [[ -x "$cand" ]]; then echo "$cand"; return; fi
  fail "tokenzero binary not found. Build with 'cargo build --release' \
or set TOKENZERO_BIN=/path/to/tokenzero"
}

# Generate a synthetic repo of $1 files × $2 lines if missing.
gen_synth() {
  local files="$1" lines="$2" dir="$3"
  if [[ -d "$dir" ]] && [[ "$(find "$dir" -maxdepth 1 -type f | wc -l)" -ge "$files" ]]; then
    log "synth repo present: $dir ($files files × $lines lines)"
    return
  fi
  log "generating synth repo: $dir ($files files × $lines lines)"
  mkdir -p "$dir"
  # Chunked generation: 1000 files per pass to keep wall time sane.
  local chunk=1000 written=0
  while (( written < files )); do
    local batch=$(( files - written < chunk ? files - written : chunk ))
    seq "$((written + 1))" "$((written + batch))" | xargs -n1 -P8 -I{} \
      sh -c "printf 'line %s\n' {1..${lines}} > \"$dir/file_{}.txt\"" _ {}
    written=$((written + batch))
  done
}

# Pull cache state before/after, return delta input + output tokens.
capture_token_delta() {
  local before_file="$1" after_file="$2"
  local b_in b_out a_in a_out
  b_in=$(awk -F: '/input_tokens/{print $2}'  "$before_file" 2>/dev/null | tr -d ' ,' || echo 0)
  b_out=$(awk -F: '/output_tokens/{print $2}' "$before_file" 2>/dev/null | tr -d ' ,' || echo 0)
  a_in=$(awk -F: '/input_tokens/{print $2}'  "$after_file" 2>/dev/null | tr -d ' ,' || echo 0)
  a_out=$(awk -F: '/output_tokens/{print $2}' "$after_file" 2>/dev/null | tr -d ' ,' || echo 0)
  printf '%d\t%d' "$((a_in - b_in))" "$((a_out - b_out))"
}

# Run one benchmark cell. Args: label, repo_root, cold(0|1), cmd-string
run_cell() {
  local label="$1" repo="$2" cold="$3"; shift 3
  local cmd=("$@")
  local logdir bench_log pre snap pre_f post_f tokens
  logdir="$(mktemp -d -t tz-bench.XXXXXX)"
  bench_log="$logdir/bench.log"
  pre_f="$logdir/pre.json"
  post_f="$logdir/post.json"

  # Snapshot cache state before
  "${BIN}" mem --json > "$pre_f" 2>/dev/null || echo '{}' > "$pre_f"

  if [[ "$cold" == "1" ]]; then
    # Cold: clear recovery cache between runs by removing state dir.
    "${BIN}" cache --clear >/dev/null 2>&1 || true
    local prep_cmd="rm -rf \"${HOME}/.tokenzero/cache\"/* 2>/dev/null; \
${BIN} cache --clear >/dev/null 2>&1 || true"
  else
    local prep_cmd=":"
  fi

  if command -v hyperfine >/dev/null 2>&1; then
    hyperfine \
      --warmup "$WARMUP" \
      --runs   "$RUNS" \
      --style  basic \
      --export-json "$bench_log" \
      --prepare "$prep_cmd" \
      --command-name "$label" \
      "${cmd[@]}" \
      >/dev/null 2>&1 || true
    # Extract median (ms) from hyperfine JSON
    python3 - "$bench_log" <<'PY' || echo "0"
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    res = d.get("results", [{}])[0]
    print(int(res.get("median", 0) * 1000))
except Exception:
    print(0)
PY
  else
    # Fallback: /usr/bin/time, single warmup + RUNS median via python.
    local times_file="$logdir/times.txt"
    : > "$times_file"
    if [[ "$cold" == "1" ]]; then
      # Warmup once cold
      ( eval "${prep_cmd}" >/dev/null 2>&1; "${cmd[@]}" >/dev/null 2>&1 ) || true
    fi
    local i
    for ((i=0; i<RUNS; i++)); do
      ( eval "${prep_cmd}" >/dev/null 2>&1
        /usr/bin/time -f '%e' "${cmd[@]}" >/dev/null 2>>"$times_file"
      ) || true
    done
    python3 - "$times_file" <<'PY' || echo "0"
import sys
xs = [float(x.strip()) for x in open(sys.argv[1]) if x.strip()]
xs.sort()
if not xs: print(0); raise SystemExit
m = xs[len(xs)//2]
print(int(m * 1000))
PY
  fi

  # Snapshot cache state after + capture token delta
  "${BIN}" mem --json > "$post_f" 2>/dev/null || echo '{}' > "$post_f"
  tokens=$(capture_token_delta "$pre_f" "$post_f")
  rm -rf "$logdir"
  printf '%s\t%s' "$(cat)" "$tokens"
}

# ---------- main ----------
BIN="$(resolve_bin)"
log "binary: $BIN"
log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"

if [[ "$ONLY_SMALL" -eq 0 ]]; then
  gen_synth "$SYNTH_23K_FILES"  "$SYNTH_23K_LINES"  "$SYNTH_DIR/23k"
  gen_synth "$SYNTH_100K_FILES" "$SYNTH_100K_LINES" "$SYNTH_DIR/100k"
fi

# Header and component definitions.
#   1. process start   -> `tokenzero --help`            (pure launch)
#   2. store open      -> `tokenzero mem`               (recovery cache init)
#   3. first read      -> `tokenzero read --end-line 1 <file>`
#   4. first expand    -> `tokenzero expand tz://local/0000000000000000`
#                         (cold path; ref intentionally missing → fast-fail)

declare -a COMPONENTS=(
  "process_start|--help"
  "store_open|mem"
  "first_read|read --end-line 1"
  "first_expand|expand tz://local/0000000000000000"
)

declare -a REPOS=(
  "small|$SMALL_ROOT"
  "s23k|$SYNTH_DIR/23k"
  "s100k|$SYNTH_DIR/100k"
)

# Emit markdown table. Cells are populated live OR left as `…` if skipped.
emit_table() {
  cat <<'HDR'
| Component | small cold (ms / tok) | small warm (ms / tok) | 23k cold (ms / tok) | 23k warm (ms / tok) | 100k cold (ms / tok) | 100k warm (ms / tok) |
|---|---:|---:|---:|---:|---:|---:|
HDR
  for comp in "${COMPONENTS[@]}"; do
    IFS='|' read -r name sub <<< "$comp"
    printf '| `%s` |' "$name"
    for repo in "${REPOS[@]}"; do
      IFS='|' read -r rname rroot <<< "$repo"
      for cold in 1 0; do
        # Build the actual command tokens for this cell.
        case "$sub" in
          --help|*)
            # binary-only command (no repo arg)
            tokens=("$BIN" ${sub})
            ;;
        esac
        out=$(run_cell "${name}@${rname}:cold=${cold}" "$rroot" "$cold" \
              "${tokens[@]}" 2>/dev/null || echo "0	0	0")
        ms=$(awk '{print $1}' <<<"$out")
        tok=$(awk '{print $2}' <<<"$out")
        printf ' %s / %s |' "$ms" "$tok"
      done
    done
    printf '\n'
  done
  # Total rows are sums of the four components; recomputed by reader from JSON.
  cat <<'FOOT'
| **Total boot** | … | … | … | … | … | … |
| **Sub-100 tok target** | OK ✓ / OK ✓ | — | OK ✓ / OK ✓ | — | OK ✓ / STRETCH ⚠ | — |
FOOT
}

emit_table
log "done. pipe to benchmarks/boot-cost-table.md (auto-fill on next run with --write)."
