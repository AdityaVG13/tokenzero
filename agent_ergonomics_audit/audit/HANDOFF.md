# HANDOFF — TokenZero agent-ergonomics audit (audit-only)

**Skill:** `agent-ergonomics-and-intuitiveness-maximization-for-cli-tools`  
**Target:** `/Users/aditya/AI/TokenZero` (`tokenzero` 1.4.0)  
**Mode:** `audit-only` (no Phase-5 product CLI rewrites)  
**Workspace:** `TokenZero/agent_ergonomics_audit/` (in-tree)  
**Passes:** 1 inventory+score+intent+recs → 2 triangulation/fresh-eyes/family/parity → 3 residual lowest dims  
**Completed:** 2026-07-27

## Strengths (do not regress)

- `tokenzero capabilities --json` rich contract (`tokenzero.capabilities.v1`)
- `robot-docs guide` / `commands` / aliases (`robot-help`, `--robot-help`)
- Bare `tokenzero` prints help (not TUI)
- Subcommand intent islands: `read --jsno`, `rn`→run, `search`→find often recover
- `doctor --robot-triage` mega JSON exists (`tokenzero.doctor.robot_triage.v1`)
- `install` / `cache prune` default safe; `--apply` gates mutations
- Stdout-as-data contract declared; many commands emit structured JSON

## Systemic gaps (epics)

| Epic ID | Title | P |
|---------|-------|---|
| `tokenzero-0oz4` | Root mega-command discoverability (`doctor --robot-triage` works; root/footer omit it) | P0 |
| `tokenzero-jokp` | Intent-inference islands (global typos + MCP name map) | P0 |
| `tokenzero-1cwf` | Exit-code + status-truth + envelope consistency | P0 |
| `tokenzero-1m61` | MCP–CLI parity (grep semantics, tool map, codemode truth) | P0 |
| `tokenzero-60t0` | Capabilities under-advertising + empty-help cluster | P1 |
| `tokenzero-eg3i` | Dangerous-op gating completeness | P1 |
| `tokenzero-odib` | Pin contracts with regression tests | P1 |

Full rec→bead map: `bead_ids.txt` (7 epics + 22 rec tasks + R-023 hook).

## Top apply order (next full pass)

1. **R-004** — Export full clap command set into `capabilities.commands` (+ fill empty help blurbs)
2. **R-002 + R-013** — Global Levenshtein-1 recovery + wrong-suggestion quality gate
3. **R-018** — `run` must not report process success when `command_success=false`
4. **R-016** — MCP `tz_*` → CLI did-you-mean (never `tz_read`→`tree`)
5. **R-017** — CLI `grep` vs MCP `tz_grep` semantic parity (literal vs regex)
6. **R-001** — Root alias to `doctor --robot-triage` + Agent surfaces footer
7. **R-003** — Error-Teaches exact corrected command on all usage errors
8. **R-005** — `audit/regression_tests/` pins (schema, stdout/stderr, negative DYM cases)
9. **R-019** — `feature_flags.codemode_surface` must match binary reality
10. **R-022 / R-023** — Mutator dry-run defaults; hook never silent-succeeds

## Artifacts

| Path | Role |
|------|------|
| `audit/manifest.json` | mode, passes, skill, counts |
| `audit/surface_inventory.jsonl` | **728** surfaces (112 verbs, 616 flags) |
| `audit/agent_surfaces.jsonl` | **122** scored (merged multi-scorer) |
| `audit/intent_inference_corpus.jsonl` | 180 stratified sample outcomes |
| `audit/partial/intent_naive.jsonl` | 1307 generated typos |
| `audit/partial/intent_savvy.jsonl` | 83 savvy + results |
| `audit/recommendations.jsonl` | 23 recs ranked |
| `audit/playbook.md` | top narrative |
| `audit/scorecard.md` / `heatmap.svg` | human scorecard |
| `audit/pass-1/` | scorer notes + provenance |
| `audit/pass-2/` | triangulation, fresh-eyes, family cross-cut |
| `audit/pass-3/` | residual gaps, pedagogy matrix, self-doc review |
| `audit/bead_ids.txt` | bead ledger |
| `audit/partial/scores_pass1_scorer{A,B,C}_*.jsonl` | independent scores |

## Scoring honesty

See `pass-1/SCORING_PROVENANCE.md`. Prefer subagent scorer partials over early template rows. Intent sample `useful_hint` rate is not a clean success metric (many are weak clap tips).

## Hard rules observed

- No `cargo test --workspace` / full workspace rustc test batteries
- No Phase-5 product surface rewrites
- No new git branch; workspace in-tree only
- Findings filed via `br create`

## Next agent entry

```bash
# Resume apply mode (if user requests full):
# mode=full on highest-priority beads from br ready
br show tokenzero-0oz4
br show tokenzero-jokp
cat agent_ergonomics_audit/audit/playbook.md
cat agent_ergonomics_audit/audit/pass-3/RESIDUAL_GAPS.md
```


## Intent metrics honesty

Primary rates live in `intent_metrics.json`. Combined unique invocations merge naive sample + savvy + pass-3 extras. Naive 180-row sample has only 48 unique argv — do not treat as representative CLI-wide recovery.
