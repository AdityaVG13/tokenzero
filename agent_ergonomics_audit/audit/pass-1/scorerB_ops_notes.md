# Scorer B (ops cluster) — Pass 1 notes

- **scorer_id:** B
- **cluster:** ops/audit (non-pure-FS verbs)
- **binary:** tokenzero 1.4.0
- **rubric_version:** 1.0.0
- **scored:** 101 surfaces
- **probe log:** `audit/partial/ops_probe_scorerB.txt`
- **scores:** `audit/partial/scores_pass1_scorerB_ops.jsonl`
- **scored_at:** 2026-07-27T23:09:26Z

## Method

- Inventory verbs minus pure FS: `read/find/grep/glob/tree/edit/recall/fetch/expand/ingest`.
- Evidence from `--help` / safe dry-runs only (no `--apply`, no full workspace tests).
- Scores 0–1000, rounded to 50; weighted_score = arithmetic mean of 11 dims.
- Scores >700 require evidence objects; n/a safety on read-side scored 1000 with reason.
- Pass 1 `regression_resistance` mostly 0–300 (product tests only; empty `audit/regression_tests/`).

## Verified known findings

| Finding | Verdict |
|---------|---------|
| No top-level `--robot-triage` mega-command | **Confirmed.** `tokenzero --robot-triage` / `robot-triage` → clap error EXIT:2. |
| `doctor --robot-triage` mega | **Exists and works.** schema `tokenzero.doctor.robot_triage.v1` with summary + recommended_command. |
| capabilities + robot-docs strong | **Confirmed.** Deterministic caps JSON; robot-docs guide/commands/examples; robot-help aliases. |
| install/cache prune gated by `--apply` | **Confirmed.** Bare install = plan dry_run; bare cache prune dry_run; migrate-cleanup dual gate. |
| clap typos lack did-you-mean for unknown flags | **Partial.** Top-level flag typos often useless; many subcommand flags get `similar argument exists` (`install --jsno`→`--json`, `doctor --foo`→`--root`). Subcommand name typos often suggest peers. |
| doctor defaults to JSON-ish health | **Partial.** Bare `doctor` = full diagnose JSON (not health one-liner). `doctor health` is the cheap liveness path. |

## Strengths

1. **capabilities --json** — full agent contract (commands, exit_codes, dangerous_operations, agent_next_steps, codemode).
2. **doctor family** — diagnose/fix/undo/ls/explain/health/capabilities/robot-docs + `--robot-triage` mega.
3. **Safe mutation defaults** — install plan, cache prune dry-run, migrate-refs dry-run, migrate-cleanup `--apply`+`--confirm-cleanup`.
4. **Intent recoveries (selected)** — doctor status/statuz, pulse status/stats, install status→clients detect, capability typos, run rn/jsno/jason.
5. **robot-docs** — paste-ready guide + examples; top-level help Agent surfaces pointer.

## Weaknesses

1. **Polish Bar gap:** no top-level `tokenzero --robot-triage` collapsing doctor+pulse+install+clients health.
2. **Thin-help cluster:** many `*-audit` / eval verbs have empty root help descriptions and flag-only `--help`.
3. **Bare parent verbs requiring subcommand EXIT:2:** `clients`, `cache`, `robot-docs` (prefer defaults: detect / status / guide).
4. **codemode on this artifact:** classic binary missing rquickjs — clear error, but first-try plan fails.
5. **Intent corpus:** sampled flag typos mostly `useless_error` (168/180 overall); uneven typo recovery.
6. **regression_resistance:** no audit golden tests yet; only scattered product crate tests.

## Worst 8 (by weighted mean)

Floor score **409** is shared by **25** thin-help / empty-description audit-eval verbs (tied). First eight by surface_id sort within the floor:

| rank | surface_id | mean |
|------|------------|------|
| 1 | `verb__adapter-approval-audit` | 409 |
| 2 | `verb__adapter-approval-template` | 409 |
| 3 | `verb__artifact-handoff` | 409 |
| 4 | `verb__bench` | 409 |
| 5 | `verb__bench_competitors_h7a18c90a` | 409 |
| 6 | `verb__bench_help_h32757b6a` | 409 |
| 7 | `verb__cache-pack` | 409 |
| 8 | `verb__completion-audit` | 409 |

Other tied-at-409 examples: `verb__harm-eval`, `verb__os-reach-audit`, `verb__install-smoke`, `verb__security-privacy-audit`, `verb__ws-skeleton`.

## Best 8 (by weighted mean)

| rank | surface_id | mean |
|------|------------|------|
| 1 | `verb__capabilities` | 877 |
| 2 | `verb__run` | 813 |
| 3 | `verb__doctor_health_hf87098d2` | 809 |
| 4 | `verb__doctor` | 795 |
| 5 | `verb__robot-docs_guide_h68c32e8c` | 786 |
| 6 | `verb__doctor_capabilities_hcc21b3e9` | 781 |
| 7 | `verb__doctor_diagnose_h4a0c1b7c` | 768 |
| 8 | `verb__doctor_explain_h291ff9ed` | 754 |

## Dim notes (ops cluster)

- **agent_ergonomics:** doctor `--robot-triage` and capabilities approach mega-command quality; top-level Polish Bar still open.
- **intent_inference:** run/capabilities/doctor aliases strong; generic clap flag typos weak.
- **safety_with_recovery:** install/cache migrate-cleanup excellent; pulse import thinner.
- **self_documentation:** robot-docs/capabilities excellent; empty-desc audit verbs pull mean down.
- **output_parseability:** most read ops have `--json` + schema_version; robot-docs markdown is intentional non-JSON.

## Independence

Did not read `scores_pass1_scorerA*.jsonl` or other scorer B cluster files for score values.

