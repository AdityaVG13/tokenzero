#!/usr/bin/env bash
# Competitor head-to-head bake-off benchmark (bead tokenzero-8u9)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
H=(python3 -m benchmarks.harness)
BIN="$("${H[@]}" resolve_bin)" || { echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2; exit 1; }
RUNS="${RUNS:-5}"; WARMUP="${WARMUP:-1}"
README="$ROOT/README.md"; CRATES="$ROOT/crates"; PATTERN="pub fn"
WORK_DIR="$(mktemp -d /tmp/tz-bakeoff.XXXXXX)"; SAMPLE="$WORK_DIR/sample_500.txt"; EDIT_FILE="$WORK_DIR/edit_sample.txt"
trap 'rm -rf "$WORK_DIR"' EXIT
log() { printf '[bakeoff] %s\n' "$*" >&2; }
measure() { "${H[@]}" measure_median "$1" "$2" --runs "$RUNS" --warmup "$WARMUP"; }
emit_header() { printf '| task | tool | wall_ms | output_bytes | est_tokens | note |\n|---|---|---:|---:|---:|---|\n'; }
emit_row() { printf '| `%s` | `%s` | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6"; }
tz_row()  { local m b e; read -r m b e <<<"$(measure "tokenzero:$1" "$2")"; emit_row "$1" tokenzero "$m" "$b" "$e" ""; }
raw_row() { local m b e; read -r m b e <<<"$(measure "raw:$1" "$2")"; emit_row "$1" raw-cli "$m" "$b" "$e" ""; }
comp_row() {
  local task="$1" tool="$2" cmd="$3"
  if command -v "$tool" >/dev/null 2>&1; then
    local m b e; read -r m b e <<<"$(measure "$tool:$task" "$cmd")"; emit_row "$task" "$tool" "$m" "$b" "$e" ""
  else
    emit_row "$task" "$tool" - - - "not installed"
  fi
}

log "binary: $BIN"; log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"
head -n 500 "$README" > "$SAMPLE" 2>/dev/null || { : > "$SAMPLE"; while [[ $(wc -l < "$SAMPLE") -lt 500 ]]; do printf 'padding line %d\n' $(($(wc -l < "$SAMPLE")+1)) >> "$SAMPLE"; done; }
printf 'alpha\nbeta\ngamma\n' > "$EDIT_FILE"
first_rs=$(find "$CRATES" -name "*.rs" -type f | head -n 1)
export BIN ROOT README CRATES SAMPLE EDIT_FILE first_rs PATTERN files_str
hits=(); while IFS= read -r line; do hits+=("$line"); done < <(grep -rl "$PATTERN" "$CRATES" 2>/dev/null | head -n 3)
files_str=""; for f in "${hits[@]}"; do files_str="$files_str $(printf '%q' "$f")"; done
[[ -z "$files_str" ]] && files_str="$(printf '%q' "$first_rs")"
emit_header

task=read_500
tz_row  "$task" "$BIN read --end-line 500 \"$SAMPLE\""
raw_row "$task" "cat \"$SAMPLE\""
comp_row "$task" rtk "rtk read --limit 500 \"$SAMPLE\""
comp_row "$task" lean-ctx "lean-ctx read \"$SAMPLE\" --limit 500"
comp_row "$task" headroom "headroom read --lines 500 \"$SAMPLE\""
comp_row "$task" ztk "ztk read --end-line 500 \"$SAMPLE\""
comp_row "$task" context-mode "context-mode read --limit 500 \"$SAMPLE\""

task=grep_read
tz_row  "$task" "$BIN grep 'TokenZero' \"$README\""
raw_row "$task" "grep -n 'TokenZero' \"$README\""
comp_row "$task" rtk "rtk grep 'TokenZero' \"$README\""
comp_row "$task" lean-ctx "lean-ctx grep 'TokenZero' \"$README\""
comp_row "$task" headroom "headroom grep 'TokenZero' \"$README\""
comp_row "$task" ztk "ztk grep 'TokenZero' \"$README\""
comp_row "$task" context-mode "context-mode grep 'TokenZero' \"$README\""

task=tree_glob_read
tz_row  "$task" "$BIN tree \"$CRATES\" --depth 2; $BIN glob '*.rs' \"$CRATES\"; $BIN read \"$first_rs\""
raw_row "$task" "find \"$CRATES\" -maxdepth 2 -type f | sort; find \"$CRATES\" -name '*.rs' -type f | head -n 1; cat \"$first_rs\""
comp_row "$task" rtk "rtk tree \"$CRATES\"; rtk glob '*.rs' \"$CRATES\"; rtk read \"$first_rs\""
comp_row "$task" lean-ctx "lean-ctx tree \"$CRATES\"; lean-ctx glob '*.rs' \"$CRATES\"; lean-ctx read \"$first_rs\""
comp_row "$task" headroom "headroom tree \"$CRATES\"; headroom glob '*.rs' \"$CRATES\"; headroom read \"$first_rs\""
comp_row "$task" ztk "ztk tree \"$CRATES\"; ztk glob '*.rs' \"$CRATES\"; ztk read \"$first_rs\""
comp_row "$task" context-mode "context-mode tree \"$CRATES\"; context-mode glob '*.rs' \"$CRATES\"; context-mode read \"$first_rs\""

task=edit_verify
tz_row  "$task" "$BIN edit --edits-json '[{\"find\":\"beta\",\"replace\":\"BETA\"}]' \"$EDIT_FILE\"; $BIN read \"$EDIT_FILE\""
raw_row "$task" "sed -i.bak 's/beta/BETA/g' \"$EDIT_FILE\"; rm -f \"$EDIT_FILE.bak\"; cat \"$EDIT_FILE\""
comp_row "$task" rtk "rtk edit --find beta --replace BETA \"$EDIT_FILE\"; rtk read \"$EDIT_FILE\""
comp_row "$task" lean-ctx "lean-ctx edit --find beta --replace BETA \"$EDIT_FILE\"; lean-ctx read \"$EDIT_FILE\""
comp_row "$task" headroom "headroom edit --find beta --replace BETA \"$EDIT_FILE\"; headroom read \"$EDIT_FILE\""
comp_row "$task" ztk "ztk edit --find beta --replace BETA \"$EDIT_FILE\"; ztk read \"$EDIT_FILE\""
comp_row "$task" context-mode "context-mode edit --find beta --replace BETA \"$EDIT_FILE\"; context-mode read \"$EDIT_FILE\""

task=multi_step
tz_row  "$task" "$BIN grep '$PATTERN' $files_str; for f in $files_str; do $BIN read \"$f\"; done"
raw_row "$task" "grep -n '$PATTERN' $files_str; for f in $files_str; do cat \"$f\"; done"
comp_row "$task" rtk "rtk grep '$PATTERN' $files_str; for f in $files_str; do rtk read \"$f\"; done"
comp_row "$task" lean-ctx "lean-ctx grep '$PATTERN' $files_str; for f in $files_str; do lean-ctx read \"$f\"; done"
comp_row "$task" headroom "headroom grep '$PATTERN' $files_str; for f in $files_str; do headroom read \"$f\"; done"
comp_row "$task" ztk "ztk grep '$PATTERN' $files_str; for f in $files_str; do ztk read \"$f\"; done"
comp_row "$task" context-mode "context-mode grep '$PATTERN' $files_str; for f in $files_str; do context-mode read \"$f\"; done"
log done.
