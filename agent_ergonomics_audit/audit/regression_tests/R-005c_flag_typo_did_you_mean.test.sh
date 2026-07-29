#!/bin/sh
# R-005c: distance-1 flag typos get did-you-mean + corrected command (R-002).
# Rust pin: cli_help_contract.rs::cli_flag_typo_distance_one_offers_corrected_command.
set -eu
BIN="${TOKENZERO_BIN:-tokenzero}"
err="$($BIN read --jsonn some/file.rs 2>&1 || true)"
printf '%s' "$err" | grep -q "did you mean: '--json'" || { echo "missing did-you-mean" >&2; exit 1; }
printf '%s' "$err" | grep -q "corrected command: tokenzero read --json some/file.rs" || { echo "missing corrected command" >&2; exit 1; }
err="$($BIN grep --exlpain needle 2>&1 || true)"
printf '%s' "$err" | grep -q 'did you mean' && { echo "far-off typo got a suggestion" >&2; exit 1; }
echo PASS R-005c
