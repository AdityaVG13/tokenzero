# Gate-C engine-local CodeMode retirement proof

Status: operator-approved staged removal. This record distinguishes preserved TokenZero domain semantics from deliberately retired engine-local planner state.

## Reachability proof

Before deleting the conditional files, the worker manifest was reduced to one bin target (`src/main.rs`) with only `tokenzero-engine` and `zero-abi` dependencies. It has no library target, `surface-codemode` feature, QuickJS, FastMCP, machine-permit, or planner dependency. The conditional modules were reachable only from the deleted `tokenzero-codemode/src/lib.rs` / `exec.rs` runtime and its tests, never from canonical raw-worker v2.

Final guard coverage is in `crates/tokenzero/tests/gate_c_retirement_contract.rs`. It asserts that only `main.rs` remains in the worker source directory, forbidden host dependencies and hooks are absent, aggregate bindings remain registered, protocol atoms remain tokenizer-verified, and the classic MCP compatibility feature remains explicit.

## Conditional semantic decisions

### `journal.rs`

**Retired:** engine-local plan transactions, idempotency files, rollback journals, and manual-intervention state. These records existed only around the deleted local plan executor. Raw-worker v2 executes one typed operation per request and does not claim plan-level atomicity or rollback. Aggregate plan transaction and durability-promotion ownership is in ZeroStack.

**Preserved:** TokenZero operation mutability/effect/approval classification remains in the operation ABI and raw-worker result frames. Workspace mutations still fail closed through the typed dispatcher. The Gate-C conformance test checks required aggregate bindings and mutation metadata, while packaged raw-worker conformance checks effect metadata and no false success.

### `store.rs`

**Retired:** the local CodeMode execution-record format (`code`, `steps`, `result`, `error`, `telemetry`) and its plan-finalization persistence. It was written only by the deleted local executor.

**Preserved:** TokenZero recovery blobs, exact refs, output caps, structured values, causal accounting, ref ownership, effect metadata, and telemetry remain in `tokenzero-engine` and raw-worker v2 envelopes. Packaged raw-worker conformance covers oversized-result rejection, exact refs, ownership, effects, and accounting.

### `recipe_registry.rs` and `fixtures/codemode-recipes.json`

**Retired:** ten engine-local JS recipe bodies and their local measured-envelope claims. They were consumed only by the deleted local executor/sentinel runtime and are not presented as active evidence after Gate C.

**Preserved:** TokenZero operation metadata and dotted aggregate bindings remain the model-facing discovery contract. Aggregate recipe composition and execution belong to ZeroStack. No unreferenced recipe fixture remains in TokenZero.

### `sentinel.rs`

**Retired:** CodeMode takeover interception, its three recipe mappings, single-use reservation policy, and the local recipe executor. These were session-control/runtime policy and were never compiled into the canonical worker.

**Preserved:** Tokenizer identity, the portable one-token alphabet, tokenizer verification, ACK classes, and ACK rendering remain in `tokenzero-core::protocol_atoms` and `tokenzero-core::ack`. Aggregate hosts may define takeover/reservation policy without moving tokenizer truth out of TokenZero.

## Additional ownership correction

TokenZero no longer depends on `zero-gate`. The only use was a dev-only pin test touching `NextBudget`; no production domain path used it. TokenZero retains classified `zero-ledger` charge emission. Native durability promotion and gate policy remain exclusively in the hub's `zero-gate`.

## Required final evidence

- `cargo tree -p tokenzero-worker --no-default-features -e normal --depth 1`
- `cargo tree -p tokenzero-engine -e normal --depth 2`
- Gate-C retirement contract test
- classic MCP projection parity
- packaged raw-worker v2 conformance
- packaging lifecycle
- four-repository strict surface/substrate guard
- unchanged `fuzz/Cargo.lock` SHA-256: `ae9a0a8cab41b0c6e097298465279ccde3bf6c3563c3137a3c572abd6ff550fa`
