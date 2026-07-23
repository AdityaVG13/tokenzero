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

Pending focused correctness, installed-artifact smoke, and three matched repetitions.
