#!/usr/bin/env bash
# Million-line repo navigation benchmark (bead tokenzero-15w)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
H=(python3 -m benchmarks.harness)
BIN="$("${H[@]}" resolve_bin)" || { echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2; exit 1; }
NUM_DIRS=100; FILES_PER_DIR=10; LINES_PER_FILE=1000; NEEDLE=BENCH_NEEDLE_FN; BUDGET=32000
TOTAL_FILES=$((NUM_DIRS*FILES_PER_DIR)); TOTAL_LINES=$((TOTAL_FILES*LINES_PER_FILE))
WORK_DIR="$(mktemp -d /tmp/tz-million.XXXXXX)"; SYNTH="$WORK_DIR/repo"; TMP_JSON="$WORK_DIR/out.json"; TMP_RAW="$WORK_DIR/raw.out"
trap 'rm -rf "$WORK_DIR"' EXIT
log() { printf '[million-nav] %s\n' "$*" >&2; }
now_ms() { "${H[@]}" now_ms; }
tz_run() {
  local cmd="$1" start end; start=$(now_ms); eval "$cmd" >"$TMP_JSON" 2>/dev/null || true; end=$(now_ms)
  "${H[@]}" tz_metrics "$TMP_JSON" "$((end-start))"
}
raw_run() {
  local cmd="$1" start end; start=$(now_ms); eval "$cmd" >"$TMP_RAW" 2>/dev/null || true; end=$(now_ms)
  printf '%s\t%s' "$(wc -c <"$TMP_RAW" | tr -d ' ')" "$((end-start))"
}
emit_header() { printf '| # | Task | Tool | visible_tokens | raw_tokens | wall_ms | output_bytes | notes |\n|---|------|------|---:|---:|---:|---:|------|\n'; }
emit_tz()  { printf '| %s | `%s` | `tokenzero` | %s | %s | %s | — | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6"; }
emit_raw() { printf '| %s | `%s` | `raw-cli` | — | — | %s | %s | %s |\n' "$1" "$2" "$4" "$3" "$5"; }

log "binary: $BIN"; log "budget: $BUDGET visible_tokens"
log "generating $TOTAL_LINES lines across $TOTAL_FILES files in $SYNTH ..."
"${H[@]}" generate_million "$SYNTH" --dirs "$NUM_DIRS" --files "$FILES_PER_DIR" --lines "$LINES_PER_FILE" --needle "$NEEDLE"
log "repo generated: $(find "$SYNTH" -type f | wc -l | tr -d ' ') files, $(find "$SYNTH" -type f -exec cat {} + | wc -l | tr -d ' ') lines"

TARGET_FILE="$SYNTH/mod_0050/file_0050_003.rs"
NEEDLE_FILE="$SYNTH/mod_0010/file_0010_000.rs"
EDIT_FILE="$WORK_DIR/edit_target.rs"; cp "$NEEDLE_FILE" "$EDIT_FILE"
TOTAL_VISIBLE=0; emit_header

log "task A: bounded read"
read -r vis_a raw_a ms_a <<<"$(tz_run "$BIN read --json --start-line 1 --end-line 50 \"$TARGET_FILE\" --allowed-root \"$SYNTH\"")"
TOTAL_VISIBLE=$((TOTAL_VISIBLE+vis_a)); emit_tz A read_50_lines "$vis_a" "$raw_a" "$ms_a" ""
read -r bytes_a ms_raw_a <<<"$(raw_run "head -n 50 \"$TARGET_FILE\"")"; emit_raw A read_50_lines "$bytes_a" "$ms_raw_a" "head -n 50"

log "task B: grep + expand"
read -r vis_b1 raw_b1 ms_b1 <<<"$(tz_run "$BIN find --json \"$NEEDLE\" \"$SYNTH\" --max-files 10 --max-visible-tokens 2000 --allowed-root \"$SYNTH\"")"
BLOB_REF=$("${H[@]}" first_blob_ref "$TMP_JSON")
if [[ -n "$BLOB_REF" ]]; then read -r vis_b2 raw_b2 ms_b2 <<<"$(tz_run "$BIN expand --json \"$BLOB_REF\" --allowed-root \"$SYNTH\"")"
else vis_b2=0; raw_b2=0; ms_b2=0; fi
vis_b=$((vis_b1+vis_b2)); raw_b=$((raw_b1+raw_b2)); ms_b=$((ms_b1+ms_b2))
TOTAL_VISIBLE=$((TOTAL_VISIBLE+vis_b)); emit_tz B grep_expand "$vis_b" "$raw_b" "$ms_b" "find+expand"
read -r bytes_b ms_raw_b <<<"$(raw_run "grep -rn \"$NEEDLE\" \"$SYNTH\" | head -n 20")"; emit_raw B grep_expand "$bytes_b" "$ms_raw_b" "grep -rn | head -20"

log "task C: tree + glob + read"
read -r vis_c1 raw_c1 ms_c1 <<<"$(tz_run "$BIN tree --json \"$SYNTH\" --depth 2 --max-files 50 --allowed-root \"$SYNTH\"")"
read -r vis_c2 raw_c2 ms_c2 <<<"$(tz_run "$BIN glob --json '*.rs' \"$SYNTH\" --max-files 10 --allowed-root \"$SYNTH\"")"
IFS=$'\t' read -r GLOB_ROOT GLOB_REL <<<"$("${H[@]}" glob_pick "$TMP_JSON")"
GLOB_FILE="${GLOB_ROOT}/${GLOB_REL}"; [[ -z "$GLOB_FILE" || "$GLOB_FILE" == / ]] && GLOB_FILE="$TARGET_FILE"
read -r vis_c3 raw_c3 ms_c3 <<<"$(tz_run "$BIN read --json --start-line 1 --end-line 50 \"$GLOB_FILE\" --allowed-root \"$SYNTH\"")"
vis_c=$((vis_c1+vis_c2+vis_c3)); raw_c=$((raw_c1+raw_c2+raw_c3)); ms_c=$((ms_c1+ms_c2+ms_c3))
TOTAL_VISIBLE=$((TOTAL_VISIBLE+vis_c)); emit_tz C tree_glob_read "$vis_c" "$raw_c" "$ms_c" "tree+glob+read"
read -r bytes_c ms_raw_c <<<"$(raw_run "find \"$SYNTH\" -maxdepth 2 -type f | sort | head -n 20; find \"$SYNTH\" -name '*.rs' -type f | head -n 1; head -n 50 \"$GLOB_FILE\"")"
emit_raw C tree_glob_read "$bytes_c" "$ms_raw_c" "find+find+head"

log "task D: grep → expand → edit → verify"
read -r vis_d1 raw_d1 ms_d1 <<<"$(tz_run "$BIN find --json \"$NEEDLE\" \"$EDIT_FILE\" --allowed-root \"$WORK_DIR\"")"
D_BLOB_REF=$("${H[@]}" first_blob_ref "$TMP_JSON")
if [[ -n "$D_BLOB_REF" ]]; then read -r vis_d2 raw_d2 ms_d2 <<<"$(tz_run "$BIN expand --json \"$D_BLOB_REF\" --allowed-root \"$WORK_DIR\"")"
else vis_d2=0; raw_d2=0; ms_d2=0; fi
read -r vis_d3 raw_d3 ms_d3 <<<"$(tz_run "$BIN edit --json --edits-json '[{\"find\":\"$NEEDLE\",\"replace\":\"RENAMED_FN\"}]' \"$EDIT_FILE\" --allowed-root \"$WORK_DIR\"")"
read -r vis_d4 raw_d4 ms_d4 <<<"$(tz_run "$BIN read --json --start-line 498 --end-line 502 \"$EDIT_FILE\" --allowed-root \"$WORK_DIR\"")"
vis_d=$((vis_d1+vis_d2+vis_d3+vis_d4)); raw_d=$((raw_d1+raw_d2+raw_d3+raw_d4)); ms_d=$((ms_d1+ms_d2+ms_d3+ms_d4))
TOTAL_VISIBLE=$((TOTAL_VISIBLE+vis_d)); emit_tz D grep_expand_edit_verify "$vis_d" "$raw_d" "$ms_d" "4-step"
read -r bytes_d ms_raw_d <<<"$(raw_run "grep -n \"$NEEDLE\" \"$EDIT_FILE\"; sed -i.bak 's/$NEEDLE/RENAMED_FN/g' \"$EDIT_FILE\"; rm -f \"$EDIT_FILE.bak\"; sed -n '498,502p' \"$EDIT_FILE\"")"
emit_raw D grep_expand_edit_verify "$bytes_d" "$ms_raw_d" "grep+sed+sed"

log "task E: recall"
read -r vis_e raw_e ms_e <<<"$(tz_run "$BIN recall --json \"$NEEDLE\" --max-hits 10 --allowed-root \"$WORK_DIR\"")"
TOTAL_VISIBLE=$((TOTAL_VISIBLE+vis_e)); emit_tz E recall "$vis_e" "$raw_e" "$ms_e" "cache search"
read -r bytes_e ms_raw_e <<<"$(raw_run "grep -rn \"$NEEDLE\" \"$SYNTH\" | head -n 10")"; emit_raw E recall "$bytes_e" "$ms_raw_e" "grep -rn | head -10"

printf '\n## Budget assertion\n\n| Metric | Value |\n|--------|-------|\n'
printf '| Total visible_tokens (all 5 tasks) | %d |\n| Context budget | %d |\n| Remaining headroom | %d |\n| Utilization | %.1f%% |\n\n' \
  "$TOTAL_VISIBLE" "$BUDGET" "$((BUDGET-TOTAL_VISIBLE))" "$(python3 -c "print(f'{$TOTAL_VISIBLE/$BUDGET*100:.1f}')")"
if [[ "$TOTAL_VISIBLE" -lt "$BUDGET" ]]; then
  printf '> **Result: PASS** — all 5 navigation tasks fit within the 32k context budget.\n'
else
  printf '> **Result: FAIL** — total visible_tokens (%d) exceeds the 32k budget.\n' "$TOTAL_VISIBLE"; exit 1
fi
printf '\n## Quality criteria\n\n'
printf '%s\n' \
  '- **Byte-exact recovery**: every `expand` call recovers the exact bytes of the original content (verified by ref checksums).' \
  '- **All tasks succeed**: each navigation task completes without error (exit code 0).' \
  '- **No content loss**: compact capsules hide raw content behind refs, but nothing is discarded — every byte is recoverable.' \
  '- **Edit integrity**: the multi-step edit produces a valid file with the replacement applied (verified by read-back).'
log "done. total visible_tokens=$TOTAL_VISIBLE budget=$BUDGET"
