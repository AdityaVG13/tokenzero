# ZeroStack CodeMode machine permits (v1)

Canonical family-wide ambient CPU budget contract for TokenZero, FSZero, GraphZero, and the ZeroStack hub.

**Beads:** `tokenzero-npia` (epic), `tokenzero-qisj` (freeze), `fszero-gzw`, `graphzero-01vw`.

| Class | Default path | Default concurrency | Env (TokenZero) |
|-------|--------------|---------------------|-----------------|
| Status / describe / containment snapshot | ungated | n/a | n/a |
| Analysis (light find/search/plans) | `/tmp/zerostack-codemode-analysis.permit` | `max(1, cores/4)` soft-capped at 8 | `TOKENZERO_CODEMODE_ANALYSIS_PERMIT`, `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY`, `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP` |
| Index (rebuild / watch.drain / `.index(`) | `/tmp/zerostack-codemode-index.permit` | `max(1, cores/8)` soft-capped at 2 | `TOKENZERO_CODEMODE_INDEX_PERMIT`, `TOKENZERO_CODEMODE_INDEX_CONCURRENCY`, `TOKENZERO_CODEMODE_INDEX_CONCURRENCY_CAP` |
| Heavy (shell / high-cost plans) | `/tmp/zerostack-codemode-heavy.permit` | 1 | `TOKENZERO_CODEMODE_HEAVY_PERMIT`, `TOKENZERO_CODEMODE_HEAVY_CONCURRENCY` |

## Family-wide path rule

TokenZero, FSZero, GraphZero, and the ZeroStack hub **MUST** use the same default permit paths above. Sibling engines may rename env vars to their own prefix, but default filesystem paths stay identical so concurrent CodeMode workers cannot stack CPU.

## Multi-tenant goal (100 sessions)

Hundreds of concurrent CodeMode sessions should stay responsive without aggregate CPU saturation:

1. Sessions **share** the analysis and index slot pools (they do not each get a full core budget).
2. Each active search is thread-capped (`rg --threads 1`); internal walks stay single-threaded.
3. In-plan JS fanout defaults to `max_parallel_width = 2` (was 16).
4. Waiters use exponential backoff (20ms → 200ms) so idle sessions do not wake-storm the permit directory.
5. Contention returns retryable `busy` / `machine_permit_busy` — never a silent ok.

Example on a 16-core host: default analysis slots = 4, default index slots = 2. One hundred sessions queue for those pools; peak search/index CPU stays near the budget, not one hundred times cores.

## Rules

1. Permit directories always use numbered `slot-N` children under the base path — including when concurrency is 1 (`base/slot-0`). Never lock `base` itself for the slots==1 shortcut; mixed concurrency envs must not stack.
2. Dead holders are reclaimed via pid liveness checks; incomplete dirs reclaim after a short grace. A live legacy exclusive permit at `base` (pid/owner directly under base) blocks all slots until reclaimed or released.
3. Acquire waits until the wall deadline, then returns retryable busy — never silent success while the permit is held.
4. Identical in-flight plans may coalesce under a bounded follower set; overflow is also retryable busy.
5. Status / describe / containment snapshot probes stay ungated.
6. Explicit `*_CONCURRENCY` env values are clamped to `*_CONCURRENCY_CAP` (`min(explicit, cap)`).

TokenZero owns this contract (`tokenzero-npia`, `tokenzero-qisj`). FSZero (`fszero-gzw`) and GraphZero (`graphzero-01vw`) adopt it.
