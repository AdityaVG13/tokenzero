# Pass 3 -- Residual Gaps

Generated: 2026-07-27T23:14:24Z
Mode: audit-only. No product code changes. No `cargo test --workspace`.

## Extra intent histogram (n=25 targeted)

Beyond pass-1 sample (global ed1 flags, mostly useless_error). Pass-3 stresses **subcommand flags, order, aliases, missing args, wrong did-you-mean**.

| actual_outcome | count |
|---|---:|
| `recovered` | 14 |
| `useful_hint` | 5 |
| `wrong_hint` | 2 |
| `useless_error` | 2 |
| `domain_error_with_ladder` | 1 |
| `partial_hint` | 1 |

**Intent-shaped rows (excl. domain edit multi-match): n=24**
- recovered: 14 (58%)
- useful_hint: 5 (21%)
- weak/fail (useless_error + wrong_hint + partial_hint): 5 (21%)

### By category

- **subcommand_flag_typo**: {'recovered': 5, 'useful_hint': 5}
- **flag_order**: {'recovered': 4, 'domain_error_with_ladder': 1}
- **alias_confusion**: {'recovered': 5, 'wrong_hint': 1, 'partial_hint': 1}
- **missing_arg**: {'useless_error': 2}
- **wrong_didyoumean**: {'wrong_hint': 1}

### Contrast with Pass 1

| Corpus | n | useless_error | useful_hint / recovered |
|---|---:|---:|---:|
| Pass 1 stratified sample (mostly global flag typos) | 180 | 168 (93%) | 12 (7% useful_hint) |
| Pass 3 subcommand/order/alias targeted | 25 | 2 useless + 2 wrong_hint | 14 recovered + 5 useful_hint |

**Interpretation:** Intent inference is **asymmetric**. Wired recoveries and clap subcommand flag tips work well on primary agent verbs. Global root typos and unknown-subcommand suggestions remain the dark matter that kept intent_inference median ~390.

## Top residual gaps (ordered)

### G1. Root mega-command discoverability (agent_ergonomics) -- REFINED

- **Pass 1 R-001** claimed no robot-triage. **Pass 3 correction:** `tokenzero doctor --robot-triage` **exists** and returns `tokenzero.doctor.robot_triage.v1` JSON.
- Still missing: root `--robot-triage` / `robot-triage`, guide First Commands entry, capabilities.commands entry, bare help footer.
- Doctor triage schema is health-centric (findings/actions); not full `{quick_ref, recommendations, commands, project_health}` mega-bundle.

### G2. Global typo recovery still broken (intent_inference)

- `tokenzero --jsonn` → useless clap (E08); no tip.
- Subcommand `read --jsno` recovers. Uneven coverage confirmed again.
- Pass1 n=168 useless_error still the right score driver for this dim.

### G3. Wrong did-you-mean actively harms agents (intent_inference + error_pedagogy)

- `read --exlpain` → suggests `--help` (E09 / p3-025).
- `ls` → suggests `false-success-shell` (E14 / p3-021) because eval verbs pollute the subcommand dictionary.
- R-013 remains P1; R-015 (hide experimental verbs) would also improve suggestion quality.

### G4. Error-Teaches incomplete on 15/15 sampled errors (error_pedagogy)

- **0 full PASS** for (a)(b)(c). 6 PARTIAL, 9 FAIL.
- Bare `read` lacks Usage; domain ladders omit exact re-run; clap stops at `--help`.
- stdout/stderr split breaks stderr-only agent parsers for edit/expand.

### G5. Regression pins miss failure modes (regression_resistance)

- `cli_help_contract.rs` is solid for **happy recoveries** and capabilities schema (14 tests).
- `audit/regression_tests/` still empty.
- No negative tests for wrong hints / global typos / bare-arg cookbook.
- Median ~357 still justified for *gap coverage*, not for total absence of tests.

### G6. capabilities/help discovery cliff (self_documentation)

- capabilities.commands: **17** vs help verbs: **~60**.
- **28** empty help descriptions (eval/audit cluster).
- Guide claims on the 10 primary paths: **all MATCH**.

### G7. CodeMode guide vs installed binary (self_documentation / composability)

- Guide documents `codemode 'search:read'` and plan trampoline.
- This PATH binary: `surface-codemode` feature missing → exit 1.
- Agents copy-paste guide → hard fail without feature explanation.

### G8. Safety note (observed during probes; not full safety audit)

- `install --apply` and `cache prune --apply` executed successfully when probed (mutators live). dangerous_operations documents gates; out of primary residual ranking.

## New / refined recommendations

| ID | Title | Dims | Priority |
|---|---|---|---|
| R-001b | Advertise `doctor --robot-triage` in guide First Commands, capabilities, and root aliases (`robot-triage`, `--robot-triage` → doctor path) | agent_ergonomics, self_documentation | P0 |
| R-002 | Global Levenshtein-1 recovery (unchanged; reconfirmed) | intent_inference, error_pedagogy | P0 |
| R-003 | Error-Teaches cookbook for bare/missing/unknown | error_pedagogy | P1 |
| R-013 | Wrong did-you-mean quality gate + denylist polluted eval names from suggestions | intent_inference, error_pedagogy | P1 |
| R-015 | Gate/hide experimental eval verbs (also fixes ls→false-success-shell) | agent_ease_of_use, intent_inference | P2 |
| R-005e | Negative regression tests: wrong-hint, global `--jsonn`, bare `read` | regression_resistance | P1 |
| R-004 | Empty help + capabilities completeness (reconfirmed 28 empty) | self_documentation | P1 |
| R-016 | Guide CodeMode section: gate on feature_flags.codemode_surface / binary feature | self_documentation | P2 |
| R-017 | Domain errors: dual-write ladder; always include exact re-run | error_pedagogy, output_parseability | P2 |

## Deliverables index

| File | Content |
|---|---|
| `pass-3/intent_extra_results.jsonl` | 25 targeted wrong-invocation transcripts |
| `pass-3/error_transcripts/` | per-probe raw stdout/stderr |
| `pass-3/pedagogy_raw/` | expanded real error set |
| `pass-3/error_pedagogy_matrix.md` | 15-error (a)(b)(c) matrix |
| `pass-3/regression_resistance_inventory.md` | test pin inventory |
| `pass-3/self_doc_review.md` | 10 command guide vs live |
| `pass-3/RESIDUAL_GAPS.md` | this summary |

## Bottom line

Pass 3 **does not** re-score the full heatmap. It shows:

1. **Intent is not uniformly broken** -- primary-path recoveries work (14/25 recovered).
2. **Lowest dims remain correct targets** -- global typos, wrong hints, missing-arg pedagogy, empty regression goldens, mega-command discoverability (now refined: doctor flag exists, root/docs don't).
3. Highest leverage residual work: **R-001b + R-002 + R-013 + R-005e**.
