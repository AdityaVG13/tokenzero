# ZeroStack CodeMode machine permits

Cross-process CPU containment for TokenZero, FSZero, GraphZero, and the ZeroStack hub.

| Class | Default path | Default concurrency | Env (TokenZero) |
|-------|--------------|---------------------|-----------------|
| Heavy (shell / high-cost plans) | `/tmp/zerostack-codemode-heavy.permit` | 1 | `TOKENZERO_CODEMODE_HEAVY_PERMIT`, `TOKENZERO_CODEMODE_HEAVY_CONCURRENCY` |
| Analysis (light find/search/plans) | `/tmp/zerostack-codemode-analysis.permit` | 1 | `TOKENZERO_CODEMODE_ANALYSIS_PERMIT`, `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY` |
| Status / describe / containment snapshot | ungated | n/a | n/a |

## Rules

1. Permit directories are exclusive locks (or numbered `slot-N` children when concurrency > 1).
2. Dead holders are reclaimed via pid liveness checks; incomplete dirs reclaim after a short grace.
3. Contention returns a retryable `busy` / `machine_permit_busy` error — never a silent ok.
4. Sibling engines must use the same default paths so concurrent CodeMode workers cannot stack CPU.

TokenZero owns this contract (`tokenzero-lpi4`). FSZero (`fszero-gzw`) and GraphZero (`graphzero-01vw`) adopt it.
