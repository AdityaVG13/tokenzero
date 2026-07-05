# Minimal Verification Plan — TokenZero

## Audit finding

First pass: ~582 program-durable / ~764 tests (76%).  
Red-team re-pass: **~541 program-durable** / ~74 session-only / ~49 investigate (71% program-durable).

The suite is genuinely high-value. The plan is to tighten the ~41 over-classifications identified by red-team review and keep the durable core intact.

---

## Phase 1 — Prune session-only tests (~74 tests)

### Already identified in first pass (~33 tests)
- `tokenzero-filters`: `cat_rewrites_to_read`
- `tokenzero-runtime`: 10 overlapping / Windows-only / source-audit / trivial tests
- `tokenzero-recovery`: `persist_compacts_duplicate_order_entries`, `persisted_cache_is_compact_json`, `small_blob_stays_inline`
- `tokenzero-install`: 13 happy-path / Windows-only / duplicate CRC / advisory tests
- `tokenzero-mcp`: `bench_tests.rs` (4) + thin telemetry/legacy smoke
- `tokenzero` CLI: 4 Windows-only tests

### Added by red-team review (~41 tests)

#### tokenzero-core (10)
- `shell_policy/tables.rs::command_succeeded_table`
- `shell_policy/tables.rs::classify_command_status_table`
- `shell_policy/tables.rs::auto_shell_policy_table`
- `shell_policy/tables.rs::decide_shell_policy_table`
- `shell_policy/tables.rs::shell_family_table`
- `tests/misc.rs::mode_aliases_map_to_new_policy_names`
- `tests/misc.rs::token_count_is_nonzero_for_text`
- `tests/shell.rs::windows_shell_wrapped_search_commands_keep_search_summary`
- `tests/shell.rs::shell_c_wrappers_do_not_analyze_positional_args_as_code` (downgrade to session-only or investigate)
- `tests/shell.rs::real_shell_operators_still_drive_status_warnings` (downgrade to session-only or investigate)

#### tokenzero-runtime (3)
- `cmd_split_preserves_doubled_quotes_inside_quoted_arguments`
- `powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments`
- `windows_findstr_regex_metacharacters_inside_argv_stay_argv` (move to investigate)

#### tokenzero-recovery (3)
- `lock_file_is_stable_anchor_not_deleted_on_drop`
- `virtual_paths_skip_source_fingerprint`
- `line_range_payloads_skip_source_fingerprint` (downgrade from investigate)

#### tokenzero-pulse (2)
- `lock_file_is_stable_anchor_not_deleted_on_drop`
- `lock_wait_retries_platform_lock_contention_errors`

#### tokenzero-install (3)
- `doctor_capabilities_names_doctor_contract_subcommands`
- `apply_and_rollback_restore_temp_home` (move to investigate)
- `manifest_is_complete_for_every_written_file` (move to investigate)

#### tokenzero-mcp (~5)
- `bench_tests.rs` — already listed; plus thin telemetry/legacy smokes

#### tokenzero CLI (~4)
- Additional Windows-only tests beyond the 4 already flagged

---

## Phase 2 — Strengthen weak oracles

1. `tokenzero-filters::discovers_launch_critical_families` → structural assertions, drop hardcoded list.
2. `tokenzero-runtime` proptest → add posix platform; explicit killed assertion.
3. `tokenzero-recovery` proptests → assert `file_ref` roundtrip and exact selector content.
4. `tokenzero-core::decide_shell_policy_table` → if kept, expand to all Mode variants; better: replace 5 table tests with 1 integration test through rendering.
5. `tokenzero-install` → move 2 misplaced tests between `tar.rs` and `zip.rs`.
6. `tokenzero-install::doctor_capabilities_names_doctor_contract_subcommands` → replace with schema-validation test checking required keys exist.
7. `tokenzero` CLI → verify `golden_outputs.rs` JSON uniqueness; add `single_quote` proptest.

---

## Phase 3 — Acceptance gates

1. Run `cargo test --workspace --locked --no-fail-fast` → green.
2. Run `cargo clippy --workspace --all-targets --locked -- -D warnings` → green.
3. Fix `cargo fmt --all -- --check` diff in `crates/tokenzero-mcp/src/codemode/tests.rs`.
4. Move 3 oversized inline test mods to sibling `tests.rs` modules.
5. Confirm every pruned test is covered by a stronger program-durable test or has no downstream failure class.

---

## Phase 4 — Long-term steering

- Add a PR template checkbox: "Every new test names the failure class it protects."
- Move benchmark harnesses into `benches/`, not `src/**/tests*`.
- Rely on `#![deny(unsafe_code)]` as the single unsafe-code gate.
- Gate platform-only tests with `#[cfg(...)]` and do not count them as program-durable unless the platform is in CI.
