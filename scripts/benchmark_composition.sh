#!/bin/sh
# CodeMode plan composition benchmark: measures one-plan execution vs equivalent
# sequences of direct calls. Produces JSON to stdout.
#
# Usage: scripts/benchmark_composition.sh
# Runs from the repo root. Requires a built tokenzero-mcp binary.
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo test -p tokenzero-mcp --quiet -- codemode::bench_harness::run_composition_benchmark --nocapture 2>/dev/null
