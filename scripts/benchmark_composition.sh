#!/bin/sh
# CodeMode composition benchmark: measures one-plan execution against raw
# subprocess output and equivalent classic per-op tool calls. Produces JSON to stdout.
#
# Usage: scripts/benchmark_composition.sh
# Runs from the repo root.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

cargo test -p tokenzero-mcp --quiet -- codemode::bench::bench_harness::run_composition_benchmark --exact --nocapture 2>/dev/null \
  | sed -n '/^{/,/^}$/p' 
