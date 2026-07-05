SHELL := /bin/bash

.PHONY: test readme-command-audit rust-test rust-verify rust-verify-report rust-release-build rust-proof package-check release-check cli-smoke doctor mcp-smoke mcp-soak shell-matrix install-smoke package-audit

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

release-check: rust-proof

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
