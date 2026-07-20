# tokenzero-irx9.2 review correction — static evidence (tests unrun)

## Mandate

Coordinator forbade running `cargo`, `rustc`, builds, tests, or compiler commands
for this dispatch (`task_ebc4d1323b47`). Verification is **static structure only**.

## Architecture change

| Before (ffb64b9) | After |
|---|---|
| `dispatcher` + `engine_*` inside `tokenzero-mcp` | New crate **`tokenzero-engine`** |
| Domain kernel shared tools.rs + JsonRpcErrorData | `domain.rs` uses `DomainDispatchError` only |
| `is_domain_operation` hard-coded name mask | `operation_is_domain` from registry `MigrationStatus` + resource_uri |
| CodeMode `exec_rewrite` bypassed dispatcher | Routes through `dispatch_codemode_method` |
| Dep tests scanned selected files | Engine tests walk **all** `src/**/*.rs` + **Cargo.toml** forbids |

## Dependency direction

`tokenzero-engine/Cargo.toml` depends on: core, recovery, filters, pulse, runtime.
It does **not** depend on: `fastmcp-rust`, `rquickjs`, `tokenzero-mcp`.

`tokenzero-mcp` depends on `tokenzero-engine` and re-exports it; transport modules
(`fastmcp_mode`, `jsonrpc`, `stdio`, `codemode`, `tools`) call
`tokenzero_engine::dispatch_operation` for domain ops.

## Registry domain classification

Domain op ⇔ `MigrationStatus::Canonical | LegacyAlias` and `resource_uri.is_none()`.
Adapter-owned ⇔ `CodemodeControl` or `Resource`.

## Tests authored (not executed this dispatch)

`crates/tokenzero-engine/tests/dispatcher.rs`:

- `engine_crate_does_not_depend_on_surface_layers`
- `no_fastmcp_codemode_cross_adapter_calls`
- `one_operation_same_dispatcher_from_all_adapters`
- `differential_registry_domain_ops_raw_mcp_cli`
- `differential_policy_failure_agrees_across_surfaces`
- `registry_domain_ops_are_metadata_driven_not_masked`
- `every_registry_domain_op_is_kernel_dispatchable`
- `dispatcher_records_profile_for_benchmark_subtraction`
- transport control tools rejected

Coordinator should run when CPU free:

```
cargo test -p tokenzero-engine --test dispatcher
```

## Residual

- Full CLI tool matrix still mostly calls engine methods directly; `dispatch_cli` is the
  thin adapter API (`mem` wired). Follow-up: route remaining CLI handlers through
  `dispatch_cli` without behavior change.
- MCP `tools.rs` still owns transport framing (execute_code envelopes); domain kernel
  is engine-only.
- Bead left open.
