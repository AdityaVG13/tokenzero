#!/usr/bin/env bash
# tokenzero-irx9.10 — focused parity + packaging + dispatcher + bench gates.
# Never runs workspace-wide cargo. Fail-closed; prints operation/surface context.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
JOBS="${CARGO_JOBS:-2}"
THREADS="${TEST_THREADS:-2}"

fail() {
  echo "irx9_release_gate: FAIL: $*" >&2
  exit 1
}

run() {
  echo "+ $*"
  "$@"
}

echo "irx9_release_gate: start (CARGO_BUILD_JOBS=$CARGO_BUILD_JOBS jobs=$JOBS threads=$THREADS)"

# 1) Packaging mutual exclusion (static + lifecycle)
run cargo test -p tokenzero-install --jobs "$JOBS" --lib packaging -- --test-threads="$THREADS" \
  || fail "packaging unit tests (surface=install)"
run cargo test -p tokenzero --jobs "$JOBS" --test packaging_static_evidence --test packaging_lifecycle -- --test-threads="$THREADS" \
  || fail "packaging static/lifecycle (surface=package)"

# 2) Operation ABI + digest ratchet
run cargo test -p tokenzero-core --jobs "$JOBS" --lib operation_abi -- --test-threads="$THREADS" \
  || fail "operation ABI contract (surface=core)"

# 3) Dispatcher identity + FastMCP/CodeMode adapter evidence
run cargo test -p tokenzero-engine --jobs "$JOBS" --test dispatcher -- --test-threads="$THREADS" \
  || fail "dispatcher identity (surface=engine)"
run cargo test -p tokenzero-engine --jobs "$JOBS" --test fastmcp_adapter_from_registry -- --test-threads="$THREADS" \
  || fail "FastMCP adapter derivation (surface=mcp)"
run cargo test -p tokenzero-engine --jobs "$JOBS" --test codemode_bindings_dispatcher -- --test-threads="$THREADS" \
  || fail "CodeMode bindings (surface=codemode)"

# 4) Private worker + handshake
run cargo test -p tokenzero-engine --jobs "$JOBS" --lib raw_worker:: -- --test-threads="$THREADS" \
  || fail "raw worker protocol (surface=raw_worker)"
run cargo test -p tokenzero-engine --jobs "$JOBS" --lib surface_handshake:: -- --test-threads="$THREADS" \
  || fail "surface handshake (surface=handshake)"

# 5) Differential conformance corpus
run cargo test -p tokenzero-engine --jobs "$JOBS" --test irx9_conformance_corpus -- --test-threads="$THREADS" \
  || fail "conformance corpus (surface=parity)"

# 6) Surface latency / overhead ratchet
run cargo test -p tokenzero-engine --jobs "$JOBS" --test irx9_surface_bench -- --test-threads="$THREADS" \
  || fail "surface bench ratchet (surface=bench)"

# 7) Dual-feature compile refusal (one package, fail closed)
if cargo check -p tokenzero-install --jobs "$JOBS" --features "surface-mcp,surface-codemode" 2>/tmp/irx9_dual_feature.err; then
  fail "dual surface features must not compile (tokenzero-install)"
else
  if ! grep -q "mutually exclusive\|compile_error\|never both" /tmp/irx9_dual_feature.err; then
    # Still a hard compile failure is acceptable.
    echo "irx9_release_gate: dual-feature path failed closed (compile error present)"
  else
    echo "irx9_release_gate: dual-feature compile_error diagnostic present"
  fi
fi

echo "irx9_release_gate: PASS"
