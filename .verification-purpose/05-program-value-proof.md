# Program-Value Proof — TokenZero Test Suite

## Fresh Evidence Commands Run

All commands were run from `/Users/aditya/AI/TokenZero` on 2026-07-05.

### 1. Workspace tests pass

Command: `cargo test --workspace --locked --no-fail-fast`

Result: **PASS** — all suites green.

Per-suite counts observed:
- `tokenzero` main unit tests: 44 passed
- `cli_adapter_approval`: 6 passed
- `cli_artifact_handoff`: 7 passed
- `cli_doctor`: 11 passed
- `cli_help_contract`: 10 passed
- `cli_install_clients`: 7 passed
- `cli_misc_audits`: 10 passed
- `cli_reach_os`: 6 passed
- `cli_release_claim_audits`: 10 passed
- `cli_run_shell`: 15 passed
- `cli_tools_io`: 10 passed
- `golden_outputs`: 6 passed
- `mcp_transport`: 3 passed
- `passthrough_conformance`: 24 passed
- `pulse_cli`: 6 passed
- `rust_verify_script`: 2 passed
- `windows_script_contract`: 1 passed
- `tokenzero_core` unit tests: 57 passed
- `shell_semantics` integration: 12 passed
- `tokenzero_filters` unit tests: 16 passed
- `tokenzero_install` unit tests: 141 passed
- `tokenzero_mcp` unit tests: 239 passed
- `jsonrpc_conformance`: 13 passed
- `tokenzero_pulse` unit tests: 29 passed
- `tokenzero_recovery` unit tests: 46 passed
- `tokenzero_runtime` unit tests: 38 passed

### 2. Clippy clean

Command: `cargo clippy --workspace --all-targets --locked -- -D warnings`

Result: **PASS** — no warnings.

### 3. Format check

Command: `cargo fmt --all -- --check`

Result: **NEEDS ATTENTION** — command printed formatting diffs for `crates/tokenzero-mcp/src/codemode/tests.rs`.
The exit code was reported as 0, but the diff indicates the file is not currently formatted.
This is pre-existing and not introduced by this audit.

### 4. Embedded test mod boundary

Command: `python3 scripts/check_embedded_tests.py`

Result: **FINDINGS** — 3 oversized inline `#[cfg(test)]` mod bodies:
- `crates/tokenzero/src/cli_args.rs:1036` — 52 lines (max 50)
- `crates/tokenzero/src/zerostack_store.rs:103` — 67 lines (max 50)
- `crates/tokenzero-mcp/src/fetch_guard.rs:296` — 95 lines (max 50)

These are existing structural issues, not blockers, but should be moved to sibling `tests.rs` modules.

### 5. Module boundaries

Command: `python3 scripts/check_module_boundaries.py`

Result: **PASS** — extracted modules present, facades under ceiling, no deep imports.

---

## Proof That Durable Tests Matter Downstream

The program-durable tests protect live, relied-upon behavior:

1. **Byte-exact recovery** (`tokenzero-recovery` proptest + restart tests) — downstream agents and users recover hidden context exactly. A regression here breaks the product's core promise.
2. **Shell safety** (`tokenzero-filters` destructive/compound/substitution tests, `tokenzero-runtime` quoting/routing tests, `tokenzero-core` shell semantics) — prevents TokenZero from vouching for destructive or injected commands. A regression could cause data loss or command injection.
3. **MCP protocol conformance** (`tokenzero-mcp` JSON-RPC matrix + initialize negotiation) — agents using TokenZero as an MCP server depend on valid JSON-RPC. Regressions break integration.
4. **Sandbox security** (`tokenzero-mcp` e2e sandbox denials) — CodeMode execution must not allow network, process, fs, or timer escapes. A regression is a security vulnerability.
5. **Install/package integrity** (`tokenzero-install` package audit tests) — release gates reject path traversal, polyglot archives, private metadata, and dev-launcher dependencies. Regressions ship unsafe packages.
6. **CLI contract** (`tokenzero` help/doctor/run/artifact/passthrough tests) — downstream scripts and agents rely on stable subcommands, JSON schemas, exit codes, and hook behavior.
7. **Pulse ledger integrity** (`tokenzero-pulse` import/export/schema/lock tests) — telemetry data used for recovery-adjusted savings must not corrupt, drift, or lose events.

---

## Why Session-Only Tests Are Safe to Prune

The identified session-only tests fall into these downstream-no-op categories:
- **Benchmark harness internals** (`bench_tests.rs`) — only used to validate a dev-time benchmark script, not a shipped contract.
- **Source-count audits** (`unsafe_escape_hatchet...`) — `#![deny(unsafe_code)]` already enforces the contract at compile time.
- **Windows-only happy paths on macOS CI** — cannot run in CI and assert platform-level behavior already covered by OS.
- **Trivial echo tests** (`env_i_style_invocation_works`, `run_command_caps_large_stdout_with_metadata`) — overlap with stronger unit/E2E tests.
- **Implementation-detail/formatting assertions** (`persisted_cache_is_compact_json`, `small_blob_stays_inline`) — pretty-printed or sidecar-cached data is equally correct downstream.
- **Advisory heuristics / docs string checks** — no failure class for shipped program.

Removing them does not reduce downstream protection; it reduces maintenance surface.
