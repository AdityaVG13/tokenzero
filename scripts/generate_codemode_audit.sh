#!/bin/sh
# Generate CodeMode claim audit artifact.
# Produces results/current/tokenzero_codemode_audit.json
#
# Usage: scripts/generate_codemode_audit.sh
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test -p tokenzero-mcp --quiet -- codemode::audit_tests::generate_audit_artifact --nocapture 2>/dev/null
