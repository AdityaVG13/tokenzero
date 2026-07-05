# Intent Statement

## Target
TokenZero repository at `/Users/aditya/AI/TokenZero`.

## Scope
All existing Rust verification artifacts across the workspace crates:
- tokenzero-core
- tokenzero-filters
- tokenzero-runtime
- tokenzero-recovery
- tokenzero-pulse
- tokenzero-install
- tokenzero-mcp
- tokenzero (CLI)

## Mode
`audit-existing` with a `full-gate-pass` report: classify every candidate test module/function as session-only vs. program-durable, apply the LOC/minimalism gate, and produce a minimal plan plus steering for future test work.

## Focus
Default to program-durable. Session scaffolding is allowed in the AI dev loop but must not ship as program code unless it carries proven downstream value. We will identify tests that are:
- happy-path-only,
- implementation-detail mirrors,
- over-mocked / mock-echo,
- overlapping with stronger program-durable tests,
- or otherwise add no new failure class.

## Behavior Contract
TokenZero is a local-first Rust runtime for AI context compression. Downstream consumers rely on:
1. Byte-exact recovery (blob/journal lifecycle).
2. Correct shell command planning and execution across macOS/Linux/Windows.
3. MCP protocol conformance (JSON-RPC, tool schemas, server lifecycle).
4. CLI contract (subcommands, flags, exit codes, artifact handoff, doctor/install/package-audit gates).
5. Cross-platform install and package integrity.

These are the public, live, paid/relied-upon surfaces; tests covering their failure modes are program-durable.

## Existing Verification Commands (from AGENTS.md / CI)
- `cargo test --workspace --locked --no-fail-fast`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo fmt --all -- --check`
- `cargo run --locked -p tokenzero -- shell-matrix --output-json ...`
- `python3 scripts/check_embedded_tests.py`
- `python3 scripts/check_module_boundaries.py`
- `cargo deny check`
- `tokenzero doctor --root . --runtime --json`
- `tokenzero package-audit --dist target/release --json`
- `tokenzero mcp-smoke --output-json ... --json`
- `tokenzero install-smoke --output-json ... --json`

## Up-Front Confirmations
- Target root resolved to `/Users/aditya/AI/TokenZero`.
- Scope = whole Rust test suite.
- Focus = program-durable default.
- No existing `.verification-purpose/` artifacts; this run creates them.
