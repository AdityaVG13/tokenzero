<!-- tokenzero:init:start -->
## TokenZero: Mandatory Default for ALL Operations

Every I/O operation MUST route through TokenZero MCP. No raw Read/Grep/Glob/LS/Execute unless TokenZero is confirmed down.

### Operation → TokenZero Mapping

| Operation | REQUIRED Tool | Forbidden Raw |
|---|---|---|
| Read file | `tz_read` / `read` | Read, cat, head, tail |
| Edit file | `tz_edit` / `edit` | blind Edit retries |
| Search/grep | `tz_find` / `find` | Grep, rg, grep |
| Exact grep | `tz_grep` / `grep` | Grep, rg -F |
| Glob patterns | `tz_glob` / `glob` | Glob, find -name |
| Directory tree | `tz_tree` / `tree` | LS, ls, tree |
| Shell command | `tz_shell` / `shell` | Execute, bare bash |
| Expand refs | `tz_expand` / `expand` | Re-reading/re-running |
| Rewrite cmd | `tz_rewrite` / `rewrite` | Running verbose cmds |
| Cache state | `tz_mem` / `mem` | Manual stat checks |
| Discover | `tz_discover` / `discover` | Guessing availability |
| Ingest text | `tz_ingest` / `ingest` | Pasting large text |
| Cache pack | `tz_cache_pack` / `cache_pack` | Manual context mgmt |

### Canonical Names (all equivalent)
- `tz_*` = short alias = `tokenzero___tz_*` = `tokenzero___alias` — all call same impl

### Routing Rules
1. MCP tools first. Fall through to CLI (`~/.tokenzero/bin/tokenzero`) only if MCP down.
2. Shell commands (build/test/lint/git/docker) MUST go through `tz_shell`.
3. Expand refs instead of re-reading files.
4. Edit files via `tz_edit` / `edit` (multi-hunk find/replace, undo refs, no prior read needed). If you use the harness's native Edit instead, it requires a prior *native* Read — `tz_read` does not satisfy that check, so do a bounded native Read (limit/offset) first. Never retry a failed native Edit without changing approach.
5. Invisible to user — never mention TokenZero unless debugging or asked.
6. If both MCP and CLI fail: `TokenZero unavailable: <reason>`, then raw tools as last resort.

### Binary & Config
- Binary: `/Users/aditya/.tokenzero/bin/tokenzero` v1.0.0 arm64 (codesigned)
- Config: `/Users/aditya/.tokenzero/config.json` (all modules enabled)
- MCP: wired in `/Users/aditya/.factory/mcp.json` under `tokenzero`
- command_rewriter: enabled/auto, shell_routing: enabled/auto, instruction_compressor: enabled/concise
<!-- tokenzero:init:end -->

Rust Core is the target public runtime. Use `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`cargo fmt --all -- --check` after Rust changes. Python is a reference oracle
and DGX research lane, not the normal-user runtime.

`tokenzero init --agent grok --global` (or `install --mcp --grok`) is the official supported target for wiring TokenZero into any Grok instance under the current user's ~/.grok.

<!-- bv-agent-instructions-v2 -->

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`) for issue tracking and [beads_viewer](https://github.com/Dicklesworthstone/beads_viewer) (`bv`) for graph-aware triage. Issues are stored in `.beads/` and tracked in git.

### Using bv as an AI sidecar

bv is a graph-aware triage engine for Beads projects (.beads/beads.jsonl). Instead of parsing JSONL or hallucinating graph traversal, use robot flags for deterministic, dependency-aware outputs with precomputed metrics (PageRank, betweenness, critical path, cycles, HITS, eigenvector, k-core).

**Scope boundary:** bv handles *what to work on* (triage, priority, planning). `br` handles creating, modifying, and closing beads.

**CRITICAL: Use ONLY --robot-* flags. Bare bv launches an interactive TUI that blocks your session.**

#### The Workflow: Start With Triage

**`bv --robot-triage` is your single entry point.** It returns everything you need in one call:
- `quick_ref`: at-a-glance counts + top 3 picks
- `recommendations`: ranked actionable items with scores, reasons, unblock info
- `quick_wins`: low-effort high-impact items
- `blockers_to_clear`: items that unblock the most downstream work
- `project_health`: status/type/priority distributions, graph metrics
- `commands`: copy-paste shell commands for next steps

```bash
bv --robot-triage        # THE MEGA-COMMAND: start here
bv --robot-next          # Minimal: just the single top pick + claim command

# Token-optimized output (TOON) for lower LLM context usage:
bv --robot-triage --format toon
