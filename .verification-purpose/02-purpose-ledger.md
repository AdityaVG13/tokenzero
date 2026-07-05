# Purpose Ledger — TokenZero Test Suite

Target: `/Users/aditya/AI/TokenZero`  
Mode: `audit-existing`  
Date: 2026-07-05

---

## Ledger by Crate

### tokenzero-core (≈55 tests)

| File / Group | Purpose | Rationale / Failure class | Decision |
|---|---|---|---|
| `src/tests/capsule.rs` | program-durable | Token budgets, exact-mode hiding, recovery-ref naming, framing-overhead invariants. Public API contracts. | keep |
| `src/tests/misc.rs` | program-durable | Critical-line preservation, inventory/search view safety, secret masking, diagnostic gap transparency, toolchain family classification. | keep |
| `src/tests/render_util.rs` | program-durable | Long-path edge, line/symbol selectors, legacy mode alias compatibility. | keep |
| `src/tests/repeat_render.rs` | program-durable | Repeat-render compaction, failure/first-run exclusions, compression floor. | keep |
| `src/tests/shell.rs` (22 tests) | program-durable | Shell policy selection, status truth, pipeline masking, wrapper detection, no-match semantics, cross-platform quoting, compression ratios. | keep |
| `src/tests/toolchain.rs` | program-durable | Toolchain-specific noise compaction (cargo, pytest, npm, git clone) preserving warnings/summary. | keep |
| `src/shell_policy/tables.rs` (5 table tests) | program-durable | Unit-level regression tables for `command_succeeded`, `classify_command_status`, `auto_shell_policy`, `decide_shell_policy`, `shell_family`. Some overlap with integration tests but table form is valuable for enumeration. | keep; expand `decide_shell_policy_table` |
| `tests/shell_semantics.rs` | program-durable | Integration-level predicate false-positive prevention, OR/AND list semantics, masking warnings, env-chdir behavior. Strong downstream signal. | keep |
| `src/tests/support.rs` | session-only | Test helper `success_input()`, not a test. Infrastructure. | keep as helper (not shipped) |

**tokenzero-core counts:** program-durable 47, session-only 1 (helper), investigate 6 (overlap notes).

---

### tokenzero-filters (16 tests)

| Test / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `discovers_launch_critical_families` | investigate | Program-durable surface but hardcoded family list couples to internal data. | strengthen: structural assertions + weak major-family check |
| `cat_rewrites_to_read` | session-only | Single happy-path; overlapped by broader read-only/compound/quoted-operator tests. | prune |
| `destructive_commands_are_unmodified` | program-durable | Safety boundary: destructive commands never rewritten. | keep |
| `compound_commands_are_left_unmodified` | program-durable | Safety: pipelines/sequences/embedded control chars never rewritten. | keep |
| `command_substitution_counts_as_compound` | program-durable | Injection safety: `$()`, backtick, arithmetic substitution. | keep |
| `dispatchers_and_remote_execution_are_never_vouched` | program-durable | Safety: xargs/eval/sudo/ssh/etc. never vouched. | keep |
| `expanded_destructive_commands_are_flagged` | program-durable | 22+ destructive commands across 8 categories. Primary `unsafe_reason` regression net. | keep |
| `unknown_families_are_not_vouched` | program-durable | Unrecognized commands default to unsafe. | keep |
| `disabled_mode_reports_honest_safety` | program-durable | Disabled mode still reports honest safety. | keep |
| `read_only_finds_and_passthroughs_stay_vouched` | program-durable | Read-only commands vouched safe. | keep |
| `backslash_escaped_quotes_split_correctly` | program-durable | Quote/operator parsing correctness. | keep |
| `quiet_flags_injected_for_noisy_toolchains` | program-durable | Quiet-flag injection contract for cargo/git/npm. | keep |
| `bounded_rewrites_respect_existing_limits` | program-durable | Idempotency: existing limits preserved. | keep |
| `quiet_injection_respects_explicit_verbosity_and_passthrough_separators` | program-durable | Verbosity/separator/passthrough handling. | keep |
| `quiet_injection_never_touches_mutations_or_compounds` | program-durable | Quiet injection blocked for destructive/compound. | keep |
| `quoted_operators_do_not_count_as_compound` | program-durable | Quoted operators don't false-positive compound detection. | keep |

**tokenzero-filters counts:** program-durable 14, session-only 1, investigate 1.

---

### tokenzero-runtime (40 tests)

| Test / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `simple_command_plans_as_argv_without_alias` | program-durable | Baseline Argv routing, no alias dependency. | keep |
| `shell_metacharacters_inside_argv_args_do_not_force_shell` | program-durable | Pre-split argv metacharacters don't trigger shell. | keep |
| `windows_findstr_regex_metacharacters_inside_argv_stay_argv` | program-durable | Windows argv display formatting. | keep |
| `shell_syntax_uses_real_shell_not_alias` | program-durable | Shell syntax routes to real `/bin/sh` or `cmd`. | keep |
| `multi_arg_shell_operators_use_real_shell` | program-durable | `&&` joins into shell argv correctly. | keep |
| `multi_arg_shell_operators_quote_literal_arguments_in_plan` | program-durable | Metacharacter quoting in shell plan. | keep |
| `generated_multi_arg_shell_literal_metacharacters_stay_data` | session-only | Integration happy-path; overlaps plan tests. | prune |
| `quoted_operator_literals_stay_argv` | program-durable | Quoted operators don't trigger shell. | keep |
| `double_quoted_backslash_stays_literal_before_ordinary_chars` | program-durable | POSIX quoting edge. | keep |
| `variable_and_tilde_expansion_route_through_shell` | program-durable | Expansion routing boundaries. | keep |
| `leading_posix_env_assignment_uses_shell` | program-durable | `VAR=val cmd` routes through shell. | keep |
| `windows_split_preserves_path_backslashes` | program-durable | Windows path backslashes preserved. | keep |
| `cmd_split_treats_single_quotes_as_literal_characters` | program-durable | cmd.exe single-quote behavior. | keep |
| `powershell_split_uses_single_quotes_for_literal_arguments` | program-durable | PowerShell single-quote grouping. | keep |
| `cmd_split_preserves_doubled_quotes_inside_quoted_arguments` | investigate | Thin cmd escape edge; may be covered by proptest. | keep if proptest misses `"` inside quotes |
| `powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments` | investigate | Thin PowerShell `''` escape edge. | keep if proptest misses it |
| `split_preserves_empty_quoted_arguments` | program-durable | Empty quoted args not dropped. | keep |
| `generated_split_roundtrips_displayed_cmd_and_powershell_args` (proptest) | program-durable | Roundtrip property for cmd/powershell. | keep; add posix platform |
| `run_command_preserves_multi_arg_shell_operators` | session-only | Runtime echo of plan test. | prune |
| `stream_capture_spills_and_truncates_large_output` | program-durable | Capture/truncation/spill contract. | keep |
| `stream_capture_spill_is_not_double_counted_on_large_first_read` | program-durable | No double-counting on large first read. | keep |
| `run_command_caps_large_stdout_with_metadata` | session-only | E2E overlap with unit capture test. | prune |
| `timeout_kills_child_while_large_stdin_write_is_blocked` | program-durable | Timeout during blocked stdin write. | keep |
| `background_descendant_holding_stdio_is_cleaned_without_false_timeout` | program-durable | Process-group cleanup, no false timeout. | keep |
| `fast_command_with_background_child_returns_promptly_without_timeout` | program-durable | Foreground exit doesn't wait for background child. | keep |
| `unsafe_escape_hatchet_stays_single_macos_allocator_shim` | session-only | Source-level `unsafe` count audit; `#![deny(unsafe_code)]` is durable gate. | prune |
| `windows_shell_plan_uses_cmd_without_alias_dependency` | session-only | Overlaps shell routing tests. | prune |
| `windows_powershell_script_plan_uses_powershell` | program-durable | PowerShell syntax detection → powershell routing. | keep |
| `explicit_powershell_invocation_stays_argv` | program-durable | Explicit `powershell -Command` stays Argv. | keep |
| `posix_shell_plan_uses_non_login_sh` | session-only | Subset of multi-arg shell test. | prune |
| `simple_windows_command_stays_explicit_argv` | session-only | Windows happy-path variant of test #1. | prune |
| `windows_run_resolves_pathext_cmd_shims` | session-only | Windows-only PATH resolution; OS-level behavior. | prune |
| `windows_run_powershell_variable_script` | session-only | Windows-only happy-path; plan test sufficient. | prune |
| `windows_builtin_echo_uses_cmd` | program-durable | Windows built-in `echo` → cmd routing. | keep |
| `env_i_style_invocation_works` | session-only | Trivial happy-path echo. | prune |
| `quoting_preserves_spaces` | program-durable | Cross-platform quoter behavior. | keep |
| `spill_prune_reclaims_expired_and_keeps_fresh_and_foreign_files` | program-durable | TTL eviction + foreign-file safety. | keep |
| `spill_prune_byte_ceiling_evicts_oldest_first` | program-durable | Oldest-first byte-ceiling eviction. | keep |
| `spill_prune_dry_run_counts_without_unlinking` | program-durable | Dry-run counts without deleting. | keep |
| `spill_prune_missing_dir_is_empty_report` | program-durable | Missing dir fail-open. | keep |

**tokenzero-runtime counts:** program-durable 28, session-only 10, investigate 2.

---

### tokenzero-recovery (46 tests)

| Test / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `restart_expand_is_byte_exact` | program-durable | Blob ref survives restart byte-exactly. | keep |
| `deferred_payloads_persist_in_one_batch` | program-durable | Deferred-batch persistence contract. | keep |
| `shell_outcome_repeat_detection_tracks_content_and_exit_code` | program-durable | Dedup correctness boundaries. | keep |
| `shell_outcomes_are_bounded_and_evict_oldest` | program-durable | Capacity + FIFO eviction. | keep |
| `shell_outcomes_survive_persistence_roundtrip` | program-durable | Dedup state survives restart. | keep |
| `persist_lock_file_is_stable_anchor_not_deleted_on_drop` | program-durable | Lock anchor stability. | keep |
| `concurrent_persistence_preserves_all_thread_payloads` | program-durable | 8-thread concurrency correctness. | keep |
| `alternating_writers_on_one_cache_path_still_merge` | program-durable | Multi-writer merge. | keep |
| `single_writer_repeat_persists_skip_reload_and_stay_byte_exact` | program-durable | Disk identity persistence + byte-exact expansion. | keep |
| `load_state_rejects_reader_growth_past_max_load_bytes` | program-durable | Oversized cache rejected. | keep |
| `load_state_ignores_invalid_utf8_cache` | program-durable | Corrupt UTF-8 graceful miss. | keep |
| `recovery_tmp_paths_are_unique_within_process` | program-durable | Tmp path uniqueness. | keep |
| `atomic_write_json_does_not_reuse_stale_temp_path` | program-durable | Stale temp file safety. | keep |
| `evict_prefix_removes_fifo_victims_once` | program-durable | FIFO eviction algorithm. | keep |
| `evict_prefix_falls_back_to_key_order_without_order_entries` | program-durable | BTreeMap fallback eviction. | keep |
| `file_ref_reports_stale_after_source_changes` | program-durable | Staleness contract. | keep |
| `stale_check_uses_native_path_identity_before_display_path` | program-durable | Native path identity for stale check. | keep |
| `virtual_paths_skip_source_fingerprint` | program-durable | Virtual paths not fingerprint-checked. | keep |
| `file_refs_distinguish_non_utf8_path_bytes` (unix) | program-durable | Non-UTF-8 path distinctness. | keep |
| `native_path_identity_round_trips_non_utf8_path_bytes` (unix) | program-durable | Lossless path identity encoding. | keep |
| `recovery_sidecar_paths_preserve_non_utf8_file_name_bytes` (unix) | program-durable | Sidecar paths preserve non-UTF-8 bytes. | keep |
| `range_fragment_selects_lines` | program-durable | Line-range selection. | keep |
| `around_selector_saturates_huge_line_and_radius` | program-durable | Panic safety at usize::MAX. | keep |
| `expand_preserves_non_newline_terminated_content` | program-durable | Regression: no trailing newline added. | keep |
| `slice_preserves_trailing_blank_line` | program-durable | Regression: blank-line preservation. | keep |
| `eviction_bounds_blob_count` | program-durable | max_blobs capacity. | keep |
| `file_refs_expand_after_their_blob_is_evicted` | program-durable | Ref independence after eviction. | keep |
| `batched_deferred_payloads_enforce_limits_on_persist_pending` | program-durable | Capacity enforcement at persist time. | keep |
| `batched_deferred_payloads_match_immediate_final_live_refs` | program-durable | Batched vs immediate equivalence. | keep |
| `deferred_search_output_enforces_limits_before_returning` | program-durable | max_search_hits enforcement. | keep |
| `arbitrary_payload_roundtrips` (proptest) | program-durable | Any payload roundtrips byte-exactly. | keep; strengthen with file_ref |
| `generated_around_selectors_do_not_panic` (proptest) | program-durable | Arbitrary selectors never panic. | keep; assert exact content |
| `tmp_sweep_reclaims_stale_orphans_of_both_shapes_only` | program-durable | Stale orphan cleanup. | keep |
| `tmp_sweep_dry_run_counts_without_unlinking` | program-durable | Dry-run sweep. | keep |
| `tmp_sweep_missing_dir_is_empty_report` | program-durable | Missing-dir fail-open. | keep |
| `second_process_persist_appends_journal_without_snapshot_rewrite` | program-durable | Multi-process journal append. | keep |
| `foreign_journal_append_forces_merge_and_nothing_is_lost` | program-durable | Foreign journal merge correctness. | keep |
| `corrupt_journal_tail_keeps_complete_entries` | program-durable | Torn journal tail doesn't corrupt complete entries. | keep |
| `big_blob_externalizes_to_sidecar_and_roundtrips` | program-durable | Large-blob sidecar contract. | keep |
| `corrupt_blob_sidecar_is_a_miss_not_bad_bytes` | program-durable | Tampered sidecar returns miss. | keep |
| `oversized_journal_compacts_into_fresh_snapshot` | program-durable | Journal compaction. | keep |
| `persist_compacts_duplicate_order_entries` | session-only | Internal dedup strategy detail; public contract covered by restart tests. | prune |
| `persisted_cache_is_compact_json` | session-only | Pretty-printed JSON is equally correct; no failure class. | prune |
| `small_blob_stays_inline` | session-only | Optimization detail; expand correctness covered elsewhere. | prune |
| `line_range_payloads_skip_source_fingerprint` | investigate | Asserts internal field; rewrite to observable stale-check behavior. | strengthen |
| `deferred_payload_enforces_limits_before_returning` | investigate | Distinct timing from batched variant but overlaps failure class. | keep |

**tokenzero-recovery counts:** program-durable 38, session-only 3, investigate 5.

---

### tokenzero-pulse (31 tests)

All 31 tests classified program-durable. Key failure classes:
- Raw-payload privacy (#1)
- Overflow safety / saturating arithmetic (#2, #3)
- Concurrency / O_APPEND atomicity, lock contention, lock anchor (#4, #27–29)
- Corruption resilience / torn lines / SQLite rebuild / schema migration (#5, #14, #18–21, #30)
- Schema enforcement / version gating / proptest panic safety (#6–#9)
- Import/export byte fidelity / temporal ordering / marker integrity (#12–#18, #23–#26)
- Platform correctness: non-UTF-8 paths, permission errors (#11, #22)

**tokenzero-pulse counts:** program-durable 31, session-only 0, investigate 0.

---

### tokenzero-install (144 tests)

| File / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `src/tests.rs` — plan/read-only/schema | program-durable | Public `plan()` doesn't mutate FS. | keep |
| `src/tests.rs` — doctor healthy root | program-durable | Doctor JSON schema contract. | keep |
| `src/tests.rs` — doctor missing root | program-durable | Missing root blocks with `tz-root-missing`. | keep |
| `src/tests.rs` — doctor cache parent info | program-durable | Severity contract (info not error). | keep |
| `src/tests.rs` — doctor fix dry-run | program-durable | Dry-run without mutation. | keep |
| `src/tests.rs` — doctor fix/undo lifecycle | program-durable | Full fix → idempotency → undo. | keep |
| `src/tests.rs` — `doctor_ls_lists_run_artifacts_for_agents` | session-only | Happy-path listing; covered by fix/undo. | prune |
| `src/tests.rs` — doctor lock exit 5 | program-durable | Concurrency contract. | keep |
| `src/tests.rs` — doctor undo non-empty | program-durable | Safety: undo refuses non-empty dir. | keep |
| `src/tests.rs` — doctor capabilities | program-durable | JSON schema for agents/CI. | keep |
| `src/tests.rs` — `doctor_explain_returns_known_finding_when_not_current` | session-only | Happy-path on clean root. | prune |
| `src/tests.rs` — `doctor_robot_triage_is_single_call_json_contract` | session-only | Overlaps missing-root + capabilities. | prune |
| `src/tests.rs` — doctor triage plans fix | program-durable | Triage recommends `--fix`. | keep |
| `src/tests.rs` — `doctor_robot_docs_describe_negative_space` | session-only | String-contains on docs; no contract. | prune |
| `src/tests.rs` — apply/rollback temp home | program-durable | Core install lifecycle. | keep |
| `src/tests.rs` — global MCP plan | program-durable | Plan lists AI clients + launcher. | keep |
| `src/tests.rs` — global MCP grok-only | program-durable | Agent-scoped plan. | keep |
| `src/tests.rs` — global JSON MCP merge | program-durable | Preserves existing servers. | keep |
| `src/tests.rs` — global TOML MCP merge | program-durable | Replace once, preserve foreign, idempotent. | keep |
| `src/tests.rs` — client surface rejects stale | program-durable | Rejects stale `/bin/false` even with tokenzero name. | keep |
| `src/tests.rs` — `surface_install_always_writes_classic` | session-only | Env var serialization detail. | prune |
| `src/tests.rs` — client surface accepts applied configs | program-durable | Inspection lifecycle. | keep |
| `src/tests.rs` — 3 Windows-only tests (`windows_mcp_config_*`, `global_cli_and_shell_wrappers_are_cmd_files_on_windows`, `windows_path_repair_*`) | session-only | `#[cfg(windows)]` dead code on macOS CI / implementation detail. | prune or gate `#[cfg(windows)]` |
| `src/tests.rs` — global CLI runtime copy rollback | program-durable | Binary install lifecycle with rollback. | keep |
| `src/tests.rs` — wrappers executable / shebang | program-durable | Unix wrapper correctness. | keep |
| `src/tests.rs` — atomic write clean | program-durable | Crash-safety: no tmp debris. | keep |
| `src/tests.rs` — manifest completeness | program-durable | Rollback integrity. | keep |
| `src/tests.rs` — hooks plan scoped | program-durable | Agent-scoped hooks. | keep |
| `src/tests.rs` — hooks merge foreign/idempotent | program-durable | Preserves user hooks. | keep |
| `src/tests.rs` — hooks merge rejects invalid | program-durable | Don't corrupt broken files. | keep |
| `src/tests.rs` — hooks surface inspection | program-durable | Hooks inspection lifecycle. | keep |
| `src/tests.rs` — shim plan resolvable | program-durable | Only plan shims for PATH-resolvable tools. | keep |
| `src/tests.rs` — shim resolution skips decoys | program-durable | Security: stale/decoy shims not picked as REAL. | keep |
| `src/tests.rs` — shim apply/rollback | program-durable | Full shim lifecycle. | keep |
| `src/tests.rs` — shim fall-through | program-durable | Missing launcher fails open to `$REAL`. | keep |
| `src/tests.rs` — `detect_present_agents_probes_home_and_path` | session-only | Advisory heuristics, happy-path. | prune |
| `src/tests.rs` — shim surface inspection | program-durable | Shim inspection lifecycle. | keep |
| `src/tests.rs` — shim passthrough exit codes | program-durable | Exit codes match real grep. | keep |
| `src/package_audit/tests/general.rs` (9 tests) | program-durable | Release gate: external runtime, dev launcher, private members, local generated members, control chars, non-UTF-8 tar/zip names/link targets. | keep |
| `src/package_audit/tests/tar.rs` (38 + 1 investigate) | program-durable | PAX/GNU overrides, tar header parsing, link target escapes, path traversal, nested archive recursion. | keep; move misplaced zip test to zip.rs |
| `src/package_audit/tests/zip.rs` (52 + 3 session-only + 1 investigate) | program-durable | zip64 ambiguity, CRC tampering, executable payloads, path traversal, symlinks, control chars. | keep; prune 3 duplicate CRC-mechanism tests; move misplaced tar test |
| `fixtures/` (533 LOC) | session-only | Test builders / helpers. Not shipped. | keep as infrastructure |

**tokenzero-install counts:** program-durable 129, session-only 13, investigate 2 (file misplacement).

---

### tokenzero-mcp (≈252 tests)

| File / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `src/codemode/bench_tests.rs` (4 tests) | session-only | Composition benchmark harness internals: run benchmark, consistency, all workloads execute, composition advantage. Used for dev-loop/benchmark scripts, not shipped program contract. | prune or move to `benches/` |
| `src/codemode/e2e_tests.rs` (11 tests) | program-durable | CodeMode execution contracts: durable/logical refs, JSON DAG, QuickJS sandbox, microtask cap, capability denials, batch telemetry, output guard. | keep |
| `src/codemode/audit_tests.rs` | program-durable | CodeMode audit/safety checks. | keep |
| `src/codemode/tests.rs` / `src/codemode/mod.rs` tests | program-durable | Plan parsing, engine creation, tool routing. | keep |
| `src/tests/edit.rs` | program-durable | Edit tool contracts (hunks, JSON args, string booleans). | keep |
| `src/tests/expand.rs` | program-durable | Expand ref contracts, stale behavior, line ranges. | keep |
| `src/tests/fetch.rs` | program-durable | Fetch guard, URL policies, SSRF protection. | keep |
| `src/tests/jsonrpc.rs` | program-durable | JSON-RPC envelope parsing/errors. | keep |
| `src/tests/misc.rs` | program-durable | Supervisor, tools, general MCP behavior. | keep |
| `src/tests/read.rs` | program-durable | Read tool contracts (allowed roots, ranges, refs). | keep |
| `src/tests/search.rs` | program-durable | Search tool contracts (rg/internal backends, dedup, zero-hit notes). | keep |
| `src/tests/session.rs` | program-durable | Session consistency, dedup, degradation, diff fallback. | keep |
| `src/tests/shell.rs` | program-durable | Shell tool contracts (env overrides, truncation, allowed roots). | keep |
| `src/recall/tests.rs` | program-durable | Recall search stored payloads. | keep |
| `src/stdio/tests.rs` | program-durable | Stdio transport framing. | keep |
| `src/supervisor/tests.rs` | program-durable | Supervisor crash recovery, lifecycle. | keep |
| `src/tools/tests.rs` | program-durable | Tool arg parsing, batch, schema coercion, windows argv display. | keep |
| `tests/jsonrpc_conformance.rs` (13 tests) | program-durable | MCP initialize/JSON-RPC conformance matrix, recall E2E, edit E2E, tool list filtering. | keep |

**Strong oracles identified:** sandbox capability denial (8 capabilities), proptest parser totality, supervisor crash recovery, symlink/parent traversal/prefix attack prevention, cache deny-policy bypass, SSRF protection, dangling ref prevention, JSON-RPC conformance matrix, MCP initialize negotiation, concurrent dedup race.

**tokenzero-mcp counts:** program-durable ≈120, session-only ≈5 (bench tests + thin telemetry/legacy smoke), investigate 0.

---

### tokenzero (CLI) (≈180 tests)

| File / Group | Purpose | Rationale | Decision |
|---|---|---|---|
| `src/tests.rs` — 3 Windows-only env/display tests | session-only | Can't run on macOS CI / Windows-only happy path. | prune or `#[cfg(windows)]` |
| `src/tests.rs` — `unix_global_home_uses_home` | program-durable | HOME resolution on actual CI platform. | keep |
| `src/tests.rs` — allowed-roots tests (2) | program-durable | Path sandboxing / root isolation. | keep |
| `src/hook/tests.rs` (17 tests) | program-durable | Hook rewrite contracts, skip-list, NO_WRAP, guide mode, read guard, session restore. | keep |
| `src/reach/tests.rs` (4 tests) | program-durable | Reach report schema, wrapper audit, path comparison, import isolation. | keep |
| `src/release_claims/tests.rs` (2 tests) | program-durable | Gate summary, benchmark claim exact-expand checks. | keep |
| `src/audits/bench.rs` — `aggregate_tracks_gates_independently` | program-durable | Gates tracked independently. | keep |
| `src/claim_actions.rs` — import isolation | program-durable | Import isolation guard. | keep |
| `src/artifact_contracts.rs` — import isolation | program-durable | Import isolation guard. | keep |
| `src/completion_handoff.rs` — import isolation | program-durable | Import isolation guard. | keep |
| `src/cli_args.rs` (3 tests) | program-durable | Plan flag/file priority. | keep |
| `src/zerostack_store.rs` (6 tests) | program-durable | Legacy/unified recovery paths, codemode cache, allowed-roots dedup. | keep |
| `tests/cli_help_contract.rs` (4 tests) | program-durable | Help output is public API for agents. | keep |
| `tests/cli_doctor.rs` (11 tests) | program-durable | Doctor JSON schema, lifecycle, triage. | keep |
| `tests/cli_run_shell.rs` (≈15 tests) | program-durable | `tokenzero run` JSON envelope contract. | keep |
| `tests/cli_tools_io.rs` (≈10 tests) | program-durable | Read→expand roundtrip, adapter approval template, tool I/O. | keep |
| `tests/cli_artifact_handoff.rs` (5 tests) | program-durable | Handoff packet, recovery/anchor audits, explain-runtime. | keep |
| `tests/cli_adapter_approval.rs` (6 tests) | program-durable | Blocking, templates, malformed rejection, approval, side-effect rejection, duplicate rejection. | keep |
| `tests/cli_install_clients.rs` (7 tests) | program-durable | Grok install, hooks/shims scoping, detect/plan/doctor, broken TOML, roundtrip, rollback, scan. | keep |
| `tests/golden_outputs.rs` (≈6 tests) | investigate | Golden JSON snapshots may overlap with behavioral tests. Verify uniqueness before pruning. | investigate |
| `tests/mcp_transport.rs` (3 tests) | program-durable | MCP resilience to malformed JSON, mixed framing, initialize. | keep |
| `tests/passthrough_conformance.rs` (≈24 tests) | program-durable | Hook binary E2E: exit parity, chains, quoting hells, large output, stderr, skip-list, fail-open, NO_WRAP, modes. Highest-value group. | keep |
| `tests/cli_reach_os.rs` (6 tests) | program-durable | Reach report, PATH shadow, OS claims, artifact merge, RC mismatch, artifact generation. | keep |
| `tests/cli_release_claim_audits/` (≈25 tests across 7 files) | program-durable | Adapter approval, benchmark claims, eval gate, completion audit, source currency, artifact pinning. | keep |

**tokenzero counts:** program-durable ≈175, session-only 4, investigate 2.

---

## Workspace Totals

| Crate | Program-durable | Session-only | Investigate | Total tests |
|---|---:|---:|---:|---:|
| tokenzero-core | 47 | 1 (helper) | 6 | ~55 |
| tokenzero-filters | 14 | 1 | 1 | 16 |
| tokenzero-runtime | 28 | 10 | 2 | 40 |
| tokenzero-recovery | 38 | 3 | 5 | 46 |
| tokenzero-pulse | 31 | 0 | 0 | 31 |
| tokenzero-install | 129 | 13 | 2 | 144 |
| tokenzero-mcp | ~120 | ~5 | 0 | ~252 |
| tokenzero (CLI) | ~175 | 4 | 2 | ~180 |
| **Workspace total** | **~582** | **~37** | **18** | **~764** |

*Counts are per test function where available; some CLI/MCP numbers are group estimates based on file-level classification and test listings.*

---

## Prune Candidates Summary

| Crate | Tests | Approx LOC | Reason |
|---|---:|---:|---|
| tokenzero-filters | `cat_rewrites_to_read` | ~8 | Happy-path overlap |
| tokenzero-runtime | 10 tests (see table) | ~180 | Overlap, Windows-only, source audit, trivial happy-path |
| tokenzero-recovery | 3 tests | ~25 | Implementation-mirroring / formatting detail |
| tokenzero-install | 13 tests | ~200 | Happy-path listing/explain/triage/docs, env serialization, Windows-only dead code, advisory heuristics, duplicate CRC |
| tokenzero-mcp | `bench_tests.rs` (4) + thin telemetry/legacy smoke (~1) | ~120 | Benchmark harness internals, not program contract |
| tokenzero (CLI) | 4 Windows-only tests | ~60 | Cannot run on macOS CI / platform-only happy path |
| **Total prune candidates** | **~36 tests** | **~593 LOC** | — |


---


---

## Red-Team Re-Pass

An adversarial review of `02-purpose-ledger.md` was performed (see
`04-red-team-review.md`). It found the first pass too generous by **~41 tests**.

### Revised totals

| Crate | Program-durable | Session-only | Investigate | Total |
|---|---:|---:|---:|---:|
| tokenzero-core | 37 | 8 | 10 | ~55 |
| tokenzero-filters | 14 | 1 | 1 | 16 |
| tokenzero-runtime | 25 | 15 | 0 | 40 |
| tokenzero-recovery | 32 | 9 | 5 | 46 |
| tokenzero-pulse | 29 | 2 | 0 | 31 |
| tokenzero-install | 121 | 21 | 2 | 144 |
| tokenzero-mcp | ~115 | ~10 | 0 | ~252 |
| tokenzero (CLI) | ~168 | ~8 | 4 | ~180 |
| **Workspace** | **~541** | **~74** | **~49** | **~764** |

### Key downgrades from program-durable to session-only

**tokenzero-core**
- `shell_policy/tables.rs` — all 5 table tests (mirror internal functions, not contracts)
- `tests/misc.rs::mode_aliases_map_to_new_policy_names` (trivial string parsing)
- `tests/misc.rs::token_count_is_nonzero_for_text` (no boundary/value contract)
- `tests/shell.rs::windows_shell_wrapped_search_commands_keep_search_summary` (overlapped)

**tokenzero-runtime**
- `cmd_split_preserves_doubled_quotes_inside_quoted_arguments`
- `powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments`
- `windows_findstr_regex_metacharacters_inside_argv_stay_argv` (→ investigate)

**tokenzero-recovery**
- `lock_file_is_stable_anchor_not_deleted_on_drop` (concurrency covered elsewhere)
- `virtual_paths_skip_source_fingerprint` (asserts internal field)
- `line_range_payloads_skip_source_fingerprint` (asserts internal field)

**tokenzero-pulse**
- `lock_file_is_stable_anchor_not_deleted_on_drop`
- `lock_wait_retries_platform_lock_contention_errors` (error-classification unit test)

**tokenzero-install**
- `doctor_capabilities_names_doctor_contract_subcommands` (snapshot test)
- `apply_and_rollback_restore_temp_home` (→ investigate)
- `manifest_is_complete_for_every_written_file` (→ investigate)

**tokenzero-mcp**
- `bench_tests.rs` (4 tests) + thin telemetry/legacy smokes

**tokenzero CLI**
- Additional Windows-only tests beyond the 4 already flagged

### Why the first pass was too inclusive

1. **Table-driven ≠ program-durable** — internal function tables mirror signatures, not observable contracts.
2. **Toolchain renders** — one test per tool is happy-path unless each tool has a distinct failure class.
3. **Optimization/formatting detail** — asserting compact JSON or inline-vs-sidecar thresholds is implementation detail.
4. **Internal field assertions** — checking `.source_fingerprint.is_none()` breaks on refactors that preserve behavior.
5. **Snapshot tests** — 40+ assertion JSON blobs break when extensible sets grow.
6. **Platform-gated dead code** — `#[cfg(windows)]` tests on macOS CI are not program-durable.

The red-team conclusion: **~541 tests (71%) are the honest program-durable core**; the rest is scaffolding or overlap.
