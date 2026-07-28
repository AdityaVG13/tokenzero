#!/usr/bin/env bash
# CodeMode CPU gate: a tokenzero-codemode server must sit at ~0% CPU both
# while a host op is pending and while idle after a plan completes.
# Regressions of the 1ms busy-poll class (tokenzero-osn1) violate this gate.
#
# Exit 0: all windows within budget. Exit 1: violation or harness failure.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT" || exit 1
CM_BIN="${TOKENZERO_CODEMODE_BIN:-$ROOT/target/release/tokenzero-codemode}"
if [[ ! -x "$CM_BIN" ]]; then
  echo "ERROR: codemode binary not found at $CM_BIN (set TOKENZERO_CODEMODE_BIN)" >&2
  exit 1
fi

IDLE_WINDOW_S="${CPU_GATE_IDLE_WINDOW_S:-10}"
IDLE_BUDGET_S="${CPU_GATE_IDLE_BUDGET_S:-0.2}"
PENDING_WINDOW_S=2
PENDING_BUDGET_S="${CPU_GATE_PENDING_BUDGET_S:-0.5}"

FIFO="$(mktemp -u /tmp/tz-cpugate.XXXXXX)"
mkfifo "$FIFO"
OUT="$FIFO.out"; ERR="$FIFO.err"
SV=""

cleanup() {
  [[ -n "$SV" ]] && kill "$SV" 2>/dev/null
  rm -f "$FIFO" "$OUT" "$ERR"
}
trap cleanup EXIT

cpu_seconds() {
  # ps TIME -> seconds; handles m:ss.cc, m:ss, h:mm:ss, d-h:mm:ss (macOS + Linux)
  ps -o time= -p "$1" 2>/dev/null | tr -d ' ' | sed 's/\.[0-9]*$//' | awk -F: '{ if (index($0, "-") > 0) { split($0, a, "-"); d=a[1]; rest=a[2] } else { d=0; rest=$0 }; n=split(rest, f, ":"); if (n==2) print d*86400 + f[1]*60 + f[2]; else if (n==3) print d*86400 + f[1]*3600 + f[2]*60 + f[3]; else print 0 }'
}

send() { printf '%s\n' "$1" >&3; }

start_server() {
  : > "$OUT"; : > "$ERR"
  "$CM_BIN" < "$FIFO" > "$OUT" 2> "$ERR" &
  SV=$!
  exec 3>"$FIFO"
  send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"cpu-gate","version":"0"}}}'
  send '{"jsonrpc":"2.0","method":"notifications/initialized"}'
}

call_plan() {
  python3 -c 'import json,sys; print(json.dumps({"jsonrpc":"2.0","id":int(sys.argv[1]),"method":"tools/call","params":{"name":"tz_execute_code","arguments":{"plan":sys.argv[2],"root":sys.argv[3]}}}))' "$1" "$2" "$ROOT" >&3
}

await_response() {
  local id="$1" i
  for i in $(seq 1 90); do
    grep -q "\"id\":$id" "$OUT" 2>/dev/null && return 0
    kill -0 "$SV" 2>/dev/null || return 1
    sleep 1
  done
  return 1
}

fail=0
echo "| window | duration | CPU consumed | budget | verdict |"
echo "|---|---:|---:|---:|---|"

start_server
call_plan 2 'const out = zero.shell("sleep 6"); return {done:true};'
sleep 2
t0=$(cpu_seconds "$SV"); sleep "$PENDING_WINDOW_S"; t1=$(cpu_seconds "$SV")
used=$(awk -v a="$t0" -v b="$t1" 'BEGIN{ printf "%.2f", ((d=b-a)<0?0:d) }')
verdict=PASS; awk -v u="$used" -v b="$PENDING_BUDGET_S" 'BEGIN{ exit !(u>b) }' && { verdict=FAIL; fail=1; }
echo "| host op pending (poll path) | ${PENDING_WINDOW_S}s | ${used}s | ${PENDING_BUDGET_S}s | $verdict |"
await_response 2 || { echo "_server died during pending-op plan_" >&2; exit 1; }

t0=$(cpu_seconds "$SV"); sleep "$IDLE_WINDOW_S"; t1=$(cpu_seconds "$SV")
used=$(awk -v a="$t0" -v b="$t1" 'BEGIN{ printf "%.2f", ((d=b-a)<0?0:d) }')
verdict=PASS; awk -v u="$used" -v b="$IDLE_BUDGET_S" 'BEGIN{ exit !(u>b) }' && { verdict=FAIL; fail=1; }
echo "| idle after completion | ${IDLE_WINDOW_S}s | ${used}s | ${IDLE_BUDGET_S}s | $verdict |"

t0=$(cpu_seconds "$SV"); w0=$(date +%s)
for i in $(seq 10 29); do
  call_plan "$i" 'const f = zero.read("Cargo.toml", {start_line: 1, end_line: 5}); return {ok:true};'
  await_response "$i" || { echo "_server died on plan $i_" >&2; exit 1; }
done
t1=$(cpu_seconds "$SV"); w1=$(date +%s)
cpu_ms=$(awk -v a="$t0" -v b="$t1" 'BEGIN{ printf "%.0f", (((d=b-a)<0?0:d))*50 }')
wall_ms=$(( (w1-w0)*50 ))
echo "| 20-plan read loop (per plan) | ${wall_ms}ms wall | ${cpu_ms}ms CPU | informational | measured |"

t0=$(cpu_seconds "$SV"); sleep 5; t1=$(cpu_seconds "$SV")
used=$(awk -v a="$t0" -v b="$t1" 'BEGIN{ printf "%.2f", ((d=b-a)<0?0:d) }')
verdict=PASS; awk -v u="$used" -v b="$IDLE_BUDGET_S" 'BEGIN{ exit !(u>b) }' && { verdict=FAIL; fail=1; }
echo "| idle after 20-plan loop | 5s | ${used}s | ${IDLE_BUDGET_S}s | $verdict |"

echo
if [[ "$fail" -eq 0 ]]; then
  echo "CPU gate: PASS (server idles at ~0% with and without pending host ops)"
else
  echo "CPU gate: FAIL (busy-poll regression class; see tokenzero-osn1)"
fi
exit "$fail"
