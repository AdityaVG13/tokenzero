# Verification Purpose Report — TokenZero Test Suite

## What was done

Ran `verification-purpose-gate` in `audit-existing` mode against the full TokenZero Rust workspace, then ran a red-team adversarial review of the classifications.
- Target root: `/Users/aditya/AI/TokenZero`
- Scope: all Rust verification artifacts across 8 crates
- Method: per-crate read-only audit by subagent, synthesis by parent, red-team challenge, fresh evidence commands run
- Red-team review: `.verification-purpose/04-red-team-review.md`

Artifacts produced in `.verification-purpose/`:
- `00-target-root.txt`
- `01-intent-statement.md`
- `02-purpose-ledger.md`
- `03-minimal-plan.md`
- `04-steering-snippet.md`
- `04-red-team-review.md`
- `05-program-value-proof.md`
- `06-purpose-report.md`

## First-pass findings

| Metric | Value |
|---|---:|
| Total tests classified | ~764 |
| Program-durable | ~582 (≈76%) |
| Session-only (prune candidates) | ~37 (≈5%) |
| Investigate / borderline | 18 (≈2%) |

## Red-team revised findings

| Metric | Value |
|---|---:|
| Program-durable | **~541 (≈71%)** |
| Session-only | **~74 (≈10%)** |
| Investigate / helpers | **~49 (≈6%)** |

The red-team found the first pass too generous by **~41 tests**. Main categories of over-classification:
1. Internal table-driven unit tests that mirror function signatures, not contracts.
2. Toolchain-specific happy-path renders with no distinct failure class.
3. Optimization/formatting detail assertions.
4. Internal field assertions rather than observable behavior.
5. Snapshot tests disguised as contract tests.
6. Platform-gated dead code on macOS-only CI.
7. Benchmark harness tests living in `src/`.

## Top prune targets added by red-team

- `tokenzero-core/src/shell_policy/tables.rs` — all 5 table tests; replace with 1 integration test through rendering.
- `tokenzero-core/tests/misc.rs::mode_aliases_map_to_new_policy_names` and `token_count_is_nonzero_for_text` — trivial sanity checks.
- `tokenzero-core/tests/shell.rs::windows_shell_wrapped_search_commands_keep_search_summary` — overlapped by bash/direct tests.
- `tokenzero-recovery` and `tokenzero-pulse` lock-anchor tests — concurrency contract already covered by concurrent-write tests.
- `tokenzero-recovery::virtual_paths_skip_source_fingerprint` and `line_range_payloads_skip_source_fingerprint` — assert internal fields, not behavior.
- `tokenzero-install::doctor_capabilities_names_doctor_contract_subcommands` — 40+ assertion snapshot; replace with schema key check.
- `tokenzero-install::apply_and_rollback_restore_temp_home` and `manifest_is_complete_for_every_written_file` — overlap / internal manifest detail.
- `tokenzero-mcp::bench_tests.rs` — move to `benches/` or delete.

## Fresh evidence

- `cargo test --workspace --locked --no-fail-fast` → **PASS**
- `cargo clippy --workspace --all-targets --locked -- -D warnings` → **PASS**
- `cargo fmt --all -- --check` → **DIFFS OBSERVED** in `crates/tokenzero-mcp/src/codemode/tests.rs` (pre-existing)
- `python3 scripts/check_embedded_tests.py` → **3 oversized inline test mods** (pre-existing)
- `python3 scripts/check_module_boundaries.py` → **PASS**

## Validation

```bash
python3 /Users/aditya/AI/JeffreySkills/_custom/verification-purpose-gate/scripts/validate_purpose_report.py .verification-purpose/06-purpose-report.md
# PASS: Basic structure OK
```

## Handoff

- Program-durable core (~541 tests) → keep; strengthen weak oracles.
- Session-only prune list (~74 tests) → hand to `zero-tech-debt`.
- Investigate list (~49) → review individually before deciding.
- Pre-existing fmt/embedded-mod findings → separate cleanup bead.

---

VERIFICATION_PURPOSE_RESULT:
status: findings
mode: audit-existing
target_root: /Users/aditya/AI/TokenZero
session_only_count: ~74
program_durable_count: ~541
loc_delta: -1000 to -1500 (if ~74 session-only tests removed)
value_added (new failure classes protected for program): n/a (audit); durable core covers recovery, shell safety, MCP conformance, sandbox security, package integrity, CLI contract, pulse ledger integrity
proof_summary: cargo test workspace PASS; clippy PASS; module boundaries PASS; format diffs and 3 oversized inline test mods are pre-existing findings
next_pass: prune ~74 session-only tests, strengthen weak oracles, review ~49 investigate items, fix pre-existing fmt/embedded-mod findings
QUEUE_ACTION: findings
