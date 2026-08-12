# tokenzero-5emy -- Recovery CAS/GC offload to canonical zero-store (receipt)

Bead: `tokenzero-5emy` -- replace duplicated GC/schema/mark-sweep/report/repair
implementation in `crates/tokenzero-recovery/src/shared_cas.rs` with a small
compatibility adapter over canonical `zero_store` exports, and repin ZeroStack
git deps to the hub rev that carries the canonical implementation.

## Decision citation

- **Model A** (infrastructure-only ZeroStack foundation crates, git/lockfile
  pin now, crates.io later) is **APPROVED** by Aditya on **2026-08-12**.
  Recorded in `docs/zero-foundation-rfc.md` (status replaced from DRAFT).
- Hub blocker (zero-store GC) resolved on pushed `origin/main` rev
  `8188fb08698a5ed29bff6b339657bdd1933de3cc`; canonical GC landed in
  `b30c0f3..1c4674a` (trace shared GC reachability).

## Seam

| What | Where |
|---|---|
| Adapter rewrite | `crates/tokenzero-recovery/src/shared_cas.rs` |
| Hub pin | `Cargo.toml` (7 git deps, including `zero-mcp`) + `Cargo.lock` |
| Test contract update | `crates/tokenzero-recovery/src/segment_store_tests.rs` (v2 pin record fields) |
| Gate tests | `crates/tokenzero-recovery/tests/shared_cas_{gc_hygiene,gc_publish_race,publish_lease,pin_query}.rs`, `zeroref_lifecycle_smokes.rs`, `zeroref_conformance_matrix.rs` |
| Performance evidence | `crates/tokenzero-recovery/benches/perf_hotspots/{baseline,candidate,comparison}.json` |

Preserved TokenZero surface: `SharedCas` + `SharedCasError` taxonomy (all six
variants), cache-path helpers (`resolve_cache_root`, `attach_root_for_cache_path`,
`sibling_engine_cache_path`, `detect_from_cache_path`), `publish_leased` /
`release_lease`, `is_pinned` (conservative pin query), `project_id`,
`format_system_time`, `unique_suffix`, `content_sha256_hex` (aliases
`zero_ref::content_hash_hex`), `lower_hex`, `GC_ENGINE_TOKENZERO`,
and hub GC re-exports. Deleted duplication: record structs, schema validation,
mark-sweep, dry-run reports, sweep progress, GC repair, report pruning,
and general RFC3339 parsing/validation. The small engine-local timestamp
formatter remains for existing TokenZero call sites.

## Rollback boundary

- `Cargo.toml` git rev pin (one line per dep) plus `Cargo.lock`.
- Reverting the pin to `fa253840910ab4051635e2de95f04ddf6043a000` restores the
  prior build; binaries are static-linked per engine and remain runnable.
- `docs/zero-foundation-rfc.md` status change is documentation-only.

## Performance budgets (each no regression >5% vs baseline)

- Wall time across the committed cold/warm read, CAS ingest/expand, shell,
  and repeated recovery-persist workloads.
- Peak process RSS across the same workloads.
- Binary size (`target/debug/tokenzero`; identical local build profile).

Measurement commands (run before and after; recorded below):
`python3 crates/tokenzero-recovery/benches/perf_hotspots.py --label
<baseline|candidate> --replicates 5` (`/usr/bin/time -l` internally);
`stat -f%z target/debug/tokenzero` for binary size; then
`perf_hotspots.py --compare --baseline-size <bytes> --candidate-size <bytes>
--baseline-revision <rev> --candidate-revision <rev>
--baseline-zero-abi-source <source> --candidate-zero-abi-source <source>`.

## Baseline

- `crates/tokenzero-recovery/src/shared_cas.rs`: **1374 Tokei code LOC**
  (git `04b68c4`), post-change target: **<474 code LOC** (>=900 deleted).
- ZeroStack dep pins: `fa253840910ab4051635e2de95f04ddf6043a000` ->
  `8188fb08698a5ed29bff6b339657bdd1933de3cc`.
- LOC command: `tokei -o json crates/tokenzero-recovery/src/shared_cas.rs`.

## Gate status

- rustfmt: exact-path on changed files.
- RCH targeted: tokenzero-recovery GC/CAS + lease + race tests, embedded
  store, zeroref lifecycle/conformance.
- clippy: `-p tokenzero-recovery --lib -- -D warnings`.
- `CARGO_TARGET_DIR=/tmp/rch_target_tokenzero` throughout.

## Results (post-validation, 2026-08)

- shared_cas.rs production code LOC: **1374 -> 299** (**-1075**; target
  >=900 met). Physical lines: **1525 -> 369** (**-1156**).
- Hub declarations: `Cargo.toml` all 7 ZeroStack dependencies now name
  `8188fb08698a5ed29bff6b339657bdd1933de3cc`; `Cargo.lock` refreshed via
  `cargo update -p zero-store -p zero-ref -p zero-abi -p zero-gauge
  -p zero-ledger -p zero-process`, followed by `cargo check -p
  tokenzero-mcp-compat --no-default-features` to add `zero-mcp`; zero remaining
  `fa25384` refs exist in the lockfile. The pre-existing local `zero-mcp` path
  and `zero-abi` patch were removed, so the full hub dependency graph resolves
  from pushed `8188fb0`.
- Cargo gates (all run with `CARGO_TARGET_DIR=/tmp/rch_target_tokenzero`):
  - rustfmt (exact-path): `rustfmt --edition 2021
    crates/tokenzero-recovery/src/shared_cas.rs
    crates/tokenzero-recovery/src/segment_store_tests.rs` -- clean.
  - Tests: `shared_cas_gc_hygiene` 2/2, `shared_cas_gc_publish_race` 1/1,
    `shared_cas_publish_lease` 3/3, `zeroref_lifecycle_smokes` 1/1,
    `zeroref_conformance_matrix` 1/1 (+1 ignored), lib `segment_store` 13/13,
    lib `embedded_store` 34/34 -- all pass, `-- --test-threads=1`.
  - clippy: `cargo clippy -p tokenzero-recovery --lib -- -D warnings` -- clean.
  - dependents: `cargo check -p tokenzero-engine -p tokenzero-cli` -- ok.
  - Full lib suite: 207 passed, **1 failed: pre-existing**
    `tests::ref_index_pay_once_reuses_one_user_cas_object_across_sessions`
    (fails identically on baseline commit `04b68c4` at pin `fa25384...`;
    unrelated to this bead's seam -- ref-index path, same assertion).
- Parent RCH verification reran the focused suites: GC/lease/race **6/6**,
  segment store **13/13**, embedded store **34/34**, ZeroRef local
  conformance **2 passed + 1 external-binary test ignored**, conservative pin
  and repair taxonomy **4/4**, targeted clippy clean, and pinned
  `tokenzero-mcp-compat` dependency check clean.
- Performance budgets (wall/RSS/binary size, no regression >5%): **PASS**.
  `perf_hotspots.py` was repaired for the current string-ref CLI schema and
  the public 256 KiB raw-expand cap, then the same deterministic harness ran
  five complete times against both baseline and candidate binaries. Median
  comparison has maximum wall-time delta **+3.23%**, maximum RSS delta
  **+2.01%**, and binary size **36,350,392 -> 36,803,208 bytes (+1.25%)**.
  No run was discarded. Full raw samples and methodology are in
  `perf_hotspots/{baseline,candidate,comparison}.json`; the comparison labels
  the baseline local `zero-abi` override and candidate pushed pin explicitly.
- Baseline SHA/LOC: `04b68c4` / `fa253840910ab4051635e2de95f04ddf6043a000` /
  shared_cas.rs 1525 lines; post: `04b68c4+` / `8188fb0...` / 369 lines.
