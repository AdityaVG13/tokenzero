# Scoring provenance (audit honesty) — residual template duals = 0

## Honest inputs only (used in agent_surfaces.jsonl)

| File | Scorer | Surfaces |
|------|--------|----------|
| `pass-1/scorer_partials/scores_pass1_scorerA_core_fs.jsonl` | A | ~20 core FS + meta |
| `pass-1/scorer_partials/scores_pass1_scorerB_ops.jsonl` | B | ~101 ops/audit |
| `pass-1/scorer_partials/scores_pass1_scorerC_audit_verbs.jsonl` | C | 28 thin-help verbs |
| `pass-1/scorer_partials/scores_pass1_scorerD_core_fs_dual.jsonl` | D | 20 primary dual (independent live probes) |

## Excluded (not merged)

| Path | Why |
|------|-----|
| `partial/template_bootstrap_DO_NOT_MERGE/scores_pass1_scorerA.jsonl` | Template dual: identical evidence 68/68, only ±15/20/25 score jitter |
| `partial/template_bootstrap_DO_NOT_MERGE/scores_pass1_scorerB.jsonl` | Pair of above |

## Merge rules (`pass-1/MERGE_REPORT.md`)

- `scorer_count` = number of **distinct honest source files** for that surface_id
- `score_confidence = multi-scorer-median` **only if** scorer_count ≥ 2
- `score_confidence = single-scorer` otherwise; `score_spread` forced to 0 (no theater)
- `template_dual_used: false` on every merged row
- No `baseline for verb …; see deep_probes` evidence strings in merge

## Primary FS dual coverage

`read/find/grep/glob/tree/edit/expand/run/doctor/capabilities/…` are multi-scorer via **A + D** (and B where overlapping), not template A/B.

## Intent metrics

See `audit/intent_metrics.json`. Primary scorecard leads with **combined unique** + **reclassified** outcomes, not raw naive 168/180.
