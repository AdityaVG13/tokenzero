#!/usr/bin/env bash
# Competitor head-to-head bake-off benchmark (bead tokenzero-8u9)
#
# Tasks: read 500 lines, grep + read matches, tree + glob + read, edit + verify,
# multi-step navigation.
# Competitors: tokenzero CLI, raw CLI, rtk, lean-ctx, headroom, ztk, context-mode.
# Metrics: wall time (ms), output bytes, estimated tokens = ceil(bytes/4).
#
# Usage:
#   ./benchmarks/competitor-bakeoff.sh
#   TOKENZERO_BIN=/path/to/tokenzero ./benchmarks/competitor-bakeoff.sh
#   ./benchmarks/competitor-bakeoff.sh > benchmarks/competitor-bakeoff-results.md

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${TOKENZERO_BIN:-$(command -v tokenzero 2>/dev/null || true)}"
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  BIN="${HOME}/.tokenzero/bin/tokenzero"
fi
if [[ ! -x "$BIN" ]]; then
  echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2
  exit 1
fi

RUNS="${RUNS:-5}"
WARMUP="${WARMUP:-1}"
README="$ROOT/README.md"
CRATES="$ROOT/crates"
PATTERN="pub fn"

WORK_DIR="$(mktemp -d /tmp/tz-bakeoff.XXXXXX)"
TMP_OUT="$WORK_DIR/out"
TMP_TIMES="$WORK_DIR/times"
TMP_HF="$WORK_DIR/hf.json"
SAMPLE="$WORK_DIR/sample_500.txt"
EDIT_FILE="$WORK_DIR/edit_sample.txt"
trap 'rm -rf "$WORK_DIR"' EXIT

log()  { printf '[bakeoff] %s\n' "$*" >&2; }
tok()  { python3 -c 'import sys; b=int(sys.argv[1]); print((b+3)//4)' "$1"; }

median_ms() {
  python3 - "$1" <<'PY'
import sys
xs = [float(x.strip()) for x in open(sys.argv[1]) if x.strip()]
xs.sort()
if not xs: print(0); raise SystemExit
print(int(xs[len(xs)//2] * 1000))
PY
}

# Run command $2 (a string safe for eval in the current shell) and print
# wall_ms\toutput_bytes\test_tokens for the named approach $1.
measure() {
  local label="$1" cmd="$2"
  local -i wall_ms=0 bytes=0 est=0

  if command -v hyperfine >/dev/null 2>&1; then
    hyperfine \
      --warmup "$WARMUP" \
      --runs "$RUNS" \
      --style basic \
      --export-json "$TMP_HF" \
      --command-name "$label" \
      "$cmd" \
      >/dev/null 2>&1 || true
    wall_ms=$(python3 - "$TMP_HF" <<'PY'
import json, sys
try:
    d = json.load(open(sys.argv[1]))
    r = d.get("results", [{}])[0]
    print(int(r.get("median", 0) * 1000))
except Exception:
    print(0)
PY
)
  else
    : > "$TMP_TIMES"
    local i
    for ((i=0; i<RUNS; i++)); do
      /usr/bin/time -f '%e' sh -c "$cmd" >/dev/null 2>>"$TMP_TIMES" || true
    done
    wall_ms=$(median_ms "$TMP_TIMES")
  fi

  # Capture stdout once for byte accounting (hyperfine discards it during timing).
  : > "$TMP_OUT"
  eval "$cmd" > "$TMP_OUT" 2>/dev/null || true
  bytes=$(wc -c < "$TMP_OUT")
  est=$(tok "$bytes")
  printf '%s\t%s\t%s\n' "$wall_ms" "$bytes" "$est"
}

emit_header() {
  printf '| task | tool | wall_ms | output_bytes | est_tokens | note |\n'
  printf '|---|---|---:|---:|---:|---|\n'
}

emit_row() {
  printf '| `%s` | `%s` | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6"
}

emit_skip() {
  emit_row "$1" "$2" "-" "-" "-" "not installed"
}

# TokenZero CLI row.
tz_row() {
  local task="$1" cmd="$2"
  local ms bytes est
  read -r ms bytes est <<<"$(measure "tokenzero:$task" "$cmd")"
  emit_row "$task" "tokenzero" "$ms" "$bytes" "$est" ""
}

# Raw CLI row.
raw_row() {
  local task="$1" cmd="$2"
  local ms bytes est
  read -r ms bytes est <<<"$(measure "raw:$task" "$cmd")"
  emit_row "$task" "raw-cli" "$ms" "$bytes" "$est" ""
}

# Competitor row: only measure if the tool binary is on PATH.
comp_row() {
  local task="$1" tool="$2" cmd="$3"
  if command -v "$tool" >/dev/null 2>&1; then
    local ms bytes est
    read -r ms bytes est <<<"$(measure "$tool:$task" "$cmd")"
    emit_row "$task" "$tool" "$ms" "$bytes" "$est" ""
  else
    emit_skip "$task" "$tool"
  fi
}

# ---------- setup ----------
log "binary: $BIN"
log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"

# Ensure a 500-line sample exists.
head -n 500 "$README" > "$SAMPLE" 2>/dev/null || {
  : > "$SAMPLE"
  while [[ $(wc -l < "$SAMPLE") -lt 500 ]]; do
    printf 'padding line %d\n' $(($(wc -l < "$SAMPLE") + 1)) >> "$SAMPLE"
  done
}

# Edit sample.
printf 'alpha\nbeta\ngamma\n' > "$EDIT_FILE"

# First Rust source for tree+glob+read task.
first_rs=$(find "$CRATES" -name "*.rs" -type f | head -n 1)

# Export variables that appear inside command strings passed to hyperfine.
export BIN ROOT README CRATES SAMPLE EDIT_FILE first_rs PATTERN files_str

# First three Rust files containing a public function for multi-step navigation.
hits=()
while IFS= read -r line; do
  hits+=("$line")
done < <(grep -rl "$PATTERN" "$CRATES" 2>/dev/null | head -n 3)
files_str=""
for f in "${hits[@]}"; do
  files_str="$files_str $(printf '%q' "$f")"
done
# Fallback: if no matches were found, use the first Rust file.
if [[ -z "$files_str" ]]; then
  files_str="$(printf '%q' "$first_rs")"
fi

# ---------- emit table ----------
emit_header

# 1. read 500-line file
task="read_500"
tz_row  "$task" "$BIN read --end-line 500 \"$SAMPLE\""
raw_row "$task" "cat \"$SAMPLE\""
comp_row "$task" "rtk"          "rtk read --limit 500 \"$SAMPLE\""
comp_row "$task" "lean-ctx"    "lean-ctx read \"$SAMPLE\" --limit 500"
comp_row "$task" "headroom"    "headroom read --lines 500 \"$SAMPLE\""
comp_row "$task" "ztk"         "ztk read --end-line 500 \"$SAMPLE\""
comp_row "$task" "context-mode" "context-mode read --limit 500 \"$SAMPLE\""

# 2. grep + read matches
task="grep_read"
tz_row  "$task" "$BIN grep 'TokenZero' \"$README\""
raw_row "$task" "grep -n 'TokenZero' \"$README\""
comp_row "$task" "rtk"          "rtk grep 'TokenZero' \"$README\""
comp_row "$task" "lean-ctx"    "lean-ctx grep 'TokenZero' \"$README\""
comp_row "$task" "headroom"    "headroom grep 'TokenZero' \"$README\""
comp_row "$task" "ztk"         "ztk grep 'TokenZero' \"$README\""
comp_row "$task" "context-mode" "context-mode grep 'TokenZero' \"$README\""

# 3. tree + glob + read
task="tree_glob_read"
tz_row  "$task" "$BIN tree \"$CRATES\" --depth 2; $BIN glob '*.rs' \"$CRATES\"; $BIN read \"$first_rs\""
raw_row "$task" "find \"$CRATES\" -maxdepth 2 -type f | sort; find \"$CRATES\" -name '*.rs' -type f | head -n 1; cat \"$first_rs\""
comp_row "$task" "rtk"          "rtk tree \"$CRATES\"; rtk glob '*.rs' \"$CRATES\"; rtk read \"$first_rs\""
comp_row "$task" "lean-ctx"    "lean-ctx tree \"$CRATES\"; lean-ctx glob '*.rs' \"$CRATES\"; lean-ctx read \"$first_rs\""
comp_row "$task" "headroom"    "headroom tree \"$CRATES\"; headroom glob '*.rs' \"$CRATES\"; headroom read \"$first_rs\""
comp_row "$task" "ztk"         "ztk tree \"$CRATES\"; ztk glob '*.rs' \"$CRATES\"; ztk read \"$first_rs\""
comp_row "$task" "context-mode" "context-mode tree \"$CRATES\"; context-mode glob '*.rs' \"$CRATES\"; context-mode read \"$first_rs\""

# 4. edit + verify
task="edit_verify"
tz_row  "$task" "$BIN edit --edits-json '[{\"find\":\"beta\",\"replace\":\"BETA\"}]' \"$EDIT_FILE\"; $BIN read \"$EDIT_FILE\""
raw_row "$task" "sed -i.bak 's/beta/BETA/g' \"$EDIT_FILE\"; rm -f \"$EDIT_FILE.bak\"; cat \"$EDIT_FILE\""
comp_row "$task" "rtk"          "rtk edit --find beta --replace BETA \"$EDIT_FILE\"; rtk read \"$EDIT_FILE\""
comp_row "$task" "lean-ctx"    "lean-ctx edit --find beta --replace BETA \"$EDIT_FILE\"; lean-ctx read \"$EDIT_FILE\""
comp_row "$task" "headroom"    "headroom edit --find beta --replace BETA \"$EDIT_FILE\"; headroom read \"$EDIT_FILE\""
comp_row "$task" "ztk"         "ztk edit --find beta --replace BETA \"$EDIT_FILE\"; ztk read \"$EDIT_FILE\""
comp_row "$task" "context-mode" "context-mode edit --find beta --replace BETA \"$EDIT_FILE\"; context-mode read \"$EDIT_FILE\""

# 5. multi-step navigation: grep a symbol across several files, then read them
task="multi_step"
tz_row  "$task" "$BIN grep '$PATTERN' $files_str; for f in $files_str; do $BIN read \"$f\"; done"
raw_row "$task" "grep -n '$PATTERN' $files_str; for f in $files_str; do cat \"$f\"; done"
comp_row "$task" "rtk"          "rtk grep '$PATTERN' $files_str; for f in $files_str; do rtk read \"$f\"; done"
comp_row "$task" "lean-ctx"    "lean-ctx grep '$PATTERN' $files_str; for f in $files_str; do lean-ctx read \"$f\"; done"
comp_row "$task" "headroom"    "headroom grep '$PATTERN' $files_str; for f in $files_str; do headroom read \"$f\"; done"
comp_row "$task" "ztk"         "ztk grep '$PATTERN' $files_str; for f in $files_str; do ztk read \"$f\"; done"
comp_row "$task" "context-mode" "context-mode grep '$PATTERN' $files_str; for f in $files_str; do context-mode read \"$f\"; done"

log "done."
