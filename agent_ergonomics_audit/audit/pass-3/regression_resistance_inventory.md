# Pass 3 -- Regression Resistance Inventory

Generated: 2026-07-27T23:14:24Z
Mode: audit-only. **Did not** run `cargo test --workspace`. Inventory via source search only.

## Audit-local golden dir

| Path | Status |
|---|---|
| `agent_ergonomics_audit/audit/regression_tests/` | **Empty** (only `.gitkeep`) |
| Pass 1 R-005 proposed pins | Not landed |

## Existing crates tests that pin CLI ergonomics

### Primary: `crates/tokenzero/tests/cli_help_contract.rs` (14 tests)

| Test | Pins |
|---|---|
| `cli_bare_invocation_prints_useful_help` | bare `tokenzero` success; Usage; mentions capabilities/robot-docs/run |
| `cli_capabilities_json_exposes_agent_contract` | schema_version, features, feature_flags, commands aliases (`rn`, `shell`, `--jason`, `search`), output_schemas |
| `cli_robot_docs_guide_is_paste_ready_for_agents` | guide non-empty; contains capabilities + stdout contract sentence |
| `cli_agent_contract_outputs_are_deterministic_and_env_clean` | capabilities deterministic; feature_flags include intent_inference_aliases |
| `cli_robot_docs_read_search_and_run_are_env_clean` | robot-docs guide/commands/examples env-clean |
| `cli_agent_contract_aliases_recover_common_wrong_invocations` | intentional recovery corpus for common wrong forms |
| `cli_safe_subcommand_recoveries_choose_read_or_plan_surfaces` | doctor status / pulse stats / cache statuz / install plan-status safe |
| `cli_run_recovers_common_wrong_json_and_timeout_invocations` | run `--jsno` / order recoveries |
| `cli_run_preserves_trailing_child_json_without_delimiter` | run child JSON boundary |
| `cli_run_inline_shell_envelope_handles_empty_stdout` | empty stdout envelope |
| `cli_run_nonzero_exit_keeps_existing_failure_envelope` | child fail envelope |
| `cli_run_parent_json_keeps_inline_payload_unwrapped` | parent --json envelope |
| `cli_search_and_capabilities_json_typo_aliases_recover` | capabilities `--jsno`/`--jason`; search alias |
| `cli_help_discovers_agent_surfaces` | help lists capabilities + robot-docs |

**Strength:** strong pin for *happy recoveries* and capabilities schema.
**Gap:** does not pin wrong did-you-mean quality, global `--jsonn` failure, `ls`→wrong tip, bare `read` pedagogy, or Error-Teaches (a)(b)(c).

### Secondary: surface / IO / golden

| File | Ergonomics relevance |
|---|---|
| `crates/tokenzero/tests/surface_arg_rejection.rs` | codemode/mcp surfaces must not silent-success on CLI verbs |
| `crates/tokenzero/tests/cli_tools_io.rs` | read/expand/run/codemode/path-reject/cache-pack JSON contracts |
| `crates/tokenzero/tests/golden_outputs.rs` | golden JSON for read/find/artifacts |
| `crates/tokenzero/tests/passthrough_conformance.rs` | exit parity stdout/stderr for passthrough |
| `crates/tokenzero/src/cli_args.rs` | clap aliases e.g. `timout`/`timeout` on timeout_seconds |
| `crates/tokenzero-install/src/doctor.rs` | `doctor_robot_triage` implementation + doctor robot-docs strings |

### Doctor robot-triage (exists; under-tested for root discoverability)

| Surface | Evidence |
|---|---|
| `tokenzero doctor --robot-triage` | Implemented (`doctor_robot_triage`, schema `tokenzero.doctor.robot_triage.v1`) |
| Root `--robot-triage` / `robot-triage` | **Missing** (exit 2) |
| Guide First Commands | Lists doctor/status/pulse/install status; **omits** `doctor --robot-triage` |
| capabilities.commands | 17 names; **no** robot-triage entry |

## Coverage heatmap (ergonomics dimensions)

| Contract | Pinned by tests? | Notes |
|---|---|---|
| capabilities schema keys | **Yes** | cli_help_contract |
| robot-docs guide paste-ready | **Yes** | presence assertions |
| Subcommand `--jsno` / run recoveries | **Yes** | several tests |
| search / rn / robot-help aliases | **Yes** | recovery tests |
| Global flag typo did-you-mean | **No** | pass1 n=168 useless_error |
| Wrong did-you-mean gate | **No** | E09/E14 |
| Error-Teaches exact corrected command | **No** | 0/15 full PASS |
| stdout purity on --json errors | **Partial** | edit ladder on stdout |
| audit/regression_tests goldens | **No** | empty dir |
| Root mega-command discoverability | **No** | doctor flag exists; root gap unpinned |
| Empty help descriptions (28 verbs) | **No** | not asserted non-empty |
| capabilities.commands completeness vs --help | **No** | 17 vs ~60 |

## Residual vs Pass 1 R-005

R-005 still open for **audit-local** pins. In-tree `cli_help_contract.rs` is stronger than pass1 narrative suggested for recovery aliases, but **regression_resistance remains low** for the failure modes that dominate intent/pedagogy scores (global typos, wrong hints, missing-arg cookbook).

### Recommended pins (not applied; audit-only)

1. `R-005c` after R-002: `tokenzero --jsonn` → useful_hint with corrected form
2. Wrong-hint negative tests: `--exlpain` must not suggest `--help`; `ls` must not suggest `false-success-shell`
3. Bare `read`/`find` must include paste-ready example lines
4. `doctor --robot-triage` schema golden + root discoverability (help/guide/capabilities)
5. capabilities.commands length ≥ public agent verbs (or explicit experimental split)
