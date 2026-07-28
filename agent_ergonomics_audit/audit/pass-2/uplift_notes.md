# Pass-2 uplift notes (priority deltas after triangulation)

**Source:** Phase 4 independent re-eval of playbook Top 10  
**Detail:** `pass-2/triangulation_top10.md`, `triangulation/pass2_top10.json`  
**Date:** 2026-07-27  

## Priority changes

| Rec | Pass-1 | Pass-2 | Delta | Why |
|---|---|---|---|---|
| **R-004a** (caps full clap tree) | (part of R-004 P1/920) | **P0 / 990** | **↑ uplift** | Family cross-cut #1 blast: 17 vs 57 verbs; agents following `capabilities --json` under-discover. Live: `commands_count=17`. |
| **R-002∪R-013** | R-002 P0/950; R-013 P1/800 | **P0 / 960** | merge; keep P0 | Confirmed islands + 168/180 useless; fold bad DYM gate into recovery. |
| **R-015** | P2 / 840 | **P1 / 880** | **↑ uplift** | 28 empty-desc verbs are active agent noise (self_doc min 107), not cosmetic. |
| **R-001** | P0 / 980 | **P1 / 820** | **↓ demote** | capabilities + doctor + robot-docs already multi-hop discovery; mega-command is composition. Merge with R-010. |
| **R-010** | P2 / 780 | merge → P1 mega-path | fold | Prefer doctor as implementation vehicle. |
| **R-013** | P1 / 800 | merge into R-002 | fold | Quality gate, not standalone rank. |
| R-003 | P1 / 900 | P1 / 910 | = | Share formatter with R-002. |
| R-005 | P1 / 880 | P1 / 900 | schedule earlier | Empty `regression_tests/`; pin with feature work. |
| R-008 | P1 / 820 | P1 / 830 | = | Extend registry; preserve install/prune pattern. |
| R-009 | P1 / 860 | P1 / 850 | slight demote | Still P1; CLI discovery (004a) outranks MCP map for CLI-first agents. |

## Do-not-regress (when applying later)

- Keep `capabilities --json` as machine contract (extend fields; do not replace with prose-only).
- Keep subcommand `--jsno` / `rn` / `search` recoveries while expanding coverage.
- Keep install / cache prune `--apply` safe defaults when expanding `dangerous_operations`.
- Keep robot-help / --robot-help / robot-docs triple (R-014 nearly done live).

## Residual (not priority uplifts, still open)

- Envelope polymorphism → future R-016  
- Honest `codemode_surface` feature flag → R-017  
- Canonical `tool` + `invoked_as` → R-018  
- Exit-code dictionary consistency → R-007  
- Verb-typo auto-act (reed→read) → R-011  

## Final Top 8 labels

1. R-004a — **P0**  
2. R-002+R-013 — **P0**  
3. R-003 — **P1**  
4. R-005 — **P1**  
5. R-015+R-004b — **P1**  
6. R-009 — **P1**  
7. R-008 — **P1**  
8. R-001+R-010 — **P1**  
