# PR28 acceptance evidence

Bead: `tokenzero-kt7z`

Candidate: `06643ec818a5ef15bcdc7297eeb24a1b59865fc3`

Matched baseline: `origin/main` at `d7b95518dc370b48eb5fc0e9f8e8636f2a8728de`

## Existing repair audit

| Requirement | Existing committed repair/evidence | Gap before this audit |
|---|---|---|
| Cross-process refs stay fresh | `b2fb6dc` reloads the recovery store on an engine miss; engine and CodeMode process tests cover replay after another store/process writes. | Re-run focused tests. |
| Acknowledged ledger records have bounded durable flush | `b2fb6dc` adds the deadline-driven flush scheduler, retry retention, explicit/drop flushes, and deterministic boundary tests. | Re-run ledger tests. |
| Revert 44 unrelated deferrals | `4daf3d4` changes 45 records: restores 44 deferred beads to open and creates/claims this PR28 epic. | None. Audit the committed JSONL transition without changing `.beads`. |
| MCP and CodeMode compile/test | Repair commits include MCP/CodeMode tests through `8f994f1`; later CodeMode serialization fixes are `212e4dd` and `68dadac`. | Re-run focused packages on Spark. |
| Frozen matched workloads | `b2fb6dc` adds `scripts/compare_binaries.py`; `8f994f1` stabilizes its invocation. It retains every sample and alternates AB/BA. | MCP workloads reported wall only. This audit adds direct child-process CPU sampling so every CLI/MCP workload gates p50/p95 wall and CPU. Run and retain every matched repetition. |
| Installed artifact smoke | Existing packaging and install-prefix tests cover separate MCP and CodeMode artifacts. | Re-run against installed release artifacts. |

## Validation protocol

All builds, tests, formatting, clippy, and benchmarks run through:

`RCH_COMPRESSION_LEVEL=0 RCH_FORCE_REMOTE=1 rch exec -- <command>`

Performance runs use already-built release binaries, an immutable `README.md` fixture, isolated stores, alternating AB/BA order per workload, and the same trial count. Reports are committed in full. No run is discarded or selected as a best run.

## Validation results

| Acceptance line | Status | Evidence |
|---|---|---|
| MCP and CodeMode compile/targeted tests | PASS with caveat | Spark release builds for both revisions passed. `packaging_static_evidence` passed 8/8. The broad CodeMode filter compiled but selected 0 tests; the narrower replay test command was still running when this evidence was committed. |
| Cross-process recovery refs fresh | UNVERIFIED in this audit | Existing repair/test is committed, but the fresh narrow Spark replay invocation did not finish before closure. |
| Acknowledged ledger bounded durable flush | PASS | Spark `cargo test -p tokenzero-engine ledger_tests -- --nocapture`: 10 passed, 0 failed, including deadline boundary, timed flush, retry, explicit flush, and drop flush. |
| All 44 unrelated deferrals reverted | PASS | Commit `4daf3d4` changes 45 records: 44 `deferred` to `open`, plus this epic. Current `.beads` was not edited or staged. |
| Frozen workload performance and warm MCP read | FAIL | All three valid matched repetitions fail. Warm MCP read candidate wall p50/p95 regresses by 38.06%/21.93%, 37.25%/23.56%, and 36.99%/17.58%. Other CLI p50/p95 regressions are recorded without omission in the JSON files. |
| Installed-artifact smoke | PASS | Spark `cargo test -p tokenzero --test packaging_e2e install_each_surface -- --test-threads=1 --nocapture`: 1 passed, 0 failed; independent installed MCP and CodeMode prefixes executed. |

### Matched reports

- `benchmarks/claims/pr28/run-1.json`: 50 AB/BA samples per binary/workload, gate failed.
- `benchmarks/claims/pr28/run-2.json`: 50 AB/BA samples per binary/workload, gate failed.
- `benchmarks/claims/pr28/run-3.json`: 50 AB/BA samples per binary/workload, gate failed.

Every JSON report includes p50/p95 wall and CPU for all six CLI and two warm MCP workloads. No valid repetition was discarded. An initial three-run attempt exposed an unsafe macOS `ctypes` CPU sampler: each invocation ended with signal 11 before producing a report. Commit `060f0d1` replaces it with safe `ps` snapshots; the three post-fix reports above are all completed repetitions. macOS `ps` CPU resolution is 10 ms, disclosed in each report, so short MCP samples can be zero.

### Exact commands

```sh
RCH_COMPRESSION_LEVEL=0 RCH_FORCE_REMOTE=1 rch exec -- cargo test -p tokenzero-engine ledger_tests -- --nocapture
RCH_COMPRESSION_LEVEL=0 RCH_FORCE_REMOTE=1 rch exec -- cargo test -p tokenzero --test packaging_e2e install_each_surface -- --test-threads=1 --nocapture
RCH_COMPRESSION_LEVEL=0 RCH_FORCE_REMOTE=1 PERF_TRIALS=50 PERF_NOISE_TOLERANCE_PCT=1.0 PERF_JSON=benchmarks/claims/pr28/run-N.json CANDIDATE_BIN=/Users/aditya/AI/TokenZero/target/release/tokenzero rch exec -- make perf-regression-gate BASELINE_BIN=/tmp/tokenzero-pr28-origin-main/target/release/tokenzero
```

Acceptance is not met because performance fails and fresh cross-process replay remains unverified.
