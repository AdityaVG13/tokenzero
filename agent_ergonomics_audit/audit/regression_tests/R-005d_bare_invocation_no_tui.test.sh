#!/bin/sh
# R-005d: bare tokenzero prints static help (no TUI, no ANSI, exit 0).
# Rust pin: cli_help_contract.rs::cli_bare_invocation_prints_useful_help.
set -eu
BIN="${TOKENZERO_BIN:-tokenzero}"
out="$($BIN </dev/null 2>/dev/null)"
printf '%s' "$out" | grep -q 'Agent surfaces' || { echo "bare help missing Agent surfaces" >&2; exit 1; }
if printf '%s' "$out" | grep -q "$(printf '\033')"; then
  echo "bare help emitted ANSI escapes" >&2
  exit 1
fi
echo PASS R-005d
