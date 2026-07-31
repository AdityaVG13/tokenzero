# Recovery-Aware Context Compression

RACC is TokenZero's public compression model.

The goal is not to make every response as short as possible. The goal is to minimize total task cost while keeping exact recovery available when visible context is not enough.

## Components

| Component | Meaning |
| --- | --- |
| Visible capsule | The compact text returned to the agent immediately |
| Exact cached payload | Byte-for-byte local payload stored outside model-visible context |
| Recovery handles | Stable refs for raw payload, file ranges, anchors, symbols, search hits, or error blocks |
| Recovery-adjusted objective | Visible tokens plus tokens recovered later for task completion |
| Task-lossless savings | Recovery-adjusted savings counted only for non-failing, non-negative events or validation tasks that preserve required facts |
| RATC | Cost proxy: visible tokens plus recovery tokens plus configured retry and failure penalties |

## Contract

TokenZero may omit payload text from the visible capsule only when one of these is true:

- The omitted content is already represented by a protected anchor.
- The omitted content is recoverable through an exact local ref.
- The mode explicitly chooses lossy visible compression and reports that recovery may be needed.

Exact refs are not model-readable payloads. They are local handles. A response that only emits an exact ref has high visible savings, but honest evaluation must count any later `expand` output used by the agent.

### Omission enforcement (RACC backport)

Capsule emission validates this rule at runtime. Exact recovery evidence must be a visible `tz://` handle with a concrete byte, line, or symbol selector. Protected-anchor evidence must name a visible `[[anchor:...]]`. A capsule without either must set `mode: lossy`, provide non-empty `lossy_spans` whose entries declare `recovery_may_be_needed: true`, and name a stable `lossy_policy_id`. The visible text repeats the lossy declaration so that consumers which render only capsule text cannot silently discard the warning.

The backport intentionally treats the omission declaration as a correctness floor: for an impossibly small token budget, the complete declaration may exceed the budget rather than degrade to unclassified text such as `omitted`.

## Public Objective

TokenZero tracks:

- Visible savings: first response token reduction.
- Recovery-adjusted savings: first response tokens plus recovered tokens.
- Task-lossless savings: recovery-adjusted savings after exact recovery and task-success gates.
- RATC: visible tokens plus weighted recovery, retry, and failure penalties for release reports.
- Exact-ref savings: compact handle cost, reported separately from model-readable content.
- Task success: whether expected task facts are present after any recovery.
- Anchor recall: preservation of signatures, imports, symbols, paths, errors, literals, and other protected facts.
- Downstream cost: latency, repeated reads, cache hits, and recovery requests.

## Zero Loss By Recovery

TokenZero's public claim is zero loss by recovery for local runtime payloads: the exact original payload can be recovered from the local cache while the cache entry exists.

The cache is bounded (per-kind counts plus a byte ceiling), and under pressure the oldest entries are evicted first. An evicted ref reports `dangling-ref` on expand — never wrong bytes — and eviction cannot break a surviving ref: every ref kind stores its payload inline, so dropping one entry never dangles another.

That is different from claiming every visible capsule is semantically complete. Visible capsules are measured by task success and anchor recall. Exact refs are measured by roundtrip recovery. Exact mode is the deliberate exception on the visible side: it hides the payload behind the ref by contract, trading visible anchors for ref-only recovery.

## Promotion Rule

A compression profile is not promoted by visible savings alone.

It needs:

- Exact refs with no dangling handles.
- Recovery-adjusted savings above the baseline.
- Task-lossless savings that does not regress behind a visible-only win.
- No protected-anchor regression for safety-critical modes.
- Task success on the release validation trace set.
- Clear artifact paths outside model-visible context.

### Reading the pilot promotion verdict

Run `cargo run -p tokenzero-xtask -- pilot-report REPORT_JSON REPORT_CSV` to write
measurement artifacts without making a promotion decision. Then run
`cargo run -p tokenzero-xtask -- pilot-gate REPORT_JSON REPORT_CSV` to consume that existing
JSON report, revalidate and recompute every aggregate and verdict from its per-task rows, rewrite
both artifacts, and exit nonzero when `promotion.eligible` is false. Human/operator approval
remains required even when the verdict is promote.

The `tokenzero.pilot-ab-report.v2` verdict is deliberately strict and machine-readable:

- exact-ref mean RATC must be strictly lower than baseline mean RATC;
- no task may exceed 2.0x its baseline RATC;
- aggregate and per-task success must not regress;
- aggregate and per-task protected-anchor recall must not regress; and
- dangling_unresolved must be zero.

Only the frozen `eviction-stress` task's explicit one-miss fallback is classified as resolved,
and only when the task succeeds with exactly one dangling ref and one typed failure. Every other
dangling ref is unresolved even when an unrelated success predicate passes. `failed_checks` names
every failed rule, while each `checks` entry records the observed values and requirement. Visible-token savings never override a
failed RATC, success, anchor, or dangling check. The fixed suite supplies deterministic deltas, not a
statistical-significance claim.
