# ZeroStack CodeMode machine permits

Cross-process CPU containment for TokenZero, FSZero, GraphZero, and the ZeroStack hub.

| Class | Default path | Default concurrency | Env (TokenZero) |
|-------|--------------|---------------------|-----------------|
| Heavy (shell / high-cost plans) | `/tmp/zerostack-codemode-heavy.permit` | 1 | `TOKENZERO_CODEMODE_HEAVY_PERMIT`, `TOKENZERO_CODEMODE_HEAVY_CONCURRENCY` |
| Analysis (light find/search/plans) | `/tmp/zerostack-codemode-analysis.permit` | `max(1, cores/4)` soft-capped at 8 | `TOKENZERO_CODEMODE_ANALYSIS_PERMIT`, `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY`, `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP` |
| Status / describe / containment snapshot | ungated | n/a | n/a |

## Multi-tenant goal (100 sessions)

Hundreds of concurrent CodeMode sessions should stay responsive without aggregate CPU saturation:

1. Sessions **share** the analysis slot pool (they do not each get a full core budget).
2. Each active search is thread-capped (`rg --threads 1`); internal walks stay single-threaded.
3. In-plan JS fanout defaults to `max_parallel_width = 2` (was 16).
4. Waiters use exponential backoff (20ms → 200ms) so idle sessions do not wake-storm the permit directory.
5. Contention returns retryable `busy` / `machine_permit_busy` — never a silent ok.

Example on an 16-core host: default analysis slots = 4. One hundred sessions queue for those four slots; peak search CPU stays near four cores, not one hundred.

## Rules

1. Permit directories are exclusive locks (or numbered `slot-N` children when concurrency > 1).
2. Dead holders are reclaimed via pid liveness checks; incomplete dirs reclaim after a short grace.
3. Sibling engines must use the same default paths so concurrent CodeMode workers cannot stack CPU.

TokenZero owns this contract (`tokenzero-lpi4`, `tokenzero-vsn3`). FSZero (`fszero-gzw`) and GraphZero (`graphzero-01vw`) adopt it.
