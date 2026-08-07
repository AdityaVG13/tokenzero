# TokenZero CodeMode

CodeMode runs a bounded JavaScript or JSON plan against TokenZero's typed tool
surface. One plan can call several operations, pass refs between them, filter or
aggregate intermediate results, and return one compact result. Use it when work
is multi-step and model-visible round trips would add latency or repeat bytes.

## CodeMode or MCP?

| Choose | When |
|---|---|
| CodeMode | Dependent operations, parallel fan-out, in-plan filtering, or background command observation. |
| MCP compatibility | A client only supports direct MCP tool calls, or the task is one independent operation. |
| ZeroStack aggregate CodeMode | A plan must compose FSZero, GraphZero, and TokenZero. The aggregate host lives in the ZeroStack hub. |

Both TokenZero surfaces use the same engine, recovery store, policy checks, and
RACC accounting. CodeMode changes composition, not correctness or authorization.

## Install

Surface selection is explicit and fail-closed:

~~~bash
./packaging/install.sh --surface codemode
# The tokenzero shim now points at the selected single-surface binary.
tokenzero codemode 'describe:zero.token.compact'
~~~

Use './packaging/install.sh --surface mcp' for direct MCP compatibility or for
a native ZeroStack aggregate that owns CodeMode composition. Installing one
surface replaces the prior registration. See [install.md](install.md) for
prefixes, package builds, rollback, and client configuration.

## Run a plan

JavaScript plans expose a frozen 'zero' object:

~~~bash
tokenzero codemode --json --root . --plan '
  const hits = await zero.grep("TODO", "crates");
  const stored = await zero.token.compact(hits.text);
  return { count: hits.count, ref: stored.ref };
'
~~~

JSON recipe plans remain available for clients that do not author JavaScript.
Use 'tokenzero codemode search:<term>' and 'describe:<method>' to discover the
live typed catalog.

## Batch choices

The batch families are distinct contracts:

- `zero.batch` runs up to 16 independent, potentially mixed operations and
  returns one combined capsule with per-operation sections and unioned refs.
  Its ABI is conservatively workspace-mutating because a member may mutate.
- `zero.token.compactMany` stores a homogeneous `items` array. Use it when
  every item needs its own recovery ref.
- `zero.token.expandMany` expands a homogeneous `items` array of refs. Use it
  when every result must preserve input order and per-ref status.
- `zero.pipe` is not a batch alias. It runs ordered dependent steps and passes
  `_prev` into the next step.

Do not interchange these names or schemas. Compatibility aliases resolve to
these same canonical operations; they do not create a second batch contract.

## Background commands

Launch a command with '{ background: true }', then observe it with a cursor. A
bare job call long-polls server-side for up to 30 seconds and returns on log
growth, terminal state, or timeout. 'waitMs: 0' is the nonblocking form.
Responses contain only bytes after 'since'; preserve the returned cursor.

~~~javascript
const started = await zero.token.shell("cargo test -p my-crate", {
  background: true,
  timeout_ms: 120000
});
const update = await zero.token.job(started.job, {
  waitMs: 30000,
  since: started.cursor,
  tailBytes: 8192
});
return update;
~~~

Changed responses carry 'tail', byte 'cursor', monotonic 'version', and terminal
'exitCode' when available. Unchanged responses are intentionally tiny and carry
'nextPollMs'; wait that long before another model-visible observation. Legacy
'wait_ms', 'cursor', and 'tail_bytes' spellings remain accepted.

## Refs and recovery

Only full-hash ZeroRef v1 blob refs are portable across TokenZero, FSZero, and
GraphZero under a compatible shared CAS. Accepted schemes are 'tz://', 'fz://',
and 'gz://'; '#B' and '#L' preserve byte/line fragments. Execution, error,
session, file, graph, index, and unit refs are engine-local. Missing,
incompatible, stale, dangling, and corrupt refs fail typed; there is no fallback
to guessed bytes. Correctness evidence does not imply zero-copy, latency, or
performance claims.

## Bounds and safety

CodeMode enforces code, wall-time, memory, microtask, operation, parallel-width,
output, ref, and visible-token bounds. Shell commands run at the plan root unless
'cwd' is explicit. Host capabilities are unavailable to authored JavaScript;
all effects cross registered TokenZero methods and policy gates. Large results
stay behind exact expandable refs instead of being silently truncated.
