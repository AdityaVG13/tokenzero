# TokenZero MCP compatibility contract

Status: compatibility policy, not a release announcement. Contract date: **2026-08-07**.

`tokenzero-mcp-compat` is the separately named Rust compatibility package. Its
installable per-operation artifact is `tokenzero-mcp`; the independent CodeMode
artifact is `tokenzero-codemode`. A process and an installation select one
surface only. This contract freezes the compatibility surface and explains how
to move existing MCP clients to CodeMode.

## Release-N support window

Release N means the first stable release after this contract that makes CodeMode
the default surface. Release N has not been announced by this document. The
calendar dates below are floors, not promised release dates:

- Compatibility support begins with release N and continues until an approved
  removal release.
- N+1 may require an explicit compatibility install no earlier than
  **2026-11-05** and no earlier than 90 days after the actual N publication,
  whichever is later.
- Removal is forbidden before **2027-02-03**, before 180 days after the actual N
  publication, or before two later stable releases, whichever is later.
- Removal also requires a complete client matrix, 60 days without an open P0 or
  P1 migration defect, representative corroborated demand evidence, a tested
  rollback package, a major-version notice, an owner-approved major release, and explicit owner approval.

### Feature freeze and supported fixes

The compatibility catalog is feature-frozen at release N. It receives only:

1. security or privacy fixes;
2. correctness fixes, including data loss, corruption, protocol framing, and
   process-lifecycle defects;
3. migration-blocker fixes needed to move a supported client to CodeMode; and
4. build or packaging fixes needed to preserve the published compatibility
   contract.

New operations, convenience features, and parity work belong on CodeMode. A
performance change is in scope only when it is required for one of the supported
fix classes. Compatibility support does not promise feature parity.

Protocol stdout remains JSON-RPC only. A compatibility, startup, or deprecation
warning must use stderr or a typed protocol diagnostic; it must never be printed
as prose on stdout.

Report a migration defect at
<https://github.com/AdityaVG13/tokenzero/issues>. When the server can still
answer, first call `tz_report_tool_issue` and attach its local report to the
issue. Do not put secrets or payload bytes in either report.

## Operation-complete migration

On CodeMode, put dependent calls in one `tz_execute_code` recipe instead of
making many per-operation MCP round trips. Bare aliases such as `read` follow
the same row as their canonical `tz_*` target.

| Compatibility operation | Alias | CodeMode path | Migration note |
|---|---|---|---|
| `tz_execute_code` | none | `tz_execute_code` | Primary CodeMode entry; retained. |
| `tz_codemode_search` | none | `tz_codemode_search` | Progressive method search; retained. |
| `tz_codemode_describe` | none | `tz_codemode_describe` | Describe a method or `capabilities`; retained. |
| `tz_read` | `read` | `zero.read` | Returns bounded text and exact refs. |
| `tz_find` | `find` | `zero.find` | Literal content search. |
| `tz_grep` | `grep` | `zero.grep` | Regex behavior still depends on the selected backend. |
| `tz_recall` | `recall` | `zero.recall` | Searches already stored payloads. |
| `tz_batch` | `batch` | `zero.batch` | Prefer one recipe when later calls depend on earlier results. |
| `tz_fetch` | `fetch` | `zero.fetch` | Keeps the same network opt-in and target policy. |
| `tz_glob` | `glob` | `zero.glob` | Path discovery only. |
| `tz_tree` | `tree` | `zero.tree` | Keep depth and result size bounded. |
| `tz_edit` | `edit` | `zero.edit` | Mutation stays explicit, atomic, and root-bounded. |
| `tz_shell` | `shell` | `zero.shell` | Mutation and command approval do not transfer automatically. |
| `tz_ingest` | `ingest` | `zero.ingest` | Stores external text behind an exact ref. |
| `tz_expand` | `expand` | `zero.token.expand` | Request line, selector, or symbol windows; use `tokenzero expand <ref> --raw` outside the recipe only for complete bytes. |
| `tz_mem` | `mem` | `zero.mem` | Diagnostic cache/config state. |
| `tz_cache_pack` | `cache_pack`, `cache-pack` | `zero.cache_pack` | Preserves stable-prefix and volatile-ref semantics. |
| `tz_rewrite` | `rewrite` | `zero.rewrite` | Plans a rewrite; it does not execute it. |
| `tz_discover` | `discover` | `zero.discover` | Runtime/filter readiness; use describe for the method catalog. |
| `tz_report_tool_issue` | `report_tool_issue`, `report-tool-issue` | `tz_report_tool_issue` | Retained as a primary diagnostic tool outside the recipe. |

Exact refs remain the recovery authority. A bounded CodeMode result is not the
full payload. Expand its ref instead of re-running an expensive operation.
`zero.token.expand` intentionally bounds its default visible result; use a line,
byte, or symbol selector. If one-call complete bytes are required, leave the
recipe and run `tokenzero expand <ref> --raw` against the exact ref.

### Protocol-method migration

| MCP method | CodeMode behavior |
|---|---|
| `initialize` | Unchanged MCP handshake; the selected surface is fixed for the session. |
| `notifications/initialized` | Unchanged notification. |
| `tools/list` | Lists only the CodeMode primary tools; `tools.listChanged` remains false. |
| `tools/call` | Call a listed primary tool, normally `tz_execute_code`. |
| `resources/read` | Still reads the advertised bounded resources, including the CodeMode catalog. |
| `server/discover` | Still reports surface-specific versions, capabilities, and identity. |

## Switch, rollback, and uninstall

Install one surface at a time. Installing the peer replaces the prior binary and
client registration atomically.

```bash
# Switch a macOS/Linux installation to CodeMode.
./packaging/install.sh --surface codemode

# Roll back the surface selection to compatibility.
./packaging/install.sh --surface mcp

# Inspect the selected surface and contract digest.
tokenzero doctor

# Restore the latest integration manifest if a client-config write must roll back.
tokenzero install --rollback latest

# Remove the selected surface and its managed registration.
./packaging/install.sh --uninstall --prefix ~/.tokenzero-install
```

A packaged surface binary provides the same lifecycle without a source checkout:

```bash
tokenzero-codemode install --surface codemode --prefix DIR
tokenzero-mcp install --surface mcp --prefix DIR
tokenzero-mcp uninstall --prefix DIR
```

Before switching, save the output of `tokenzero doctor --json`. After switching,
run it again and verify that exactly one surface and one peer-excluded SBOM are
reported. If verification fails, switch back to the previous surface; do not
start both servers in one process.
