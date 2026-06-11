# Development

Build, test, and verify the TokenZero Rust Core from source. Most users should
prefer a prebuilt binary from the [latest Release](https://github.com/AdityaVG13/tokenzero/releases);
this page is for contributors and from-source builds.

## Build

```bash
cargo build --release -p tokenzero

target/release/tokenzero doctor --json
target/release/tokenzero read README.md --json
target/release/tokenzero find "TokenZero" docs --json
target/release/tokenzero tree . --depth 2 --json
target/release/tokenzero run -- cargo test --workspace
target/release/tokenzero expand tz://blob/<id> --selector raw --force
```

## Verify

The debug binary is fine for the development loop:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
target/debug/tokenzero mcp-smoke --json
```

## Workspace

Eight Rust crates:

| Crate | Responsibility |
| --- | --- |
| `tokenzero-core` | Compression model and content-addressed exact-recovery refs |
| `tokenzero-recovery` | Bounded recovery cache with exact byte-recovery for refs |
| `tokenzero-runtime` | Runtime and session orchestration for the context layer |
| `tokenzero-filters` | Content filters and selectors for compression |
| `tokenzero-mcp` | MCP server exposing read/find/tree/expand/shell tools |
| `tokenzero` | The `tokenzero` binary |
| `tokenzero-install` | Installer and agent-wiring (Claude/Codex/Grok/etc.) |
| `tokenzero-pulse` | Pulse telemetry and forecasting |

## Verification artifacts

Proof artifacts are written under ignored `results/current/` paths:

| Artifact | Proves |
| --- | --- |
| `rust_cli_verification.json` | CLI read/expand byte-exact check |
| `rust_mcp_smoke.json` | MCP tool and alias smoke |
| `rust_mcp_soak.json` | Accelerated malformed/restart durability soak |
| `rust_shell_matrix_local.json` | Local shell/runtime matrix |
| `rust_perf_budget.json` | Release binary latency budget |
| `rust_install_smoke.json` | Isolated install/rollback smoke |
| `rust_package_audit.json` | Release-only Rust package audit |

## Release boundaries

Pre-launch: do not upload packages, mutate global config, publish remotes,
rewrite history, or perform a public release without explicit approval. See
[`../SECURITY.md`](../SECURITY.md) and [`../CONTRIBUTING.md`](../CONTRIBUTING.md).
