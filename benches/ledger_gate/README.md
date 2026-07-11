# Token ledger regression gate

This gate replays the versioned MCP corpus in `corpus.json` against a fresh, explicit recovery cache, reads the adjacent `ledger.jsonl`, and compares the resulting token mass with `baseline.json`. It is Python-only and does not build TokenZero.

## Run the gate

From the repository root, using an already-built debug binary:

```sh
python3 benches/ledger_gate/gate.py --binary target/debug/tokenzero
```

The command exits 0 when the candidate visible-token mass is within the allowed regression threshold and exits 1 when it exceeds the threshold. The report includes visible, raw, and prevented token masses plus per-tool call deltas.

To use a different evidence file or binary:

```sh
python3 benches/ledger_gate/gate.py \
  --binary /path/to/tokenzero \
  --baseline /path/to/baseline.json
```

## Threshold configuration

The checked-in baseline records `threshold_percent` (currently 5%). Override it for one run with `--threshold PERCENT`:

```sh
python3 benches/ledger_gate/gate.py --threshold 2.5
```

An override changes only that invocation; it does not rewrite the baseline.

## Update the baseline

Update evidence only after reviewing an intentional corpus or token-accounting change. Use the exact binary whose behavior should become the new reference:

```sh
python3 benches/ledger_gate/gate.py \
  --binary target/debug/tokenzero \
  --update-baseline
```

To update a non-default evidence file, combine `--baseline PATH` with `--update-baseline`. Review the generated JSON: corpus ID, record count, aggregate masses, session/turn counts, and per-tool calls are regression evidence, not hand-edited targets. Run the normal gate twice afterward; both candidate reports must have identical mass and tool-call numbers.

## Corpus versioning

`corpus.json` is an ordered, deterministic MCP JSON-RPC replay corpus. Its `corpus_id` is `ledger-gate-v1`. Preserve request order and stable inputs. Any semantic change to requests, fixtures, expected tool mix, or replay interpretation requires:

1. assigning a new corpus ID (for example, `ledger-gate-v2`),
2. updating the corpus as one reviewed change,
3. regenerating `baseline.json` with `--update-baseline`, and
4. recording before/after gate output in the change review.

Do not overwrite a baseline for a changed corpus while retaining the old corpus ID.
