# AGENTS.md -- TokenZero

Private local law (gitignored). Claude/Pi/Grok read this first. Operator override wins.

## Program (four repos, one system)

| Repo | Role |
|------|------|
| **ZeroStack** | Hub: contracts, composition, CodeMode host |
| **FSZero** | Bytes/state |
| **GraphZero** | Structure |
| **TokenZero** | Model-facing tokens, decision views, honest telemetry |

Never import FSZero/GraphZero. Depend only on hub contract crates. Pin hub by **pushed** `origin/main` rev.

**Specs / playbooks:** `~/Downloads/racc-r-handoff/`, `~/Downloads/TokenZero-GOLD-HANDOFF.md`. Receipts generate claims; no unlabeled %.

## Law

Same as hub: operator override; no silent deletes; `main` only; one writer; RCH targeted tests only; one bead at a time; smallest change; fail loud.

## This repo -- authority

Exact tokenizer identity, token pages/capsules, Decision Views, stable-prefix geometry, provider eligibility vs reported hit (never conflate), opaque reasoning-state transport, headroom, shell witnesses, output novelty, continuation classes. Strict mode never lowers model/effort/tools/output.

## Current program state (2026-08)

- RACC-R TokenZero adoption epic largely closed; raw-worker job surface, packaging bug-hunts, compression gates landed.
- Large open surface remains: never-worse-than-raw / million-line budgets, agent-ergonomics epics, gauntlet/oracle honesty, RADC corridors, slim-public-repo / CodeMode-default install, many p2 golden/MCP/compile residuals.
- Beads were reset to unassigned open (clean claim slate).

## Repo rules

- No peer-engine imports; composition is the hub's job.
- No unexpanded `~` store paths; converge on hub zero-store.
- Hub defects found here -> hub beads, not drive-by hub edits from this checkout.
- Benchmarks: never-worse gates honest; Q99 labeled.

## Ops defaults

```bash
br ready --json
br update <id> --claim
br sync --flush-only
rch exec -- env CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo test -p <crate> <filter> -- --test-threads=1
```
