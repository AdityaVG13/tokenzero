#!/usr/bin/env bash
# Competitor head-to-head bake-off benchmark (bead tokenzero-8u9)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"; H=(python3 "$ROOT/benchmarks/harness.py")
BIN="$("${H[@]}" resolve_bin)" || { echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2; exit 1; }; RUNS="${RUNS:-5}"; WARMUP="${WARMUP:-1}"; README="$ROOT/README.md"; CRATES="$ROOT/crates"; PATTERN="pub fn"
WORK_DIR="$(mktemp -d /tmp/tz-bakeoff.XXXXXX)"; SAMPLE="$WORK_DIR/sample_500.txt"; EDIT_FILE="$WORK_DIR/edit_sample.txt"
trap 'rm -rf "$WORK_DIR"' EXIT
log() { printf '[bakeoff] %s\n' "$*" >&2; }; measure() { "${H[@]}" measure_median "$1" "$2" --runs "$RUNS" --warmup "$WARMUP"; }
emit() { printf '| `%s` | `%s` | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6"; }
row() {
  local task="$1" tool="$2" cmd="$3" m b e
  if [[ "$tool" != tokenzero && "$tool" != raw-cli ]] && ! command -v "$tool" >/dev/null 2>&1; then
    emit "$task" "$tool" - - - "not installed"; return
  fi
  read -r m b e <<<"$(measure "$tool:$task" "$cmd")"; emit "$task" "$tool" "$m" "$b" "$e" ""
}
command_for() {
  local task="$1" tool="$2" exe="$tool"; [[ "$tool" == tokenzero ]] && exe="$BIN"
  case "$task:$tool" in
    read_500:tokenzero) echo "$exe read --end-line 500 \"$SAMPLE\"";; read_500:raw-cli) echo "cat \"$SAMPLE\"";;
    read_500:rtk) echo "rtk read --limit 500 \"$SAMPLE\"";; read_500:lean-ctx) echo "lean-ctx read \"$SAMPLE\" --limit 500";;
    read_500:headroom) echo "headroom read --lines 500 \"$SAMPLE\"";; read_500:ztk) echo "ztk read --end-line 500 \"$SAMPLE\"";;
    read_500:context-mode) echo "context-mode read --limit 500 \"$SAMPLE\"";;
    grep_read:raw-cli) echo "grep -n 'TokenZero' \"$README\"";; grep_read:*) echo "$exe grep 'TokenZero' \"$README\"";;
    tree_glob_read:raw-cli) echo "find \"$CRATES\" -maxdepth 2 -type f | sort; find \"$CRATES\" -name '*.rs' -type f | head -n 1; cat \"$first_rs\"";;
    tree_glob_read:tokenzero) echo "$exe tree \"$CRATES\" --depth 2; $exe glob '*.rs' \"$CRATES\"; $exe read \"$first_rs\"";;
    tree_glob_read:*) echo "$exe tree \"$CRATES\"; $exe glob '*.rs' \"$CRATES\"; $exe read \"$first_rs\"";;
    edit_verify:tokenzero) echo "$exe edit --edits-json '[{\"find\":\"beta\",\"replace\":\"BETA\"}]' \"$EDIT_FILE\"; $exe read \"$EDIT_FILE\"";;
    edit_verify:raw-cli) echo "sed -i.bak 's/beta/BETA/g' \"$EDIT_FILE\"; rm -f \"$EDIT_FILE.bak\"; cat \"$EDIT_FILE\"";;
    edit_verify:*) echo "$exe edit --find beta --replace BETA \"$EDIT_FILE\"; $exe read \"$EDIT_FILE\"";;
    multi_step:raw-cli) echo "grep -n '$PATTERN' $files_str; for f in $files_str; do cat \"\$f\"; done";;
    multi_step:*) echo "$exe grep '$PATTERN' $files_str; for f in $files_str; do $exe read \"\$f\"; done";;
  esac
}
log "binary: $BIN"; log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"; head -n 500 "$README" > "$SAMPLE" 2>/dev/null || { : > "$SAMPLE"; while [[ $(wc -l < "$SAMPLE") -lt 500 ]]; do printf 'padding line %d\n' $(($(wc -l < "$SAMPLE")+1)) >> "$SAMPLE"; done; }
printf 'alpha\nbeta\ngamma\n' > "$EDIT_FILE"
first_rs=$(find "$CRATES" -name "*.rs" -type f | head -n 1)
hits=(); while IFS= read -r line; do hits+=("$line"); done < <(grep -rl "$PATTERN" "$CRATES" 2>/dev/null | head -n 3)
files_str=""; for file in "${hits[@]}"; do files_str="$files_str $(printf '%q' "$file")"; done
[[ -n "$files_str" ]] || files_str="$(printf '%q' "$first_rs")"; printf '| task | tool | wall_ms | output_bytes | est_tokens | note |\n|---|---|---:|---:|---:|---|\n'
tools=(tokenzero raw-cli rtk lean-ctx headroom ztk context-mode)
for task in read_500 grep_read tree_glob_read edit_verify multi_step; do
  for tool in "${tools[@]}"; do row "$task" "$tool" "$(command_for "$task" "$tool")"; done
done
log done.
