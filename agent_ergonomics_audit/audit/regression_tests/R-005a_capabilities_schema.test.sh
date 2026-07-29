#!/bin/sh
# R-005a: capabilities --json pins the tokenzero.capabilities.v1 schema keys.
# Rust pin: cli_help_contract.rs::cli_capabilities_json_exposes_agent_contract.
set -eu
BIN="${TOKENZERO_BIN:-tokenzero}"
out="$($BIN capabilities --json)"
for key in schema_version tool version contract_version features feature_flags commands commands_by_name exit_codes env_vars; do
  printf '%s' "$out" | grep -q "\"$key\"" || { echo "missing key: $key" >&2; exit 1; }
done
printf '%s' "$out" | grep -q '"schema_version": "tokenzero.capabilities.v1"' || { echo "schema_version mismatch" >&2; exit 1; }
echo PASS R-005a
