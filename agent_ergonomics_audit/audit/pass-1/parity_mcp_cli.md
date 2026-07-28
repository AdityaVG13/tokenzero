# MCP ↔ CLI Parity Audit (TokenZero)

**Date:** 2026-07-27  
**Binary:** `tokenzero` (PATH) v1.4.0 (`capabilities --json`)  
**Sources:** `tokenzero capabilities --json`, `tokenzero --help`, `crates/tokenzero-mcp/src/catalog.rs`, `crates/tokenzero-core/src/operation_abi/registry.rs`  
**Scope:** audit-only; no product code changes.

## Inventories

### MCP tools (20) — `crates/tokenzero-mcp/src/catalog.rs`

| MCP tool | Cluster | Closest CLI | Parity |
|---|---|---|---|
| `tz_read` | material | `read` | name diverge (`tz_` prefix) |
| `tz_find` | material | `find` (+ alias `search`) | name diverge; CLI adds `search` |
| `tz_grep` | material | `grep` | **semantic diverge**: CLI `grep` = alias of `find` (literal); MCP `tz_grep` = regex when `rg` active |
| `tz_recall` | material | `recall` | name diverge |
| `tz_batch` | execution | — | **MCP-only** |
| `tz_fetch` | web | `fetch` | name diverge |
| `tz_glob` | material | `glob` | name diverge |
| `tz_tree` | material | `tree` | name diverge |
| `tz_edit` | edit | `edit` | name diverge |
| `tz_shell` | execution | `run` (aliases `shell`, `rn`) | name + primary verb diverge |
| `tz_ingest` | execution | `ingest` | name diverge |
| `tz_expand` | material | `expand` | name diverge |
| `tz_mem` | execution | `mem` | name diverge |
| `tz_cache_pack` | execution | `cache-pack` | name diverge (underscore vs hyphen) |
| `tz_rewrite` | execution | `rewrite` | name diverge |
| `tz_discover` | execution | `discover` | name diverge |
| `tz_execute_code` | codemode | `codemode` | different surface name; CLI is shell trampoline |
| `tz_codemode_search` | codemode | `codemode 'search:…'` | CLI uses discovery string form, not a tool |
| `tz_codemode_describe` | codemode | `codemode 'describe:…'` | CLI uses discovery string form |
| `tz_report_tool_issue` | codemode | — | **MCP-only** |

ABI registry (`operation_abi/registry.rs`) lists the same 20 `tz_*` ops.

### Agent contract CLI (`tokenzero capabilities --json`) — 17 surfaces

`read`, `find`, `recall`, `fetch`, `glob`, `tree`, `edit`, `run`, `expand`, `mem`, `pulse`, `doctor`, `install`, `hook claude-code`, `capabilities`, `codemode`, `robot-docs guide`

### Full CLI (`tokenzero --help`) — 57 commands

Includes contract surface plus: `grep`, `ingest`, `session-open`, `rewrite`, `hook`, `discover`, `stats`, `session-ledger`, `cache`, `init`, `clients`, `client-status`, `robot-docs`, `cache-pack`, `mcp-server`, many `*-audit` / eval verbs, `help`.

**Gap:** contract advertises 17 / 57 CLI verbs. Agents that only read `capabilities --json` never learn `ingest`, `discover`, `rewrite`, `cache-pack`, `init`, `clients`, etc.

## Naming divergence matrix (agent-relevant)

| Intent | MCP | CLI canonical | CLI aliases / notes |
|---|---|---|---|
| Read file | `tz_read` | `read` | none |
| Search literal | `tz_find` | `find` | `search`, `grep` (literal!) |
| Search regex | `tz_grep` | *(none true)* | `grep` wrongly looks like regex |
| Shell | `tz_shell` | `run` | `shell`, `rn` |
| Expand ref | `tz_expand` | `expand` | requires `tz://`/`fz://`/`gz://` |
| Cache pack | `tz_cache_pack` | `cache-pack` | underscore vs hyphen |
| CodeMode plan | `tz_execute_code` | `codemode` | Tier B trampoline |
| Batch ops | `tz_batch` | — | no CLI batch |
| Doctor / install / robot-docs | — | `doctor`, `install`, `robot-docs` | **CLI-only** agent contract |
| Capabilities | — | `capabilities` | **CLI-only** (MCP has resources, not this verb) |

## Dangerous ops (safe-default)

From `capabilities --json` → `dangerous_operations`:

| Command | Mutation gate | Safe default |
|---|---|---|
| `install` | `--apply` | `tokenzero install --plan --json` |
| `cache prune` | `--apply` | `tokenzero cache prune --json` (dry-run) |

Live probes (savvy corpus): omit `--apply` → `dry_run: true` / `status: planned`. Gate holds.

## Exit-code contract vs agent misuse

Documented (`exit_codes`): 0 success, 1 blocked, 2 usage.

**Pitfall:** `tokenzero run --json -- false` returns **process exit 0** with `telemetry.command_success: false` and `status_label: command_failed`. Agents that only check argv exit code silently treat failed child commands as success. Contract text already points at `command_success`; still a top ergonomics footgun.

## Live savvy stress (categories H–M) — summary

See `audit/partial/intent_savvy_results.jsonl`.

| Category | Focus | Dominant outcome |
|---|---|---|
| H | `tz_*` as CLI subcommand | mostly **useless_error** (clap tip often wrong, e.g. `tz_read` → tip `tree`) |
| I | flag order / JSON typos | mixed; some recoveries (`--jason`, run delimiter) |
| J | aliases (`search`, `init`, `rn`, …) | mostly **inferred_and_acted** |
| K | robot-docs / robot-help | mostly **inferred_and_acted** |
| L | codemode typos | **useful_hint** / typed errors (this binary lacks JS sandbox feature) |
| M | expand prefix, apply gates, exit codes | expand → useful `invalid_ref`; apply gates hold |

## Top parity gaps (ranked)

1. **MCP `tz_*` names are not valid CLI subcommands** — no strip-prefix recovery; wrong clap tips (`tz_read`→`tree`).
2. **CLI `grep` ≠ MCP `tz_grep`** — same word, different semantics (literal alias vs regex).
3. **MCP `tz_shell` vs CLI primary `run`** — agents copy MCP name; tip to `shell` only for exact `tz_shell`.
4. **`tz_batch` has no CLI equivalent** — multi-op agents stuck on MCP-only path.
5. **`capabilities --json` under-advertises CLI** — 17 vs 57 verbs; omits `ingest`/`discover`/`rewrite`/`cache-pack`/`init`/`clients`.
6. **`tz_cache_pack` vs `cache-pack`** — underscore/hyphen split across transports.
7. **CodeMode triplication** — `tz_execute_code` + `tz_codemode_{search,describe}` vs single CLI `codemode` + `search:`/`describe:` strings.
8. **CLI-only setup/health** (`install`, `doctor`, `robot-docs`, `capabilities`) invisible on MCP tools/list.
9. **Expand scheme strictness** — bare hashes / paths fail with good error, but agents still pass paths (catalog documents the mistake).
10. **Exit-code vs `command_success`** — process exit 0 on failed shell child misleads agents.

## Recommended fixes (not applied; audit-only)

1. CLI unknown-subcommand recovery: strip `tz_` / `tokenzero_` prefixes and remap to CLI verbs with a one-line hint.
2. Split or rebrand CLI `grep` so it is not a silent alias of `find`, **or** make CLI `grep` true-regex and document parity with `tz_grep`.
3. Publish a `parity` block inside `capabilities --json`: `{mcp_name, cli_name, aliases, semantic_notes}[]`.
4. Add CLI `batch` or document that batch is MCP-only in robot-docs + capabilities.
5. Expand agent contract `commands` list to cover all non-audit core verbs (`ingest`, `discover`, `rewrite`, `cache-pack`, `init`, `clients`, `stats`, `session-open`).
6. Codemode robot-docs: one table mapping MCP tools ↔ CLI trampoline forms.
7. `run` JSON: promote `command_success` into top-level `status` when child fails, or non-zero process exit when `--strict-exit` is set (opt-in).
8. Install/cache prune: keep `--apply` gate; add explicit `mutation_applied: false` on dry-run for agents that ignore `dry_run`.

## Evidence paths

- Corpus: `agent_ergonomics_audit/audit/partial/intent_savvy.jsonl`
- Results: `agent_ergonomics_audit/audit/partial/intent_savvy_results.jsonl`
- Findings: `agent_ergonomics_audit/audit/partial/parity_findings.jsonl`
- This report: `agent_ergonomics_audit/audit/pass-1/parity_mcp_cli.md`
