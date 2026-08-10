# TokenZero aggregate CodeMode bindings

TokenZero no longer embeds a CodeMode planner, JavaScript runtime, plan journal, or local `tokenzero codemode` command. Multi-step plan composition belongs to the ZeroStack aggregate host.

## Current architecture

| Layer | Owner |
|---|---|
| Plan parsing, scheduling, recipes, transaction policy, machine permits | ZeroStack `zero-codemode` / aggregate host |
| TokenZero dotted bindings and operation schemas | TokenZero operation ABI |
| Token-domain execution, refs, effects, output bounds, telemetry | `tokenzero-engine` |
| Process transport | Planner-free raw-worker v2 artifact `tokenzero-codemode` |
| Direct per-operation compatibility | Explicit classic `tokenzero-mcp` package |

The retained artifact name `tokenzero-codemode` is rollout compatibility only. Its package semantic is `raw-worker`; it accepts raw-worker lifecycle/capability commands and never executes plans.

## Compose through ZeroStack

The aggregate host discovers TokenZero bindings such as:

- `zero.read`
- `zero.find`
- `zero.grep`
- `zero.tree`
- `zero.edit`
- `zero.shell`
- `zero.token.compact`
- `zero.token.expand`

It composes those bindings with FSZero and GraphZero, then dispatches typed operations to each engine's raw worker. TokenZero remains authoritative for tokenizer identity, exact refs, root policy, effects, visible/output caps, and honest accounting.

## Batch choices

The aggregate binding metadata keeps distinct contracts:

- `zero.batch`: mixed independent operations with a combined result.
- `zero.token.compactMany`: homogeneous items, one recoverable result per item.
- `zero.token.expandMany`: homogeneous refs, preserving input order and status.
- Aggregate plan sequencing is a host responsibility, not a TokenZero batch alias.

## Refs and recovery

Only full-hash ZeroRef v1 blob refs are portable across compatible shared stores. Accepted schemes are `tz://`, `fz://`, and `gz://`; `#B` and `#L` preserve byte/line fragments. Session, execution, and unit refs are engine-local and stay owner-scoped. Missing, stale, dangling, incompatible, or corrupt refs fail typed. Correctness evidence does not imply zero-copy or any performance claim.

## Compatibility

Clients that require direct MCP calls use `tokenzero-mcp` in classic mode. That package preserves its published tools, aliases, resources, schemas, output behavior, and support policy in [mcp-compat.md](mcp-compat.md). It does not register an engine-local CodeMode surface.

See [gate-c-semantic-retirement.md](gate-c-semantic-retirement.md) for the exact staged deletion and preservation proof.
