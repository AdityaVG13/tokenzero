#!/usr/bin/env bash
set -euo pipefail

export PATH="/usr/local/cargo/bin:${PATH}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/linux-docker}"

cargo test --workspace
cargo run -p tokenzero-cli -- shell-matrix \
  --output-json results/current/rust_shell_matrix_linux_docker.json \
  --output-md results/current/rust_shell_matrix_linux_docker.md \
  --json
cargo run -p tokenzero-cli -- package-audit \
  --dist "${CARGO_TARGET_DIR}/debug" \
  --json > results/current/rust_package_audit_linux_docker.json
