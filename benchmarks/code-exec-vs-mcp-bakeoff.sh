#!/usr/bin/env bash
# Code-execution vs MCP-schema token bake-off (bead tokenzero-da8.1)
#
# Fixed task suite (5 agent tasks) on the TokenZero public corpus.
# Baselines:
#   a) MCP schema load  — simulate by counting JSON schema bytes from
#      `tokenzero capabilities --json` for the tools each task needs.
#   b) CLI-only         — real `tokenzero` CLI commands; tokens from the
#      binary's own accounting (accounting.raw_tokens) plus command bytes.
#   c) CodeMode         — real `tokenzero codemode` JS plans; tokens from
#      the codemode result envelope (value.raw_tokens).
#
# Output: markdown table — task, approach, input_tokens, output_tokens,
#         turns, wall_ms, quality.
#
# Does NOT modify the repository. Edit tasks operate on a temp file.
# Usage:
#   ./benchmarks/code-exec-vs-mcp-bakeoff.sh
#   TOKENZERO_BIN=/path/to/tokenzero ./benchmarks/code-exec-vs-mcp-bakeoff.sh
#   ./benchmarks/code-exec-vs-mcp-bakeoff.sh > benchmarks/code-exec-vs-mcp-report.md

set -euo pipefail

# ---------- config ----------
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${TOKENZERO_BIN:-$(command -v tokenzero 2>/dev/null || true)}"
if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  BIN="${HOME}/.tokenzero/bin/tokenzero"
fi
if [[ ! -x "$BIN" ]]; then
  echo "ERROR: tokenzero binary not found. Set TOKENZERO_BIN=/path/to/tokenzero" >&2
  exit 1
fi

readonly CORPUS_FILE="${ROOT}/README.md"
readonly CARGO_FILE="${ROOT}/Cargo.toml"

EDIT_TMP="$(mktemp /tmp/tz-bakeoff-edit.XXXXXX)"
CAP_FILE="$(mktemp /tmp/tz-bakeoff-cap.XXXXXX)"
trap 'rm -f "$EDIT_TMP" "$CAP_FILE"' EXIT

# ---------- helpers ----------
log()  { printf '[bakeoff] %s\n' "$*" >&2; }

# Approximate tokenizer: ceil(UTF-8 bytes / 4). Honest proxy when no
# production tokenizer is linked into the benchmark harness.
tok() {
  python3 -c 'import sys; b=len(sys.stdin.buffer.read()); print((b+3)//4)'
}

now_ms() {
  python3 -c 'import time; print(int(time.time()*1000))'
}

emit_row() {
  # $1 task  $2 approach  $3 in_tok  $4 out_tok  $5 turns  $6 wall_ms  $7 quality
  printf '| `%s` | `%s` | %s | %s | %s | %s | %s |\n' "$1" "$2" "$3" "$4" "$5" "$6" "$7"
}

# Cache the capabilities JSON once and measure the schema-load wall time.
cap_start=$(now_ms)
"$BIN" capabilities --json > "$CAP_FILE" || { echo "ERROR: capabilities --json failed" >&2; exit 1; }
cap_end=$(now_ms)
MCP_WALL=$((cap_end - cap_start))

# MCP schema tokens for a comma-separated tool list. Extracts the named
# tool definitions from the cached capabilities JSON and counts bytes/4.
mcp_schema_tokens() {
  local tools="$1"
  python3 - "$CAP_FILE" "$tools" <<'PY'
import sys, json
cap_file, tools_csv = sys.argv[1], sys.argv[2]
tools = [t.strip() for t in tools_csv.split(",") if t.strip()]
cap = json.load(open(cap_file))
by_name = cap.get("commands_by_name", {})
out = {}
for t in tools:
    if t in by_name:
        out[t] = by_name[t]
    else:
        for c in cap.get("commands", []):
            if c.get("name") == t:
                out[t] = c
                break
js = json.dumps(out, separators=(",", ":"), ensure_ascii=False)
print((len(js.encode("utf-8")) + 3) // 4)
PY
}

# Quality check shared by CLI and CodeMode rows.
# Usage: quality_check <task> <json>
quality_check() {
  local task="$1" json="$2"
  python3 - "$task" <<<"$json" <<'PY'
import sys, json
task = sys.argv[1]
try:
    d = json.load(sys.stdin)
except Exception:
    print("FAIL"); raise SystemExit

def extract_text(d):
    if isinstance(d, str):
        return d
    val = d.get("value", {})
    if isinstance(val, dict) and "text" in val:
        return val["text"]
    vis = d.get("visible", {})
    if isinstance(vis, dict) and "text" in vis:
        return vis["text"]
    return json.dumps(d)

text = extract_text(d)
low = text.lower()

if task == "read_file":
    ok = "[workspace]" in text
elif task == "search_filter":
    ok = "TokenZero" in text and text.count("\n") >= 1
elif task == "edit_verify":
    ok = "BETA" in text and "beta" not in low
elif task == "multi_step_nav":
    ok = "workspace" in low
elif task == "shell_expand":
    ok = "Cargo.toml" in text
else:
    ok = False
print("PASS" if ok else "FAIL")
PY
}

# Run one CLI command and emit tab-separated metrics:
#   in_tok  out_tok  turns  wall_ms  quality
# Usage: cli_one <task> <cmd_str_for_counting> <actual args...>
cli_one() {
  local task="$1" cmd_str="$2"; shift 2
  local start end out status=0 in_tok out_tok wall q
  start=$(now_ms)
  out=$("$@") || status=$?
  end=$(now_ms)
  wall=$((end - start))
  in_tok=$(printf '%s' "$cmd_str" | tok)
  out_tok=$(printf '%s' "$out" | python3 -c \
    'import sys,json; d=json.load(sys.stdin); print(d.get("accounting",{}).get("raw_tokens", d.get("telemetry",{}).get("raw_tokens",0)))' \
    2>/dev/null || echo 0)
  if [[ $status -ne 0 ]]; then
    q="FAIL"
  else
    q=$(quality_check "$task" "$out")
  fi
  printf '%s\t%s\t%s\t%s\t%s\n' "$in_tok" "$out_tok" "1" "$wall" "$q"
}

# Accumulate multiple cli_one rows into one summed row.
sum_rows() {
  local in_t=0 out_t=0 turns=0 wall=0 q="PASS"
  local i o t w qu
  while IFS=$'\t' read -r i o t w qu; do
    in_t=$((in_t + i))
    out_t=$((out_t + o))
    turns=$((turns + t))
    wall=$((wall + w))
    [[ "$qu" == "PASS" ]] || q="FAIL"
  done
  printf '%s\t%s\t%s\t%s\t%s\n' "$in_t" "$out_t" "$turns" "$wall" "$q"
}

# Run a CodeMode plan and emit a markdown row.
run_codemode() {
  local task="$1" plan="$2"
  local start end out status=0 in_tok out_tok wall turns q
  start=$(now_ms)
  out=$("$BIN" codemode --json "$plan") || status=$?
  end=$(now_ms)
  wall=$((end - start))
  in_tok=$(printf '%s' "$plan" | tok)
  out_tok=$(printf '%s' "$out" | python3 -c \
    'import sys,json; d=json.load(sys.stdin); print(d.get("value",{}).get("raw_tokens", d.get("telemetry",{}).get("raw_tokens",0)))' \
    2>/dev/null || echo 0)
  turns=$(printf '%s' "$out" | python3 -c \
    'import sys,json; d=json.load(sys.stdin); print(d.get("telemetry",{}).get("logical_ops",1))' \
    2>/dev/null || echo 1)
  if [[ $status -ne 0 ]]; then
    q="FAIL"
  else
    q=$(quality_check "$task" "$out")
  fi
  emit_row "$task" "CodeMode" "$in_tok" "$out_tok" "$turns" "$wall" "$q"
}

setup_edit_file() {
  printf 'alpha\nbeta\ngamma\n' > "$EDIT_TMP"
}

# ---------- tasks ----------

task_read_file() {
  local task="read_file"
  local mcp_in; mcp_in=$(mcp_schema_tokens "read")
  emit_row "$task" "MCP-schema" "$mcp_in" "0" "1" "$MCP_WALL" "simulated"

  local row
  row=$(cli_one "$task" "$BIN read $CARGO_FILE --end-line 20 --json" \
        "$BIN" read "$CARGO_FILE" --end-line 20 --json)
  local in_tok out_tok turns wall q
  IFS=$'\t' read -r in_tok out_tok turns wall q <<<"$row"
  emit_row "$task" "CLI" "$in_tok" "$out_tok" "$turns" "$wall" "$q"

  local plan='const f = await zero.read("'"$CARGO_FILE"'", { end_line: 20 }); return f;'
  run_codemode "$task" "$plan"
}

task_search_filter() {
  local task="search_filter"
  local mcp_in; mcp_in=$(mcp_schema_tokens "find,grep")
  emit_row "$task" "MCP-schema" "$mcp_in" "0" "1" "$MCP_WALL" "simulated"

  local row
  row=$(cli_one "$task" "$BIN find TokenZero $CORPUS_FILE --json" \
        "$BIN" find "TokenZero" "$CORPUS_FILE" --json)
  local in_tok out_tok turns wall q
  IFS=$'\t' read -r in_tok out_tok turns wall q <<<"$row"
  emit_row "$task" "CLI" "$in_tok" "$out_tok" "$turns" "$wall" "$q"

  local plan='const h = await zero.find("TokenZero", "'"$CORPUS_FILE"'"); return h;'
  run_codemode "$task" "$plan"
}

task_edit_verify() {
  local task="edit_verify"
  local mcp_in; mcp_in=$(mcp_schema_tokens "edit,read")
  emit_row "$task" "MCP-schema" "$mcp_in" "0" "1" "$MCP_WALL" "simulated"

  setup_edit_file
  local row
  row=$(
    cli_one "$task" "$BIN edit --edits-json [{find:beta,replace:BETA}] --json $EDIT_TMP" \
          "$BIN" edit --edits-json '[{"find":"beta","replace":"BETA"}]' --json "$EDIT_TMP"
    cli_one "$task" "$BIN read $EDIT_TMP --json" \
          "$BIN" read "$EDIT_TMP" --json
  )
  local in_tok out_tok turns wall q
  IFS=$'\t' read -r in_tok out_tok turns wall q <<<"$(printf '%s' "$row" | sum_rows)"
  emit_row "$task" "CLI" "$in_tok" "$out_tok" "$turns" "$wall" "$q"

  setup_edit_file
  local plan='const e = await zero.edit("'"$EDIT_TMP"'", [{ find: "beta", replace: "BETA" }]); const f = await zero.read("'"$EDIT_TMP"'"); return f;'
  run_codemode "$task" "$plan"
}

task_multi_step_nav() {
  local task="multi_step_nav"
  local mcp_in; mcp_in=$(mcp_schema_tokens "tree,read,find")
  emit_row "$task" "MCP-schema" "$mcp_in" "0" "1" "$MCP_WALL" "simulated"

  local row
  row=$(
    cli_one "$task" "$BIN tree $ROOT --depth 2 --json" \
          "$BIN" tree "$ROOT" --depth 2 --json
    cli_one "$task" "$BIN read $CARGO_FILE --json" \
          "$BIN" read "$CARGO_FILE" --json
    cli_one "$task" "$BIN find workspace $CARGO_FILE --json" \
          "$BIN" find "workspace" "$CARGO_FILE" --json
  )
  local in_tok out_tok turns wall q
  IFS=$'\t' read -r in_tok out_tok turns wall q <<<"$(printf '%s' "$row" | sum_rows)"
  emit_row "$task" "CLI" "$in_tok" "$out_tok" "$turns" "$wall" "$q"

  local plan='const t = await zero.tree("'"$ROOT"'", { depth: 2 }); const f = await zero.read("'"$CARGO_FILE"'"); const hits = await zero.find("workspace", "'"$CARGO_FILE"'"); return hits;'
  run_codemode "$task" "$plan"
}

task_shell_expand() {
  local task="shell_expand"
  local mcp_in; mcp_in=$(mcp_schema_tokens "run,expand,read")
  emit_row "$task" "MCP-schema" "$mcp_in" "0" "1" "$MCP_WALL" "simulated"

  local row
  row=$(
    cli_one "$task" "$BIN run --json -- find . -maxdepth 1 -name Cargo.toml" \
          "$BIN" run --json -- find . -maxdepth 1 -name Cargo.toml
    cli_one "$task" "$BIN read $CARGO_FILE --end-line 1 --json" \
          "$BIN" read "$CARGO_FILE" --end-line 1 --json
  )
  local in_tok out_tok turns wall q
  IFS=$'\t' read -r in_tok out_tok turns wall q <<<"$(printf '%s' "$row" | sum_rows)"
  emit_row "$task" "CLI" "$in_tok" "$out_tok" "$turns" "$wall" "$q"

  local plan='const out = await zero.shell("find . -maxdepth 1 -name Cargo.toml"); const f = await zero.expand(out.ref); return f;'
  run_codemode "$task" "$plan"
}

# ---------- main ----------
log "binary: $BIN"
log "corpus: $CORPUS_FILE , $CARGO_FILE"

printf '# Code-Exec vs MCP-Schema Bake-off Results\n\n'
printf '> Bead: tokenzero-da8.1 — generated by `benchmarks/code-exec-vs-mcp-bakeoff.sh`\n'
printf '> Baseline commit: %s\n\n' "$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"

printf '| Task | Approach | input_tokens | output_tokens | turns | wall_ms | quality |\n'
printf '|---|---|---:|---:|---:|---:|---|\n'

task_read_file
task_search_filter
task_edit_verify
task_multi_step_nav
task_shell_expand

printf '\n_Legend: quality = PASS/FAIL for executed approaches; `simulated` = MCP schema load only (no task execution)._\n'
log "done."
