# TokenZero Agent Ergonomics Scorecard — honest merge (audit-only)

- Generated: 2026-07-27T23:26:31Z
- Surfaces scored (honest partials only): **113**
- multi-scorer-median (≥2 independent source files): **48**
- single-scorer (explicit, no spread theater): **65**
- Inventory (all CLI surfaces): 728 (112 verbs / 616 flags)
- Template duals used in scorecard: **0** (quarantined under partial/template_bootstrap_DO_NOT_MERGE/)

## Intent stress (honest headline)

- Combined unique invocations (naive sample ∪ savvy ∪ pass-3): **148**
- Combined reclassified outcomes: `{'useless_error': 64, 'useful_hint': 34, 'wrong_hint': 2, 'inferred_and_acted': 45, 'domain_error_with_ladder': 1, 'skipped_unsafe': 2}`
- Naive sample alone: n=180 rows but only **48 unique** invocations; raw actual_outcome={'useless_error': 168, 'useful_hint': 12}
- Naive reclassified (clap --help tips → wrong_hint when inappropriate): {'useless_error': 168, 'useful_hint': 11, 'wrong_hint': 1}
- Savvy (n=83): {'useless_error': 19, 'useful_hint': 26, 'inferred_and_acted': 36, 'skipped_unsafe': 2}
- Pass-3 extra targeted (n=25): {'inferred_and_acted': 14, 'useful_hint': 5, 'domain_error_with_ladder': 1, 'wrong_hint': 2, 'partial_hint': 1, 'useless_error': 2}

**Do not use raw naive 168/180 useless_error as CLI-wide recovery.** Intent is asymmetric: primary-path subcommand recoveries often work; global flag typos and MCP-name-as-CLI often fail. See `intent_metrics.json` and `pass-3/RESIDUAL_GAPS.md`.

## Dimension medians (honest rows only)

| Dimension | Median | Mean | Min | Max |
|---|---:|---:|---:|---:|
| agent_intuitiveness | 700 | 650 | 350 | 950 |
| agent_ergonomics | 700 | 623 | 350 | 900 |
| agent_ease_of_use | 650 | 570 | 225 | 950 |
| output_parseability | 700 | 673 | 350 | 950 |
| error_pedagogy | 550 | 531 | 325 | 900 |
| intent_inference | 500 | 513 | 300 | 900 |
| safety_with_recovery | 900 | 842 | 450 | 1000 |
| determinism_and_reproducibility | 525 | 589 | 450 | 900 |
| self_documentation | 650 | 567 | 175 | 1000 |
| composability | 700 | 694 | 500 | 900 |
| regression_resistance | 100 | 157 | 0 | 615 |

## Lowest 30 surfaces

| surface_id | mean | confidence | scorers | worst dims |
|---|---:|---|---|---|
| verb__ws-skeleton | 343.2 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=225 |
| verb__bench | 347.7 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__install-smoke | 359.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__quote | 359.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__repo-inventory | 375.0 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=175, agent_ease_of_use=250 |
| verb__exact-recovery-audit | 381.8 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__harm-eval | 381.8 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=175, agent_ease_of_use=250 |
| verb__prompt-cache-pack | 381.8 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=175, agent_ease_of_use=250 |
| verb__adapter-approval-template | 384.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__artifact-handoff | 384.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__completion-audit | 384.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__protected-anchor-audit | 384.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__shell-matrix | 384.1 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__adapter-approval-audit | 386.4 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=275 |
| verb__one-shot-eval | 386.4 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__exact-recovery-shell | 388.6 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__false-success-shell | 390.9 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__os-release-artifact | 390.9 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=275 |
| verb__security-privacy-audit | 390.9 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=250 |
| verb__os-reach-audit | 393.2 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=275 |
| verb__source-currency-audit | 395.5 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=275 |
| verb__cache-pack | 402.3 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=225, agent_ease_of_use=275 |
| verb__mcp-soak | 404.5 | multi-scorer-median | 2 | regression_resistance=50, self_documentation=225, agent_ease_of_use=300 |
| verb__reach | 406.8 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=225, agent_ease_of_use=275 |
| verb__bench_competitors_h7a18c90a | 409.1 | single-scorer | 1 | regression_resistance=0, intent_inference=300, self_documentation=300 |
| verb__bench_help_h32757b6a | 409.1 | single-scorer | 1 | regression_resistance=0, intent_inference=300, self_documentation=300 |
| verb__claim-audit | 422.7 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=200, agent_ease_of_use=275 |
| verb__mcp-smoke | 438.6 | multi-scorer-median | 2 | regression_resistance=75, self_documentation=250, agent_ease_of_use=325 |
| verb__package-audit | 443.2 | multi-scorer-median | 2 | regression_resistance=25, self_documentation=225, agent_ease_of_use=275 |
| verb__mcp-server | 459.1 | multi-scorer-median | 2 | regression_resistance=125, self_documentation=425, agent_ease_of_use=450 |

## Highest 15 surfaces

| surface_id | mean | confidence | scorers |
|---|---:|---|---:|
| verb__doctor_health_hf87098d2 | 809.1 | single-scorer | 1 |
| verb__capabilities | 807.3 | multi-scorer-median | 3 |
| verb__robot-docs_guide_h68c32e8c | 786.4 | single-scorer | 1 |
| verb__doctor_capabilities_hcc21b3e9 | 781.8 | single-scorer | 1 |
| verb__doctor_diagnose_h4a0c1b7c | 768.2 | single-scorer | 1 |
| verb__doctor_explain_h291ff9ed | 754.5 | single-scorer | 1 |
| verb__robot-docs_commands_h76b0f0e3 | 750.0 | single-scorer | 1 |
| verb__robot-docs_examples_hea6be2e7 | 745.5 | single-scorer | 1 |
| verb__install | 745.5 | single-scorer | 1 |
| verb__client-status | 736.4 | single-scorer | 1 |
| verb__pulse | 731.8 | single-scorer | 1 |
| verb__doctor_robot-docs_h96c79805 | 731.8 | single-scorer | 1 |
| verb__robot-docs | 730.9 | multi-scorer-median | 3 |
| verb__pulse_stats_h856332e0 | 727.3 | single-scorer | 1 |
| verb__codemode | 727.3 | multi-scorer-median | 3 |

## Dual-scorer coverage (primary agent verbs)

| surface_id | scorer_count | sources |
|---|---:|---|
| verb__cache | 1 | scores_pass1_scorerB_ops.jsonl |
| verb__capabilities | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__codemode | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__doctor | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__edit | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__expand | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__fetch | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__find | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__glob | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__grep | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__ingest | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__install | 1 | scores_pass1_scorerB_ops.jsonl |
| verb__mem | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__read | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__recall | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__robot-docs | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__run | 3 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerB_ops.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |
| verb__tree | 2 | scores_pass1_scorerA_core_fs.jsonl, scores_pass1_scorerD_core_fs_dual.jsonl |

## Provenance

- Honest partials: `pass-1/scorer_partials/scores_pass1_scorer{A_core_fs,B_ops,C_audit_verbs,D_core_fs_dual}.jsonl`
- Merge rules: `pass-1/MERGE_REPORT.md`
- Template duals excluded: `partial/template_bootstrap_DO_NOT_MERGE/`

