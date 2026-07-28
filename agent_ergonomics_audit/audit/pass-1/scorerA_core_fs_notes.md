# Scorer A -- Core FS/context cluster notes (pass 1)

Scored at: 2026-07-27T23:08:22Z  
Binary: `tokenzero` 1.4.0 (`/Users/aditya/.local/bin/tokenzero`)  
Surfaces: 20  
Probes: `agent_ergonomics_audit/audit/partial/probe_scorerA_core_fs/` + help_probes

## Top 5 worst by mean score

| rank | surface_id | mean | lowest dims |
|------|------------|------|-------------|
| 1 | `verb__session-open` | 495.4 | agent_ease_of_use=350, error_pedagogy=350, self_documentation=350 |
| 2 | `verb__ingest` | 581.8 | agent_intuitiveness=400, agent_ease_of_use=400, error_pedagogy=400 |
| 3 | `verb__fetch` | 590.9 | determinism_and_reproducibility=450, agent_ease_of_use=500, error_pedagogy=500 |
| 4 | `verb__mem` | 600.0 | agent_ease_of_use=400, error_pedagogy=400, self_documentation=400 |
| 5 | `verb__recall` | 618.2 | agent_ease_of_use=450, regression_resistance=450, error_pedagogy=500 |

## Lowest dimensions across cluster

| dim | cluster mean | pattern |
|-----|--------------|---------|
| `error_pedagogy` | 552 | worst: verb__session-open:350; verb__ingest:400; verb__mem:400 |
| `agent_ease_of_use` | 572 | worst: verb__session-open:350; verb__ingest:400; verb__mem:400 |
| `regression_resistance` | 575 | worst: verb__session-open:350; verb__ingest:400; verb__mem:400 |
| `determinism_and_reproducibility` | 622 | worst: verb__fetch:450; verb__session-open:450; verb__mem:500 |
| `self_documentation` | 630 | worst: verb__session-open:350; verb__ingest:400; verb__mem:400 |
| `agent_intuitiveness` | 665 | worst: verb__ingest:400; verb__session-open:400; verb__edit:500 |
| `intent_inference` | 690 | worst: verb__session-open:400; verb__edit:550; verb__mem:550 |
| `composability` | 702 | worst: verb__session-open:550; verb__fetch:600; verb__edit:650 |
| `agent_ergonomics` | 752 | worst: verb__session-open:550; verb__ingest:600; verb__edit:650 |
| `output_parseability` | 772 | worst: flag__global__help:400; verb__robot-docs:450; flag__global__version:500 |
| `safety_with_recovery` | 935 | worst: verb__fetch:650; verb__codemode:700; verb__run:700 |

## Cluster findings (evidence-backed)

### Strengths

- **Uniform JSON envelope** on FS verbs: `schema_version=tokenzero.cli.v1`, `status`, `tool`, `detail_ref`, `refs`, `accounting` -- jq-safe without grepping.
- **`--jsno` silent normalize to `--json`** on read/find/grep/glob/tree (strong intent_inference); codemode/expand/doctor use did-you-mean tip instead.
- **`expand` + content-addressed `tz://blob/`** recovers exact bytes (`--raw`); central ergonomics win.
- **Global agent onboarding**: `--help` Agent surfaces → `capabilities --json` + `robot-docs guide`; doctor with `--robot-triage` / fix+undo.
- **edit path_not_allowed** includes multi-step Write recovery ladder (not bare clap).
- **Regression anchors exist**: `cli_help_contract.rs`, `golden_outputs.rs` (read/find), MCP tests for many ops.

### Weaknesses (lowest dims)

1. **agent_ease_of_use / self_documentation on core verbs** -- most verb `--help` is clap flag lists with empty option docs and **no examples**. Agents must leave the verb surface for robot-docs.
2. **error_pedagogy inconsistency** -- mix of custom (`read requires a path`, exit 1) vs clap required-args (exit 2). Missing-arg errors rarely print a full corrected example command.
3. **session-open / ingest / mem help thin** -- session-open purpose opaque; bare `ingest` accepts empty stdin as success; `ingest --text` is wrong shape (positional/path/`--stdin`).
4. **determinism_and_reproducibility** -- session_delta / dedup telemetry changes consecutive `--json` bodies even when file bytes identical (observed different SHA256 on two `read Cargo.toml --json`).
5. **codemode unavailable on this artifact** -- excellent help/errors, but `error.kind=unavailable` (missing rquickjs feature) means the mega-command cannot execute on PATH binary without tokenzero-codemode.
6. **Subcommand typo recovery is suggest-only** -- `reed` → tip includes `read` but does not auto-run; global `--jsno` fails without did-you-mean (unlike verb-level).

### Per-surface mean scores

| surface_id | mean |
|------------|------|
| `verb__capabilities` | 840.9 |
| `verb__codemode` | 772.7 |
| `verb__robot-docs` | 768.2 |
| `flag__global__help` | 754.5 |
| `flag__global__version` | 750.0 |
| `verb__doctor` | 736.4 |
| `verb__expand` | 727.3 |
| `verb__find` | 686.4 |
| `verb__run` | 686.4 |
| `verb__read` | 681.8 |
| `verb__grep` | 681.8 |
| `verb__glob` | 672.7 |
| `verb__tree` | 659.1 |
| `verb__rewrite` | 650.0 |
| `verb__edit` | 627.3 |
| `verb__recall` | 618.2 |
| `verb__mem` | 600.0 |
| `verb__fetch` | 590.9 |
| `verb__ingest` | 581.8 |
| `verb__session-open` | 495.4 |

## Method notes

- Scores rounded to nearest 50 per rubric.
- Scores >700 have invocation or file:line evidence in JSONL.
- Read-side safety scored 1000 with n/a evidence markers.
- No `cargo test --workspace`; probes limited to `--help` / missing-arg / `--json` / typo / one happy path.
- Independent scorer A; did not read scorer B outputs.
