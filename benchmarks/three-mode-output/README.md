# Three-mode output benchmark

This is a receipt builder for matched `full_file`, `text_diff`, and Zero Edit
Protocol (`edit_protocol`) trials. It does not call a model and ships no product,
speed, savings, or release claim. Feed it raw trials from a provider run.

## Run

```sh
python3 benchmarks/three_mode_output.py \
  --lbi results/three-mode/lbi.json \
  --tasks results/three-mode/tasks.json \
  --trials results/three-mode/trials.jsonl \
  --out results/three-mode/report.json
```

Add `--require-supported` only on a preregistered claim path. It returns 3 for
`falsified_on_locked_suite` or `insufficient_locked_scales`. Validation errors
return 2. A valid falsification report otherwise returns 0 so losses are not
hidden.

## Z7 locked benchmark identity

`lbi.json` uses `tokenzero.three-mode.lbi.v1`. Freeze it before any trial. The
harness hashes canonical JSON and rejects every trial whose `lbi_sha256` differs.
Required pins cover:

- model provider, model id, weights revision, and execution identity;
- `backend_identity` id, revision, and routing-policy digest;
- `reasoning_config` effort and full configuration digest;
- decoder/sampling law and random stream;
- tokenizer revision and canonical rendering schema;
- `output_cap` maximum tokens and cap-policy digest;
- `transcript_policy` assembly and prefix-policy digests;
- repository commits and tree digests;
- the canonical task-manifest digest;
- tool/effect interfaces and verifier command;
- hardware, setup, and index receipts;
- fallback, timeout, and resource policies;
- fresh-work accounting, an inline USD/micro-USD price card plus its canonical
  digest, and the cost policy;
- the exact `cold` and `retained` cache states;
- `amortization_policy` horizon, schema-charge rule, and policy digest;
- seeds, exclusions, and statistical rule;
- `trial_order_policy` randomization algorithm/seed and the full `trial_order`.

Randomize the paired cell order before freezing the LBI. Record the method as
`randomized_before_lock`, the algorithm, and its seed. The harness requires the
raw JSONL rows to match the locked `trial_order` exactly; a reordered, missing,
duplicate, or extra cell fails closed.

The task manifest uses `tokenzero.three-mode.tasks.v1`. Each task pins its prompt,
snapshot, expected artifact, scale group, and scale rank. Every task must have
one trial for every `(mode, seed, cache_state)` cell. This makes the comparison
paired and prevents a missing loss from becoming an average win.

## Raw trial schema

Each JSONL row uses `tokenzero.three-mode.trial.v1` and contains:

```json
{
  "schema_version": "tokenzero.three-mode.trial.v1",
  "trial_id": "task-a-edit_protocol-7-cold",
  "lbi_sha256": "<canonical lbi sha256>",
  "task_id": "task-a",
  "requested_mode": "edit_protocol",
  "cache_state": "cold",
  "seed": 7,
  "outcome": "success",
  "attempts": [
    {
      "kind": "primary",
      "mode": "edit_protocol",
      "outcome": "success",
      "raw_output_path": "raw/task-a-edit_protocol-7-cold.json",
      "usage": {
        "input_tokens": {"class": "billed", "value": 123},
        "cached_input_tokens": {"class": "absent"},
        "output_tokens": {"class": "billed", "value": 17}
      },
      "backend_work": {
        "fresh_work_tokens": {"class": "exact", "value": 4},
        "replayed_tokens": {"class": "exact", "value": 90},
        "recovery_tokens": {"class": "exact", "value": 2},
        "overhead_tokens": {"class": "exact", "value": 27},
        "file_read_bytes": {"class": "exact", "value": 0},
        "index_query_units": {"class": "exact", "value": 0},
        "tool_executions": {"class": "exact", "value": 1},
        "verifier_runs": {"class": "exact", "value": 1},
        "latency_ms": {"class": "exact", "value": 40}
      },
      "total_cost_microusd": {"class": "billed", "value": 81}
    }
  ],
  "materialized_artifact_path": "artifacts/task-a.out",
  "verifier_receipt_path": "verifiers/task-a-edit_protocol-7-cold.json"
}
```

Paths must be relative to the JSONL directory and cannot escape it. Every raw
attempt and verifier receipt is hashed into the report. A successful trial must
materialize the task's exact artifact digest. A successful compact attempt must
be valid `zep/1` JSON. Failed primary, repair, and Level-0 fallback attempts stay
in the raw receipt and all denominators.

## Accounting rules

Every usage and cost scalar is an observation with one class:

- `exact`: measured by the local harness or exact counter;
- `estimated`: derived rather than provider billed;
- `billed`: provider-reported billing usage or charge;
- `absent`: the source reported no value. It has no `value` field.

`{"class":"absent"}` is not zero. The report carries observed sums, observed
counts, absent counts, and class counts. `total_cost_microusd` is integer
micro-USD; no binary floating-point money enters a receipt. The LBI carries the
full price card (`currency`, `unit`, `source`, revision, and integer rates) and a
canonical digest, so the report retains the assumptions rather than only a
number.

`eta_action_ppm` follows active `fresh-work-vector-v1`:
`fresh_work_tokens / (fresh + replayed + recovery + overhead)`. It is `null` for
an undeclared all-zero vector or any absent component. The separate
`action_to_artifact_ppm` measures emitted action bytes divided by materialized
artifact bytes; the two ratios are never conflated.

The frontier verdict is scoped to the locked suite. It compares at least two
artifact scales in the same logical `scale_group`, requires both textual diff
and ZEP/1 action growth to remain below artifact growth, retains pointwise
failures and fallbacks as falsifiers, and never implies a product or release
claim.
