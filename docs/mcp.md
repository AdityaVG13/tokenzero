# TokenZero MCP

MCP is the structured-tool adapter for the Rust TokenZero Core runtime. The CLI
remains the universal fallback for hosts without MCP.

## Protocol Compatibility

TokenZero supports the legacy initialization flow for existing clients, the
2025 stable stdio framing used by SDK-based clients, and the MCP 2026-07-28
release-candidate request shape for modern clients.

- Legacy clients may continue to call `initialize`, `notifications/initialized`, `tools/list`, and `tools/call` without per-request `_meta`. `notifications/initialized` notifications produce no response; ID-bearing legacy requests are tolerated with an empty result.
- Modern clients should call `server/discover` with `params._meta`, then use per-request `_meta` on `tools/list` and `tools/call`.
- `server/discover` returns the draft discovery fields `supportedVersions`, `capabilities`, `serverInfo`, `resultType`, `ttlMs`, and `cacheScope`. The older `protocolVersions` field remains as an additive compatibility alias and matches `supportedVersions`.
- Supported wire versions include `2024-11-05`, `2025-03-26`, `2025-06-18`, `DRAFT-2026-v1`, and `2026-07-28`.
- `initialize` responses include `_meta.tokenzero/protocolNegotiation` with the requested version, negotiated version, supported versions, and whether fallback negotiation occurred.
- Modern `tools/list` responses include `resultType: "complete"`, deterministic tool ordering, `ttlMs`, and `cacheScope`.
- Modern `tools/call` responses include `resultType: "complete"` and preserve the same text content legacy clients receive. Structured content is additive.
- Stdio transport accepts both newline-delimited JSON-RPC and `Content-Length`
  framed MCP messages; responses mirror the detected input framing.

The 2026-07-28 release candidate was posted on May 21, 2026. The final MCP specification is planned for July 28, 2026. TokenZero docs intentionally claim RC compatibility, not final MCP certification. Revalidate against the final specification before public final-support claims.

Deferred scope: MCP Apps, Tasks execution, OAuth server work, deprecated Roots/Sampling/Logging expansion, and public remote HTTP hosting are intentionally out of scope for this compatibility pass.

## Start The Server

```bash
tokenzero mcp-server --allowed-root /path/to/project
```

The MCP server is a foreground Rust stdio tool bridge launched by the client. It
does not start watchers, indexers, background services, or helper runtimes.
Clients should reuse one launched stdio process while the session is connected.

### Multi-project store isolation

Recovery cache paths default **per allowed root** (see docs/core.md). A process
env `ZEROSTACK_STORE_ROOT` does not collate unrelated projects unless
`TOKENZERO_SHARED_STORE=1` or `ZEROSTACK_SHARED_STORE=1` is set. Prefer
`--cache-path` / `TOKENZERO_CACHE_PATH` for explicit stores. Inspect
`resource://tokenzero/cache` for the active path and isolation note.

### Portable binary discovery (wqw.3)

Prefer `command: "tokenzero"` on PATH in MCP client configs (see
`docs/mcp-tokenzero.portable.json`). Optional env overrides:
`TOKENZERO_BIN`, `TOKENZERO_RG_PATH`, `TOKENZERO_CURL_PATH`. Discovery order:
env → PATH → well-known layouts → clear error. Never hardcode personal
`/Users/.../AI/*/target/release` paths. `tokenzero doctor --json` reports
`engine_binaries`.

### Connection hardening

The server is built to stay connected for the full life of an agent session:

- **No idle exit by default.** Agent sessions can sit idle for hours between
  tool calls; the server exits only when the client closes stdin. Idle exit is
  an explicit opt-in via `TOKENZERO_MCP_IDLE_TIMEOUT_SECS=<seconds>` or
  `tokenzero mcp-server --idle-timeout-seconds <seconds>` (0 keeps it
  disabled).
- **Malformed input never kills the session.** Invalid JSON or oversized
  unframed lines are answered with JSON-RPC `-32700` and the stream resyncs at
  the next line boundary.
- **Panic isolation.** A panicking tool call is answered with a retryable
  `-32603` Internal error; the server keeps serving.
- **Liveness under load.** `tools/call` requests run on a worker pool, so
  `ping`, `initialize`, and list requests are answered immediately even while
  long shell commands are running.
- **Crash-transparent supervision.** `tokenzero mcp-server --supervise`
  (the default in generated client configs) keeps a tiny supervisor on the
  client-facing pipes. If the inner server ever dies, the supervisor respawns
  it with backoff, replays the `initialize` handshake, answers in-flight
  requests with a retryable error, and the client never sees a disconnect.

Generated client configs use strict workspace roots plus read-only agent-context roots for skill/instruction files.

### Token-adaptive rendering

Capsules carry an adaptive floor: when framing (labels, telemetry headers)
would cost more tokens than the raw payload it wraps, the renderer falls back
to the raw text, so a TokenZero call never costs more context than the
underlying output. Small successful shell commands shrink to a `# shell ok`
header plus the `combined_ref` recovery anchor; failures, timeouts, and
summarized output keep the full diagnostic header.

Search, grep, glob, and tree responses store the flat grep-/path-compatible
output as the canonical recoverable payload, then render a grouped projection
(`# root:` headers, matches grouped per file, indented tree names) whenever it
is strictly cheaper than the flat form. Expanding the blob/file refs always
recovers the exact flat output. Zero-hit results render a one-line note
(`# find|grep <query> — 0 matches`, `# glob <pattern> — 0 matches`,
`# tree — 0 entries`) instead of a bare `refs:` footer. Empty payloads get
the same treatment: `tz_read` of an empty file or an empty requested line
range renders `# read <path> — 0 bytes`, and `tz_ingest` of empty text
renders `# ingest — 0 bytes`, while the stored payload stays the exact
(empty) bytes. The echoed query or path is clamped to one short line, the
note appends ` (scan truncated)` when result/visit/depth limits cut the scan
short, and passthrough mode — plus `tz_read` with `raw=true`, which keeps
its verbatim-slice contract — never gets a note.

`tz_edit` applies an all-or-nothing batch of find/replace hunks to one file
(atomic temp-file + rename write): a hunk that matches zero or several times
aborts the whole batch with `hunk_not_found`/`ambiguous_hunk` and the file is
untouched. The visible capsule is a hunk-labelled diff (context-1
before/after rendering, not a strict unified diff) under a
`# edit <path> — N hunks applied (+A -D lines)` header; `dry_run=true`
renders the same preview without writing. The refs footer lists the
post-image blob/file refs verbatim and summarizes the pre-image as `+1:undo`;
expanding the `undo` ref recovers the exact pre-edit bytes for rollback.
`create=true` takes exactly one hunk with an empty `find` whose `replace`
becomes the full new-file content (it fails if the file exists).

`tools/call` results are text-only by default: the visible capsule plus a
one-line `refs:` recovery footer (shell output carries its refs and
`command_success` inline instead). A `structuredContent` envelope diverging
from the text block makes several MCP clients render the JSON instead of the
tool text and roughly doubles per-call context cost, so it is opt-in: set
`TOKENZERO_MCP_ENVELOPE=compact` for the pruned `structuredContent.cli`
envelope (payload duplicates and forensic telemetry removed) or
`TOKENZERO_MCP_ENVELOPE=full` for the complete CLI envelope.

### Session redundancy layer

The server keeps an in-memory seen-set of payloads already served this
session: file reads keyed per canonicalized path and requested line range,
find/grep outputs keyed per tool, query, and root set. The content hash of
the exact served payload is the only invalidation source — touching a file
without changing its bytes still dedups, and a changed hash always
invalidates. The map lives in process memory and dies with the session, by
design: a sidecar surviving restart would claim "served earlier this
session" to a fresh session whose context never contained the bytes, and two
agents sharing one repo would cross-contaminate. Supervisor respawn loses
the map and degrades to full serves — never wrong, only un-optimized. A
poisoned session lock fails open the same way.

A re-read or re-search whose payload is byte-identical to one already served
collapses to a two-line note instead of the full render:

```
unchanged: tz://file/… (served earlier this session)
# src/lib.rs — 240 lines, 1830 tokens; full bytes: expand tz://blob/…
```

A re-read whose content changed serves a unified diff against the previously
served bytes (recovered through the normal `tz_expand` path and charged as
recovery tokens) under a
`# read <path> — changed since served this session (diff vs <old ref>)`
header with a `full file: expand <new ref>` tail. Diffing is skipped above
2 MiB or 50k lines per side, when the previously served base was pruned from
the recovery cache, and for search output (changed results serve full).
Range-keyed reads only diff against the same requested range.

Both serves obey the adaptive floor: the note or diff is emitted only when
strictly cheaper in tokens than the full render, so the layer never costs
more than serving full. Refs are freshly minted on every serve and the note
embeds its own expand instruction, so even a client that compacted the
earlier payload out of its context recovers the exact bytes in one
`tz_expand` (stale refs return typed errors with a rerun hint). Notes are
substituted only after this call's refs actually persisted: under a degraded
recovery cache (`cache_write_failed`) the server always serves full content
— a note must never advertise unrecoverable refs — and the serve is not
recorded as a dedup base. Empty payloads and zero-hit notes never dedup, and
`tz_expand` is never deduped — it is the recovery path and always returns
bytes.

Opt-outs: `TOKENZERO_MCP_DEDUP=0|off` disables the layer,
`TOKENZERO_MCP_DIFF_READS=0|off` disables only diff-aware re-reads (both
default on, read once at engine construction), per-call `fresh: true` on
`tz_read`/`tz_find`/`tz_grep` forces a full serve, and `raw=true` reads plus
passthrough mode keep their verbatim contracts. Bypassed serves are still
recorded, so a later normal call can dedup against them.

Accounting flows through the existing fields: `raw_tokens` stays the stored
payload's size, `visible_tokens` becomes the note/diff cost, and the diff
base expansion is charged to `recovery_tokens`. Telemetry reports
`output_strategy` `seen_set_dedup` or `diff_since_served`, `cache_hit`, and
a `dedup`/`diff` detail object, merged alongside existing markers (degraded
storage, search backend). `tz_mem` reports a session rollup under
`session_dedup`: `{records, dedup_hits, diff_hits, visible_tokens_saved,
diff_tokens_saved}`.

## Tools

### Write recovery ladder (wqw.12)

When CodeMode `zero.edit` / `tz_edit` fails, the error includes a **write recovery
ladder**: prefer CodeMode edit under allowed roots → check roots/doctor → if the
write substrate is down (or `TOKENZERO_WRITE_ESCAPE=1`), harnesses may use a
**bounded native Write for that failure only** → record via `tz_report_tool_issue`.
Native Write is not the default while CodeMode works.

### Field issue reports (wqw.6)

`tz_report_tool_issue` / `report_tool_issue` accepts **`zero_execute`** (and aliases:
`zerostack`, `tz_execute_code`, `zero.token.*`, `zero.fs.*`, `tz_*`) so expand/root/shell
failures can be recorded without leaving the harness. Reports land under
`.tokenzero/tool-issues/` next to the recovery cache.


| Tool | Purpose |
| --- | --- |
| `tz_read` | compact local file reads with exact refs |
| `tz_find` | compact literal-substring search results with recoverable hits |
| `tz_grep` | grep-compatible exact-first search results (regex under the ripgrep backend) |
| `tz_glob` | glob-compatible exact-first path discovery |
| `tz_tree` | bounded repo tree inspection |
| `tz_edit` | one-call multi-hunk file edit with undo ref |
| `tz_recall` | full-text search over payloads already in the recovery cache |
| `tz_batch` | several TokenZero ops in one call with a combined capsule |
| `tz_fetch` | TTL-cached http(s) fetch via curl with exact refs |
| `tz_shell` | compressed shell/test/log output |
| `tz_ingest` | external payload ingest with exact refs |
| `tz_expand` | exact recovery from `tz://`, `fz://`, or `gz://` blob refs (same-store scheme alias; cross-engine blob expand under shared CAS is release-gated on retained multi-OS evidence; non-blob portable refs unsupported) |

CodeMode `zero.token.expand(ref)` bounds large default results to 1,200 visible tokens and keeps the ref recoverable. Prefer `{start_line, end_line}` or `{symbol}` for exact windows. Use `{raw: true}` only when the complete byte-exact payload is required. The classic `tz_expand`/CLI recovery surfaces remain exact.
| `tz_mem` | recovery/cache/config state |
| `tz_cache_pack` | daemonless prompt-cache pack generation |
| `tz_rewrite` | conservative command rewrite planning |
| `tz_discover` | filter and runtime readiness discovery |

Short aliases are exposed over the same implementations: `read`, `find`,
`grep`, `glob`, `tree`, `edit`, `recall`, `batch`, `fetch`, `shell`,
`ingest`, `expand`, `mem`, `rewrite`, `discover`, and `cache_pack`.

`tz_batch` runs up to 16 independent sub-operations ({tool, args} pairs) in
one round trip: one combined capsule with per-op sections, unioned refs, and
summed raw accounting. A failing op renders its error inline and the rest
still run; nested batches are rejected. `tz_fetch` fetches an http(s) URL
through the system curl and keeps a TTL index beside the recovery cache:
repeat fetches inside the TTL (default 24h) serve the stored body without
touching the network, `fresh=true` re-fetches, and every serve — cached or
fresh — carries exact blob/file refs.

Network access for `tz_fetch` is **off by default** (it is an SSRF surface
for any MCP-capable agent): enable it with `TOKENZERO_FETCH=on`. When
enabled, targets are validated after DNS resolution — loopback, RFC1918,
link-local (including cloud metadata endpoints), carrier-grade NAT, and
IPv6 unique-local/link-local addresses are refused, the resolved IP is
pinned for the connection, and redirects (max 5) are re-validated per hop.
`TOKENZERO_FETCH_ALLOW=host1,host2` (suffix match) explicitly trusts hosts
and bypasses the IP checks for them — the escape hatch for intentionally
reachable private hosts; `TOKENZERO_FETCH_DENY` always refuses matching
hosts and wins over the allowlist.

`tz_recall` is the redundancy layer's retrieval half: it searches every
payload TokenZero has stored for this workspace (earlier reads, search
output, shell captures) as a literal case-insensitive substring match. Every
hit line carries its exact `tz://` ref, so the full payload is one
`tz_expand` away — re-finding content never requires re-reading files or
re-running commands. Recall is read-only over the cache and degrades to zero
hits with a `recall_cache_unreadable` diagnostic if the cache cannot be
parsed.

`grep` and `glob` are first-class because many agents call those tools
directly. They follow the same RACC contract as reads and shell output: exact
results are stored first, compact visible results are rendered second, and
`tz://` refs recover the full matched output.

### Search backends

`tz_find` and `tz_grep` are backed by ripgrep when available, with the
built-in scanner as a contained fallback. `TOKENZERO_SEARCH_BACKEND` selects
the backend: `rg`, `internal`, or `auto` (the default — use rg when a usable
binary is found on `PATH`, the internal scanner otherwise). `TOKENZERO_RG_PATH`
points at an explicit rg binary and skips the `PATH` lookup; the lookup result
is cached per engine instance.

The backends are output-compatible: rg runs with
`--line-number --no-heading --color=never --no-messages --hidden --no-ignore`
plus glob excludes mirroring the internal scanner's skip list (hidden entries
— which keeps the `.tokenzero` recovery cache out of results — `target`, and
`__pycache__`), matches are sorted into the internal scanner's traversal
order, and the flat `path:line:text` payload is byte-identical between
backends. Result limits truncate identically and set `truncated_by_results`.

Pattern semantics differ deliberately: `tz_find` always searches a literal
substring (rg runs with `--fixed-strings`), while `tz_grep` treats the
pattern as a regular expression under the rg backend — the parity upgrade
over the substring-only internal scanner, which still matches grep patterns
as literal substrings. An invalid regex under the rg backend returns an
`invalid_pattern` tool error carrying rg's parse message instead of silently
degrading to substring results.

A broken rg never breaks search: exit code 1 means zero matches, while a
spawn failure or unexpected exit status falls back to the internal scanner.
Search telemetry always reports `search_backend` (`rg` or `internal`) and
adds `fallback_reason` (e.g. `rg_not_found`, `rg exited with ...`) when the
rg backend was requested or auto-selected but the internal scanner answered.

Tool schemas use JSON Schema 2020-12. TokenZero does not mark any tool argument with `x-mcp-header`; no sensitive argument is mirrored into HTTP headers.

`tools/list` is kept deliberately lean (~3.7k tokens for the full 27-tool
catalog, ~81% below the long-form catalog): descriptions are one-line trigger
conditions, alias entries advertise a permissive `{"type": "object"}` stub
instead of repeating the canonical schema, and legacy argument aliases
(`cmd`/`input`/`script`, `args`, `timeout`/`timeout_secs`, glob's
`glob`/`query`, ingest's `input`) are accepted by the server but no longer
advertised. The long-form catalog — full section descriptions, canonical alias
schemas, and an `argumentAliases` map per tool — is served by
`resource://tokenzero/tools` (progressive disclosure; nothing is lost, it is
one `resources/read` away). The `initialize` response's `instructions` field
points agents at that resource.

Every advertised `inputSchema` is a plain JSON object: no top-level
combinators (`anyOf`/`oneOf`/`allOf`). Several MCP clients — Claude Code
included — silently drop a tool whose top-level schema is not a plain object,
which previously hid `tz_find`, `tz_grep`, `tz_shell`, and `tz_rewrite` from
agents. Either-or argument requirements (`query`/`pattern`,
`command`/`argv`) are stated in property descriptions and enforced
server-side with structured `INVALID_ARGUMENT` recovery data.

Because alias entries advertise a permissive `{"type": "object"}` stub,
clients that rely on the schema for type information may serialize arguments
as strings. The server coerces stringly-typed booleans (`"true"`/`"1"`),
integers (`"420"`), and JSON-encoded path arrays (`"[\"a\", \"b\"]"`) instead
of silently dropping them, so `read(raw="true", start_line="420")` behaves
the same through an alias as through the canonical tool.

By default, `tools/list` returns the full compatibility catalog with canonical
`tz_*` names and aliases. Agents that support request `_meta` should ask for a
small cluster when they only need one workflow:

```json
{
  "jsonrpc": "2.0",
  "id": "tools-material",
  "method": "tools/list",
  "params": {
    "_meta": {
      "tokenzero/toolCluster": "material"
    }
  }
}
```

Accepted clusters are `material` for read/search/tree/glob/expand operations
and `execution` for shell/ingest/cache/rewrite/discover/mem operations. Cluster
filters return canonical names only by default so each menu stays under seven
tools. Set `_meta.tokenzero/includeAliases` to `true` only when a client needs
short aliases in that filtered list. Invalid clusters return structured
`INVALID_ARGUMENT` error data with suggestions and example recovery calls.

## Resources

`resources/list` exposes read-only discovery resources for agents that need valid
parameter values before calling tools:

| Resource | Purpose |
| --- | --- |
| `resource://tokenzero/capabilities` | tool clusters, aliases, protocol versions, and next actions |
| `resource://tokenzero/tools` | complete tool catalog with schemas and agent-oriented descriptions |
| `resource://tokenzero/roots` | allowed file-system roots for paths and shell working directories |
| `resource://tokenzero/modes` | accepted render-mode values |
| `resource://tokenzero/cache` | local recovery/cache configuration without raw payload contents |
| `resource://tokenzero/shell-contract` | shell transport and command-success semantics |

Use `resources/read` with one of those URIs to retrieve the resource payload.

## Error Data Contract

JSON-RPC errors include machine-readable recovery data. Agents should inspect
`error.data` before retrying.

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32602,
    "message": "Invalid params",
    "data": {
      "error_type": "NOT_FOUND",
      "recoverable": true,
      "entity_type": "tool",
      "provided": "tz_reed",
      "fix_hint": "Call tools/list, then retry tools/call with one of available_options as params.name.",
      "available_options": ["tz_read", "tz_find"],
      "suggestions": [{"value": "tz_read", "score": 0.86}],
      "suggested_tool_calls": [{"method": "tools/list", "params": {}}]
    }
  }
}
```

`tools/call`, `resources/read`, and unknown top-level methods return typed
`NOT_FOUND` or `INVALID_ARGUMENT` data with valid options and suggested MCP
calls. Parse and invalid-request errors also include `error_type`, `recoverable`,
`fix_hint`, and `available_options` so clients can repair malformed requests
without guessing.

`logging/setLevel` errors expose the accepted syslog severity values in
`available_options` and `available_levels`, plus a retryable suggested call.

## HTTP Header Validation Contract

TokenZero remains a stdio-focused MCP server. Remote HTTP hosting is out of
scope for this pass. For reusable HTTP request handling, the protocol helper
validates the RC routing headers before dispatch:

- `MCP-Protocol-Version` must match `params._meta.io.modelcontextprotocol/protocolVersion`.
- `Mcp-Method` must match the JSON-RPC `method`.
- `Mcp-Name` must match `params.name` for `tools/call`.
- Missing, malformed, or mismatched headers return JSON-RPC code `-32001` (`HeaderMismatch`) before any tool executes.
- Unsupported modern protocol versions return JSON-RPC code `-32004` with `supported` and `requested` fields.

## Recovery Contract

Responses include visible content and recovery refs. Non-shell tools append a
one-line `refs:` footer to the text content (`refs: tz://blob/… tz://file/…`,
with secondary per-match refs summarized as `+N:<kind>`); shell output lists
its refs inline. Full metadata is available in JSON/debug paths; normal
display stays compact.

Common refs:

- `tz://blob/<id>` for raw payload recovery.
- `tz://file/<id>#Lx-Ly` for file-like ranges.
- `tz://search/<id>` for stored search hits.

### Surface exclusivity (CodeMode)

When the server runs in CodeMode (`TOKENZERO_MCP_TOOL_SURFACE=codemode` /
`--mode=codemode`), `tools/list` advertises **only** the CodeMode primary
tools (`tz_execute_code`, `tz_codemode_search`, `tz_codemode_describe`,
`tz_report_tool_issue`) for the whole session (`tools.listChanged=false`).
Per-op MCP tools (`tz_expand`, `tz_read`, shell, …) are not listed and
`tools/call` returns `unknown_tool` — one agent-visible surface, always.

1. Prefer `zero.token.expand` / `zero.token.read` inside `tz_execute_code`.
2. On expand miss / X0 the engine retries sibling stores and other internal
   routes before surfacing failure. Agents never switch to `tz_expand`.
3. Successful expand/read clears the failure streak in surface-health telemetry.
4. Telemetry: `resource://tokenzero/metrics` → `surface_health`.

CLI `tokenzero expand` / `tokenzero read` remain available outside MCP.

## Shell Contract

MCP `shell` defaults to auto policy and uses the same renderer as
`tokenzero run --json`. Status truth ships in the text output (`# shell`
headers with `command_success`, `exit_code`, and ref lines); the CLI response
envelope — policy, policy reason, command family, accounting, refs, and the
command-status truth model under `structuredContent.cli` — is available with
`TOKENZERO_MCP_ENVELOPE=compact|full`.

### Timeout and process-group kill (wqw.4)

Default shell timeout is 60s (`TOKENZERO_SHELL_TIMEOUT_SECS`,
`tokenzero mcp-server --shell-timeout-seconds`, or per-call
`timeout_seconds` / CodeMode `timeout_seconds`). On timeout the runtime
sends **SIGTERM then SIGKILL to the whole process group** (Unix
`process_group(0)`), returns partial captured stdout/stderr, and sets
`timed_out` / shell timeout status. Test proof of no orphans:
`timeout_process_group_kill_leaves_no_orphans_and_keeps_partial_stdout`
in `tokenzero-runtime`.

**Background cancel gap:** CodeMode/router `zero.token.shell({ background: true })`
async job cancel is tracked separately as bead
`tokenzero-shell-background-inert-3vv` (background option ignored / unwired
on some orchestrated paths). Foreground timeout process-group kill is fixed
here; do not assume background cancel reaps orphans until 3vv lands.

Shell responses distinguish tool transport from the child command:
`transport_status` can be `ok` while `command_success` is `false`. Nonzero
exits, missing `cd` paths, and likely pipeline-masked failures expose
`exit_code`, `failed_segment`, `pipeline_masking_warning`, `status_label`, and
exact refs for stdout, stderr, combined output, and the capture record.

CLI `tokenzero run` mirrors the child's exit status in both text and `--json`
modes, so `&&`/`||` chains, CI steps, and agent harnesses observe failures
directly (a masked pipeline that itself exits 0 still exits 0, matching `sh`
semantics). The `--json` envelope content is unchanged: machine consumers can
still read `telemetry.command_success` and `telemetry.exit_code`. Set
`TOKENZERO_RUN_CHILD_EXIT=0` to keep the legacy exit-0 envelope contract.
Over MCP, a child that ran but failed sets `isError: true` on the tool result
while the envelope keeps `transport_status: ok` and `command_success: false`.

Large shell streams are read concurrently and capped per stream instead of held
unbounded in memory. Defaults capture 4 MiB per stream and spill full streams to
`.tokenzero/shell-spills/` after 1 MiB. If a stream exceeds its capture cap,
`transport_status` becomes `degraded`, `diagnostic.code` is
`shell_output_truncated`, `refs_cover_full_output` is `false`, and telemetry
includes captured byte counts, total bytes seen, and the local spill path.
On macOS, truncated or spilled streams also trigger a process-local malloc
pressure-relief call before the tool response is returned. Telemetry reports
whether that relief was attempted and how many bytes malloc reported reclaiming.
`TOKENZERO_SHELL_CAPTURE_BYTES` and `TOKENZERO_SHELL_SPILL_BYTES` tune the caps.

## Cache Packs

`tz_cache_pack` and `cache_pack` return `tokenzero.cache-pack.v1` JSON for an
on-demand prompt-cache pack. Packs include deterministic stable-prefix content,
`cache_key`, `content_digest`, cacheable-token accounting, volatile-tail refs,
source refs, host hints, and invalidation reason. Cache packs are daemonless and
local-only.

## Install

```bash
tokenzero install --plan --json
tokenzero install --apply --json
tokenzero install --global --apply --mcp --shell --cli --json
```

The installer merges TokenZero into parseable MCP registries and preserves
existing entries. JSON and TOML registries are supported. Global install writes
a stable launcher plus a versioned `tokenzero-runtime-*` copy so clients do not
depend on a source checkout, `target/release`, or `PATH`. Unix-like hosts use
`~/.tokenzero/bin/tokenzero`; Windows hosts use
`~/.tokenzero/bin/tokenzero.cmd` with generated MCP configs launching through
`cmd.exe /C`.

See [`docs/install.md`](install.md) for rollback and global scope.

CodeMode telemetry serializes observed `raw_tokens`, `visible_tokens`, `bytes_materialized`, and `measurement_coverage_pct`; zero-operation job/status calls report measured zero savings instead of a synthetic baseline.
