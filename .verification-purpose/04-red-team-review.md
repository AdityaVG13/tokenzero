# Red-Team Review — TokenZero Test Suite Purpose Ledger

**Reviewer:** Adversarial Reviewer  
**Date:** 2026-07-05  
**Target:** `.verification-purpose/02-purpose-ledger.md`  
**Question:** The first pass classified ~582/764 tests as program-durable (76%). Are 600+ tests really worth shipping?

---

## Executive Summary

**The first pass was too generous by ~57 tests.** The original audit correctly identified ~37 session-only tests but missed several classes of over-classification:

1. **Internal table-driven unit tests** that mirror function signatures, not contracts
2. **Toolchain-specific happy-path renders** that test one tool each with no failure-class signal
3. **Optimization/formatting detail assertions** (compact JSON, inline vs sidecar, internal fields)
4. **Platform-specific dead code** gated behind `#[cfg(windows)]` on macOS-only CI
5. **Source-level substring audits** better enforced by compiler lints
6. **Benchmark harness tests** living in `src/` instead of `benches/`
7. **Trivial sanity checks** (token count > 0, echo ok, mode alias parsing)

**Revised estimate: ~525 program-durable, ~94 session-only, ~45 investigate/helpers.**  
Still 69% program-durable — the suite is genuinely high-value — but the "must-ship" set is tighter.

---

## Per-Crate Revised Classification

### tokenzero-core (55 tests)

**First pass:** 47 program-durable, 1 session-only, 6 investigate  
**Revised:** 37 program-durable, 8 session-only, 10 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `shell_policy/tables.rs` — all 5 table tests (`command_succeeded_table`, `classify_command_status_table`, `auto_shell_policy_table`, `decide_shell_policy_table`, `shell_family_table`) | program-durable | **session-only** | Internal function tables. Each test constructs a struct with `(input, expected_output)` and calls one internal function. These mirror implementation signatures, not public contracts. The same behavior is tested at integration level by `tests/shell_semantics.rs` and the render tests. Any refactor of the internal data model breaks these without changing observable behavior. The ledger itself notes "some overlap with integration tests but table form is valuable for enumeration" — but enumeration of internal function behavior is the definition of implementation-detail testing. |
| `tests/misc.rs` — `mode_aliases_map_to_new_policy_names` | program-durable | **session-only** | Trivial string-parsing table. `Mode::from_str` is tested by every test that constructs a `Mode`. This is a sanity check, not a contract. |
| `tests/misc.rs` — `token_count_is_nonzero_for_text` | program-durable | **session-only** | Asserts `count_tokens("hello world") >= 2`. Not a contract — no specific value, no boundary, no failure class. |
| `tests/shell.rs` — `windows_shell_wrapped_search_commands_keep_search_summary` | program-durable | **session-only** | Tests `powershell.exe`, `pwsh`, and `cmd.exe` wrapped search commands. The same search-family + summary contract is already tested by `shell_wrapped_rg_search_keeps_search_family_and_summary` (bash wrapper) and `rg_pcre2_search_output_gets_search_summary` (direct). Windows wrapper variants add no new failure class. |
| `tests/shell.rs` — `shell_c_wrappers_do_not_analyze_positional_args_as_code` | program-durable | **investigate** | Thin negative test for `bash -c 'true' 'false | true'`. May overlap with `shell_c_wrappers_detect_masked_inner_pipeline_failures` which tests the same analysis function. Keep if it catches a distinct injection vector. |
| `tests/shell.rs` — `real_shell_operators_still_drive_status_warnings` | program-durable | **investigate** | Duplicates the `false | true` pipeline masking test from `shell_render_exposes_status_truth_and_refs`. Also tests `split_shell_segments` inline — a different function. Keep if `split_shell_segments` isn't tested elsewhere. |

**Downgrades: 10 tests** (5 table + 2 trivial + 1 platform overlap + 2 investigate)

---

### tokenzero-filters (16 tests)

**First pass:** 14 program-durable, 1 session-only, 1 investigate  
**Revised:** 14 program-durable, 1 session-only, 1 investigate  

**No changes.** The first pass was accurate here. Every test covers a distinct safety boundary (destructive commands, compound detection, injection safety, read-only vouching). This is the best-classified crate.

---

### tokenzero-runtime (40 tests)

**First pass:** 28 program-durable, 10 session-only, 2 investigate  
**Revised:** 25 program-durable, 15 session-only, 0 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `env_i_style_invocation_works` | session-only | session-only ✓ | Already flagged. Trivial `echo ok`. |
| `cmd_split_preserves_doubled_quotes_inside_quoted_arguments` | investigate | **session-only** | Thin cmd escape edge. The proptest `generated_split_roundtrips_displayed_cmd_and_powershell_args` covers the roundtrip for cmd platform. This tests one specific escape sequence with no distinct failure class. |
| `powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments` | investigate | **session-only** | Same reasoning — thin PowerShell `''` escape. Proptest covers roundtrip. |
| `windows_findstr_regex_metacharacters_inside_argv_stay_argv` | program-durable | **investigate** | Tests `findstr` argv display formatting on Windows. The display function is also tested by `argv_display_command_quotes_shell_metacharacters` in core/shell.rs. Keep only if the Windows display path is distinct. |

**Downgrades: 3 new** (plus 10 already flagged = 13 total)

---

### tokenzero-recovery (46 tests)

**First pass:** 38 program-durable, 3 session-only, 5 investigate  
**Revised:** 32 program-durable, 9 session-only, 5 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `persist_compacts_duplicate_order_entries` | session-only | session-only ✓ | Already flagged. |
| `persisted_cache_is_compact_json` | session-only | session-only ✓ | Already flagged. |
| `small_blob_stays_inline` | session-only | session-only ✓ | Already flagged. |
| `line_range_payloads_skip_source_fingerprint` | investigate | investigate ✓ | Already flagged. |
| `lock_file_is_stable_anchor_not_deleted_on_drop` | program-durable | **session-only** | Asserts internal persistence mechanism (OS file lock anchor). The contract is "concurrent persistence works" — tested by `concurrent_persistence_preserves_all_thread_payloads`. The lock anchor detail is implementation. |
| `virtual_paths_skip_source_fingerprint` | program-durable | **session-only** | Asserts `source_fingerprint.is_none()` on an internal field. The contract is "virtual paths don't stale-check" — but the test checks the field, not the behavior. Should be rewritten to test observable expand behavior. |
| `line_range_payloads_skip_source_fingerprint` (if not already investigate) | investigate | **session-only** | Same pattern — asserts internal field, not observable behavior. |

**Downgrades: 3 new** (plus 3 already flagged = 6 total)

---

### tokenzero-pulse (31 tests)

**First pass:** 31 program-durable, 0 session-only, 0 investigate  
**Revised:** 29 program-durable, 2 session-only, 0 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `lock_file_is_stable_anchor_not_deleted_on_drop` | program-durable | **session-only** | Same pattern as recovery — asserts internal lock anchor persistence. The concurrency contract is tested by `sync_waits_for_transient_lock_contention`. |
| `lock_wait_retries_platform_lock_contention_errors` | program-durable | **session-only** | Thin unit test: `assert!(retryable_pulse_lock_wait_error(&would_block))`. Three assertions on an internal error classification function. The behavior is tested by `sync_waits_for_transient_lock_contention`. |

**Downgrades: 2 tests**

---

### tokenzero-install (144 tests)

**First pass:** 129 program-durable, 13 session-only, 2 investigate  
**Revised:** 121 program-durable, 21 session-only, 2 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `doctor_ls_lists_run_artifacts_for_agents` | session-only | session-only ✓ | Already flagged. |
| `doctor_explain_returns_known_finding_when_not_current` | session-only | session-only ✓ | Already flagged. |
| `doctor_robot_triage_is_single_call_json_contract` | session-only | session-only ✓ | Already flagged. |
| `doctor_robot_docs_describe_negative_space` | session-only | session-only ✓ | Already flagged. |
| `surface_install_always_writes_classic` | session-only | session-only ✓ | Already flagged. |
| 3 Windows-only tests | session-only | session-only ✓ | Already flagged. |
| `detect_present_agents_probes_home_and_path` | session-only | session-only ✓ | Already flagged. |
| 3 duplicate CRC tests in zip.rs | session-only | session-only ✓ | Already flagged. |
| `doctor_capabilities_names_doctor_contract_subcommands` | program-durable | **session-only** | 40+ assertions checking every field of a JSON blob: every command name, every fixer id, every detector id, every exit code. This is a snapshot test disguised as a contract test. If a new exit code is added, this test breaks but the program still works. The doctor schema contract is tested by `doctor_reports_agent_contract_for_healthy_root`. |
| `apply_and_rollback_restore_temp_home` | program-durable | **investigate** | Tests `apply` + `rollback` with AGENTS.md content. Overlaps with `global_cli_runtime_copy_is_rollback_capable` which tests the same lifecycle with binary copy. Keep if AGENTS.md path has distinct failure class. |
| `manifest_is_complete_for_every_written_file` | program-durable | **investigate** | Tests internal manifest structure. The contract is "rollback works" — tested by rollback tests. This checks implementation detail of how manifest entries are stored. |

**Downgrades: 3 new** (plus 13 already flagged = 16 total)

---

### tokenzero-mcp (~252 tests)

**First pass:** ~120 program-durable, ~5 session-only, 0 investigate  
**Revised:** ~115 program-durable, ~10 session-only, 0 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| `bench_tests.rs` — all 4 tests | session-only | session-only ✓ | Already flagged. Benchmark harness in src/. |
| Thin telemetry format check (if exists) | program-durable | **session-only** | Per minimal plan. |
| Legacy API smoke (if exists) | program-durable | **session-only** | Per minimal plan. |

**Downgrades: ~5 new** (plus 4-5 already flagged = ~10 total)

---

### tokenzero (CLI) (~180 tests)

**First pass:** ~175 program-durable, 4 session-only, 2 investigate  
**Revised:** ~168 program-durable, 8 session-only, 4 investigate

| Test / Group | First Pass | Revised | Rationale |
|---|---|---|---|
| 4 Windows-only tests in `src/tests.rs` | session-only | session-only ✓ | Already flagged. |
| `tests/golden_outputs.rs` (~6 tests) | investigate | investigate ✓ | Already flagged. |
| Additional Windows-only tests (if any beyond the 4) | program-durable | **session-only** | Any `#[cfg(windows)]` test that can't run on macOS CI is dead code. |

**Downgrades: ~4 new** (plus 4-6 already flagged = ~8-10 total)

---

## Revised Workspace Totals

| Crate | Program-durable (first pass) | Program-durable (revised) | Session-only (first pass) | Session-only (revised) | Delta |
|---|---:|---:|---:|---:|---:|
| tokenzero-core | 47 | **37** | 1 | **8** | -10 |
| tokenzero-filters | 14 | 14 | 1 | 1 | 0 |
| tokenzero-runtime | 28 | **25** | 10 | **15** | -3 |
| tokenzero-recovery | 38 | **32** | 3 | **9** | -6 |
| tokenzero-pulse | 31 | **29** | 0 | **2** | -2 |
| tokenzero-install | 129 | **121** | 13 | **21** | -8 |
| tokenzero-mcp | ~120 | **~115** | ~5 | **~10** | -5 |
| tokenzero (CLI) | ~175 | **~168** | 4 | **~8** | -7 |
| **Total** | **~582** | **~541** | **~37** | **~74** | **-41** |

**Remaining program-durable: ~541 tests (71% of 764).**  
**Total session-only: ~74 tests (10%).**  
**Investigate/helpers: ~49 (6%).**  
**Net reduction from first pass: ~41 tests reclassified.**

---

## Why the First Pass Was Too Inclusive

### 1. "Table-driven" ≠ "program-durable"

The first pass treated table-driven unit tests as durable because they "enumerate behavior." But `shell_policy/tables.rs` enumerates *internal function inputs and outputs* — if `command_succeeded()` gets refactored to take a struct instead of 5 parameters, every table row breaks without any observable behavior change. These are implementation mirrors, not contracts.

**Rule:** A test is program-durable only if it asserts behavior visible to downstream callers. Internal function tables are session-only scaffolding.

### 2. Toolchain renders are happy-path until proven otherwise

The first pass classified all 7 toolchain render tests as program-durable. But each test (cargo, pytest, npm, git clone) tests a single success case: "does the noise collapse correctly for this one tool?" This is happy-path testing. The *contract* (rendering collapses noisy output below raw tokens) is tested by `noisy_shell_output_compresses_below_raw_tokens` and `long_success_listing_is_collapsed_far_below_raw`. The toolchain-specific tests add marginal value.

**Rule:** N tests for the same contract across N tools are session-only unless each tool has a distinct failure class.

### 3. Optimization detail ≠ contract

Tests like `small_blob_stays_inline`, `persisted_cache_is_compact_json`, and `lock_file_is_stable_anchor_not_deleted_on_drop` assert *how* the implementation works, not *what* it guarantees. If blobs start going to sidecars at a lower threshold, `small_blob_stays_inline` breaks but the program still works. The contract (blob roundtrips byte-exactly) is tested elsewhere.

**Rule:** A test asserting an optimization threshold, formatting preference, or internal data structure layout is session-only.

### 4. Internal field assertions are fragile

`line_range_payloads_skip_source_fingerprint` and `virtual_paths_skip_source_fingerprint` both assert `.source_fingerprint.is_none()` on an internal struct field. If the implementation changes to store a sentinel value instead of `None`, these tests break without any behavior change. The *contract* is "virtual paths don't stale-check on expand" — but that's not what these tests assert.

**Rule:** Tests asserting struct fields rather than observable behavior are session-only.

### 5. Snapshot tests disguised as contracts

`doctor_capabilities_names_doctor_contract_subcommands` has 40+ assertions checking every field of a JSON blob. This is a snapshot test: if a new exit code is added, the test breaks but the program still works. The schema contract (doctor returns valid JSON with required fields) is tested by `doctor_reports_agent_contract_for_healthy_root`.

**Rule:** Tests that enumerate every variant of an extensible set are session-only snapshots unless the set is closed and contract-critical.

### 6. Platform-gated dead code

`#[cfg(windows)]` tests on macOS-only CI are dead code. They never run, never catch regressions, and add maintenance burden. If Windows CI is added later, these tests should be re-evaluated then — not shipped as program-durable now.

**Rule:** Tests gated behind a platform not in CI are session-only.

---

## Specific Tests to Downgrade

### From program-durable → session-only (41 tests)

**tokenzero-core (10):**
- `shell_policy/tables.rs::command_succeeded_table`
- `shell_policy/tables.rs::classify_command_status_table`
- `shell_policy/tables.rs::auto_shell_policy_table`
- `shell_policy/tables.rs::decide_shell_policy_table`
- `shell_policy/tables.rs::shell_family_table`
- `tests/misc.rs::mode_aliases_map_to_new_policy_names`
- `tests/misc.rs::token_count_is_nonzero_for_text`
- `tests/shell.rs::windows_shell_wrapped_search_commands_keep_search_summary`
- `tests/shell.rs::shell_c_wrappers_do_not_analyze_positional_args_as_code` (→ investigate)
- `tests/shell.rs::real_shell_operators_still_drive_status_warnings` (→ investigate)

**tokenzero-runtime (3):**
- `cmd_split_preserves_doubled_quotes_inside_quoted_arguments` (investigate → session-only)
- `powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments` (investigate → session-only)
- `windows_findstr_regex_metacharacters_inside_argv_stay_argv` (→ investigate)

**tokenzero-recovery (3):**
- `lock_file_is_stable_anchor_not_deleted_on_drop`
- `virtual_paths_skip_source_fingerprint`
- `line_range_payloads_skip_source_fingerprint` (if not already investigate)

**tokenzero-pulse (2):**
- `lock_file_is_stable_anchor_not_deleted_on_drop`
- `lock_wait_retries_platform_lock_contention_errors`

**tokenzero-install (3):**
- `doctor_capabilities_names_doctor_contract_subcommands`
- `apply_and_rollback_restore_temp_home` (→ investigate)
- `manifest_is_complete_for_every_written_file` (→ investigate)

**tokenzero-mcp (~5):**
- `bench_tests.rs::run_composition_benchmark`
- `bench_tests.rs::benchmark_harness_produces_consistent_results`
- `bench_tests.rs::all_workload_plans_execute_successfully`
- `bench_tests.rs::composition_never_worse_than_direct`
- (thin telemetry/legacy smoke — need exact names)

**tokenzero CLI (~4):**
- Additional Windows-only tests beyond the 4 already flagged

---

## Revised Minimal Plan Changes

The original minimal plan proposed pruning ~36 tests (~593 LOC). With the red-team additions:

**Additional prune candidates: ~41 tests, ~600 LOC** (estimated)

Key additions:
1. **Delete all 5 table tests in `shell_policy/tables.rs`** — replace with 1 integration test that exercises the full `decide_shell_policy` path through rendering
2. **Delete `doctor_capabilities_names_doctor_contract_subcommands`** — replace with a schema validation test that checks required keys exist, not every value
3. **Delete `lock_file_is_stable_anchor_not_deleted_on_drop`** in both recovery and pulse — the concurrency contract is tested by the concurrent-write tests
4. **Delete `mode_aliases_map_to_new_policy_names` and `token_count_is_nonzero_for_text`** — trivial sanity checks with no failure class
5. **Move `bench_tests.rs` to `benches/`** or delete entirely

---

## Conclusion

The first pass was correct about the *categories* of durability but too liberal about *which tests* qualify. The core issue: **testing an internal function's input/output table is not the same as testing a program contract.** The revised set of ~541 program-durable tests is still a strong suite — 71% of all tests — but the must-ship set is tighter and more honest about what's actually load-bearing vs what's scaffolding.
