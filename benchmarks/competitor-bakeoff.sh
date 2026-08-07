#!/usr/bin/env bash
# Competitor head-to-head bake-off benchmark (bead tokenzero-8u9)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"; H=(python3 "$ROOT/benchmarks/harness.py")
BIN="$("${H[@]}" resolve_bin)" || { echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2; exit 1; }; RUNS="${RUNS:-5}"; WARMUP="${WARMUP:-1}"; README="$ROOT/README.md"; CRATES="$ROOT/crates"; PATTERN="pub fn"
WORK_DIR="$(mktemp -d /tmp/tz-bakeoff.XXXXXX)"; SAMPLE="$WORK_DIR/sample_500.txt"; EDIT_FILE="$WORK_DIR/edit_sample.txt"; GREP_CACHE="$WORK_DIR/grep-cache.json"
trap 'rm -rf "$WORK_DIR"' EXIT
log() { printf '[bakeoff] %s\n' "$*" >&2; }; measure() { "${H[@]}" measure_median "$1" "$2" --runs "$RUNS" --warmup "$WARMUP" --prepare "$3"; }
emit() { printf '| `%s` | `%s` | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6"; }
row() {
  local task="$1" tool="$2" cmd="$3" prepare="$4" metrics m b e
  if [[ "$tool" != tokenzero && "$tool" != raw-cli ]] && ! command -v "$tool" >/dev/null 2>&1; then
    emit "$task" "$tool" - - - "not installed"; return
  fi
  if ! metrics=$(measure "$tool:$task" "$cmd" "$prepare"); then
    log "FAILED: $tool:$task"
    return 1
  fi
  read -r m b e <<<"$metrics"
  if [[ "$task" == grep_read && "$tool" == tokenzero ]]; then
    GREP_TOKENZERO_BYTES="$b"; GREP_TOKENZERO_TOKENS="$e"
  elif [[ "$task" == grep_read && "$tool" == raw-cli ]]; then
    GREP_RAW_BYTES="$b"; GREP_RAW_TOKENS="$e"
  fi
  if [[ "$b" == 0 ]]; then
    emit "$task" "$tool" "$m" "$b" "$e" "ran but produced no output (arg mismatch with installed version)"
  elif [[ "$task" == grep_read && "$tool" == tokenzero ]]; then
    emit "$task" "$tool" "$m" "$b" "$e" "warm/dedup; Q99-Input candidate"
  elif [[ "$task" == grep_read && "$tool" == raw-cli ]]; then
    emit "$task" "$tool" "$m" "$b" "$e" "Q99-Input denominator"
  else
    emit "$task" "$tool" "$m" "$b" "$e" ""
  fi
}
prepare_for() {
  case "$1" in
    edit_verify) printf "%s > %q" "printf 'alpha\\nbeta\\ngamma'" "$EDIT_FILE" ;;
    *) printf 'true' ;;
  esac
}
command_for() {
  local task="$1" tool="$2" exe="$tool"; [[ "$tool" == tokenzero ]] && exe="$BIN"
  case "$task:$tool" in
    read_500:tokenzero) echo "$exe read --end-line 500 \"$SAMPLE\"";; read_500:raw-cli) echo "cat \"$SAMPLE\"";;
    read_500:rtk) echo "rtk read --limit 500 \"$SAMPLE\"";; read_500:lean-ctx) echo "lean-ctx read \"$SAMPLE\" --limit 500";;
    read_500:headroom) echo "headroom read --lines 500 \"$SAMPLE\"";; read_500:ztk) echo "ztk read --end-line 500 \"$SAMPLE\"";;
    read_500:context-mode) echo "context-mode read --limit 500 \"$SAMPLE\"";;
    grep_read:tokenzero) echo "TOKENZERO_CACHE_PATH=\"$GREP_CACHE\" $exe grep 'TokenZero' \"$README\"";;
    grep_read:raw-cli) echo "grep -n 'TokenZero' \"$README\"";; grep_read:*) echo "$exe grep 'TokenZero' \"$README\"";;
    tree_glob_read:raw-cli) echo "find \"$CRATES\" -maxdepth 2 -type f | sort; find \"$CRATES\" -name '*.rs' -type f | head -n 1; cat \"$first_rs\"";;
    tree_glob_read:tokenzero) echo "$exe tree \"$CRATES\" --depth 2; $exe glob '*.rs' \"$CRATES\"; $exe read \"$first_rs\"";;
    tree_glob_read:*) echo "$exe tree \"$CRATES\"; $exe glob '*.rs' \"$CRATES\"; $exe read \"$first_rs\"";;
    edit_verify:tokenzero) echo "cd \"$WORK_DIR\"; $exe edit --edits-json '[{\"find\":\"beta\",\"replace\":\"BETA\"}]' \"$EDIT_FILE\" && $exe read \"$EDIT_FILE\"";;
    edit_verify:raw-cli) echo "sed -i.bak 's/beta/BETA/g' \"$EDIT_FILE\"; rm -f \"$EDIT_FILE.bak\"; cat \"$EDIT_FILE\"";;
    edit_verify:*) echo "$exe edit --find beta --replace BETA \"$EDIT_FILE\"; $exe read \"$EDIT_FILE\"";;
    multi_step:raw-cli) echo "grep -n '$PATTERN' $files_str; for f in $files_str; do cat \"\$f\"; done";;
    multi_step:*) echo "$exe grep '$PATTERN' $files_str; for f in $files_str; do $exe read \"\$f\"; done";;
  esac
}
log "binary: $BIN"; log "hyperfine: $(command -v hyperfine || echo 'fallback: /usr/bin/time')"; head -n 500 "$README" > "$SAMPLE" 2>/dev/null || { : > "$SAMPLE"; while [[ $(wc -l < "$SAMPLE") -lt 500 ]]; do printf 'padding line %d\n' $(($(wc -l < "$SAMPLE")+1)) >> "$SAMPLE"; done; }
first_rs=$(find "$CRATES" -name "*.rs" -type f -print -quit)
hits=(); while IFS= read -r line; do hits+=("$line"); done < <(grep -rl "$PATTERN" "$CRATES" 2>/dev/null | sed -n '1,3p')
files_str=""; for file in "${hits[@]}"; do files_str="$files_str $(printf '%q' "$file")"; done
[[ -n "$files_str" ]] || files_str="$(printf '%q' "$first_rs")"
log 'priming isolated warm/dedup grep cache outside timing'
TOKENZERO_CACHE_PATH="$GREP_CACHE" "$BIN" grep 'TokenZero' "$README" >/dev/null
printf '| task | tool | wall_ms | output_bytes | est_tokens | note |\n|---|---|---:|---:|---:|---|\n'
tools=(tokenzero raw-cli rtk lean-ctx headroom ztk context-mode)
for task in read_500 grep_read tree_glob_read edit_verify multi_step; do
  for tool in "${tools[@]}"; do row "$task" "$tool" "$(command_for "$task" "$tool")" "$(prepare_for "$task")"; done
done
if [[ -z "${GREP_TOKENZERO_TOKENS:-}" || -z "${GREP_RAW_TOKENS:-}" || "$GREP_RAW_TOKENS" -le 0 ]]; then
  log 'FAILED: missing Q99-Input grep receipt'
  exit 1
fi
q99_input_saved_tokens=$((GREP_RAW_TOKENS - GREP_TOKENZERO_TOKENS))
q99_input_savings_ppm=$((q99_input_saved_tokens * 1000000 / GREP_RAW_TOKENS))
printf '\nQ99-Input receipt: candidate=%s bytes/%s estimated tokens; denominator raw-cli=%s bytes/%s estimated tokens; numerator saved=%s estimated tokens; savings=%s ppm; gate >=850000 ppm: %s.\n' \
  "$GREP_TOKENZERO_BYTES" "$GREP_TOKENZERO_TOKENS" "$GREP_RAW_BYTES" "$GREP_RAW_TOKENS" \
  "$q99_input_saved_tokens" "$q99_input_savings_ppm" \
  "$([[ "$q99_input_savings_ppm" -ge 850000 ]] && printf PASS || printf FAIL)"
if [[ "$q99_input_savings_ppm" -lt 850000 ]]; then
  exit 1
fi
log done.
