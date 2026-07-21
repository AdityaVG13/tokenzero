SHELL := /bin/bash

.PHONY: test readme-command-audit rust-test rust-verify rust-verify-report rust-release-build rust-proof package-check release-check irx9-gate perf-regression-gate cli-smoke doctor mcp-smoke mcp-soak shell-matrix install-smoke package-audit

test: readme-command-audit rust-test

readme-command-audit:
	@python3 scripts/readme_command_audit.py
	@python3 scripts/readme_command_audit.py --self-check

rust-test:
	@cargo test --workspace

rust-verify:
	@scripts/rust_verify.sh

rust-verify-report:
	@scripts/rust_verify.sh --robot --output-json results/current/rust_verify.json

rust-release-build:
	@cargo build --release -p tokenzero

rust-proof: rust-verify rust-release-build mcp-smoke mcp-soak shell-matrix install-smoke package-audit

package-check: rust-release-build package-audit

# irx9-gate is mandatory: release-check cannot claim green without it.
# Note: rust-proof may run broad verify; irx9-gate is the named-package irx9 path.
release-check: irx9-gate rust-proof

# Focused irx9 parity/packaging/dispatcher/bench gates (no workspace-wide cargo).
irx9-gate:
	@scripts/irx9_release_gate.sh

# Matched baseline/candidate p50+p95 gate. BASELINE_BIN must name an already
# built comparison binary; the candidate defaults to this checkout's release build.
perf-regression-gate: rust-release-build
	@test -n "$(BASELINE_BIN)" || { echo "BASELINE_BIN is required" >&2; exit 2; }
	@python3 scripts/compare_binaries.py \
		--baseline "$(BASELINE_BIN)" \
		--candidate "$${CANDIDATE_BIN:-target/release/tokenzero}" \
		--fixture "$${PERF_FIXTURE:-README.md}" \
		--work-dir "$${PERF_WORK_DIR:-.}" \
		--trials "$${PERF_TRIALS:-500}" \
		--json-output "$${PERF_JSON:-results/current/matched-ab.json}"

cli-smoke:
	@target/debug/tokenzero read README.md --json >/dev/null
	@target/debug/tokenzero grep TokenZero README.md docs crates --json >/dev/null
	@target/debug/tokenzero glob 'crates/**/*.rs' . --json >/dev/null
	@target/debug/tokenzero run --json -- echo ok >/dev/null

doctor:
	@target/debug/tokenzero doctor --json

mcp-smoke:
	@target/debug/tokenzero mcp-smoke --output-md results/current/rust_mcp_smoke.md --output-json results/current/rust_mcp_smoke.json --json

mcp-soak:
	@target/debug/tokenzero mcp-soak --output-md results/current/rust_mcp_soak.md --output-json results/current/rust_mcp_soak.json --json

shell-matrix:
	@target/debug/tokenzero shell-matrix --output-md results/current/rust_shell_matrix_local.md --output-json results/current/rust_shell_matrix_local.json --json

install-smoke:
	@target/debug/tokenzero install-smoke --output-json results/current/rust_install_smoke.json --json

package-audit:
	@target/release/tokenzero package-audit --dist target/release --json
