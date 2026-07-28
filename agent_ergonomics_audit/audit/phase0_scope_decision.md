# Phase 0 scope decision — agent-ergonomics audit (TokenZero)

## Locked intake (non-interactive harness)

- **Target:** `/Users/aditya/AI/TokenZero`
- **Tool binary:** `tokenzero` (PATH `~/.local/bin/tokenzero`; also `target/release/tokenzero`)
- **Mode:** `audit-only` (score + recommend + file beads; no Phase-5 product surface rewrites)
- **Workspace:** `/Users/aditya/AI/TokenZero/agent_ergonomics_audit/` (in-tree; never sibling)
- **Branch policy:** current branch only (no new branch)
- **Triangulation:** peer multi-subagent; multi-model skill unavailable per preflight
- **CASS:** skip (optional cass skill unavailable)
- **Scope:** entire agent-facing CLI surface set (all `tokenzero` subcommands + material flags/env/exit/error + robot/MCP/codemode entry points)
- **Must not touch:** product feature redesigns; full `cargo test --workspace`; Phase 5 CLI rewrites
- **Allowed writes:** `agent_ergonomics_audit/**`, `.beads` via `br create`, narrow `.gitignore` if needed
- **Hard rule:** no full-workspace cargo/rustc tests; only targeted package/bin/`--help` smokes

## Maturity (runtime probe)

- `capabilities --json`: present (`tokenzero.capabilities.v1`)
- `robot-docs guide`: present
- stdout_contract declared: stdout data / stderr diagnostics; `--json` flag
- `discover-cli` initial auto-detect missed binaries (empty); corrected via runtime probe

## Pass plan

1. Pass 1: inventory + dual score primary surfaces + intent + recs
2. Pass 2: triangulation / fresh-eyes / family-cross-cut / re-score gaps
3. Pass 3: residual lowest dims + parity + self-doc review
4. Distill to `br` beads (epics for systemic clusters)
