# Honest merge report

Generated: 2026-07-27T23:25:47Z

## Inputs (honest only)
- `pass-1/scorer_partials/scores_pass1_scorerA_core_fs.jsonl`
- `pass-1/scorer_partials/scores_pass1_scorerB_ops.jsonl`
- `pass-1/scorer_partials/scores_pass1_scorerC_audit_verbs.jsonl`
- `pass-1/scorer_partials/scores_pass1_scorerD_core_fs_dual.jsonl`

## Excluded
- `partial/template_bootstrap_DO_NOT_MERGE/scores_pass1_scorerA.jsonl` (template dual)
- `partial/template_bootstrap_DO_NOT_MERGE/scores_pass1_scorerB.jsonl` (template dual)
- Any partial with only ±15/20/25 jitter and identical evidence

## Results
- Surfaces: 113
- multi-scorer-median (≥2 independent source files): 48
- single-scorer: 65
- template_dual_used on any row: False
- baseline evidence strings remaining: 0

## multi-scorer rule
`scorer_count` = number of distinct honest source files contributing scores.
`score_confidence` = `multi-scorer-median` only if scorer_count ≥ 2; else `single-scorer` with score_spread all 0.
