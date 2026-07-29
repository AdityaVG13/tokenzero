#!/bin/sh
# R-005b: --json stdout is data-only and stderr stays empty on success for
# read/find/doctor.
# Rust pin: cli_help_contract.rs::cli_agent_contract_outputs_are_deterministic_and_env_clean.
set -eu
BIN="${TOKENZERO_BIN:-tokenzero}"
TMPD="$(mktemp -d)"
trap 'rm -rf "$TMPD"' EXIT
probe() {
  name="$1"; shift
  "$@" >"$TMPD/out" 2>"$TMPD/err" || { echo "$name: non-zero exit" >&2; exit 1; }
  [ -s "$TMPD/out" ] || { echo "$name: empty stdout" >&2; exit 1; }
  [ ! -s "$TMPD/err" ] || { echo "$name: stderr not empty" >&2; cat "$TMPD/err" >&2; exit 1; }
}
probe read "$BIN" read --json README.md
probe find "$BIN" find --json TokenZero .
probe doctor "$BIN" doctor --json
echo PASS R-005b
