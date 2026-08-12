# Ship suite

TokenZero has two local test tiers plus the shared ZeroStack testkit.

- Development tests stay beside their crates and remain available through
  targeted `cargo test -p <crate>` commands.
- The release proof is the `tokenzero-ship-tests` workspace package in `tests/`:
  at most 50 observable tests and 2,500 Rust lines.
- Shared codecs and reusable fixtures belong in `zero-testkit`, not in another
  engine-local harness.

Run the static policy gate:

```sh
python3 scripts/check_ship_suite.py
```

Run the bounded release proof:

```sh
python3 scripts/run_ship_suite.py
```

## Mutation evidence

Every ship test has one reproducible product-source mutant in
`tests/ship-mutations.json`. `scripts/verify_ship_mutations.py` applies each
mutant, rebuilds the affected public binary, and requires that test to fail.
It restores every source file and writes `tests/ship-mutation-receipts.json`.
The static gate rejects missing, stale, surviving, or preimage-drifted receipts.

```sh
python3 scripts/verify_ship_mutations.py
```

## Full classification

`docs/test-classification-v1.jsonl` records every source-level `#[test]` and
`#[tokio::test]` attribute under `crates/` and `tests/`. Each row is classified
as `DUPLICATE`, `SCAFFOLDING`, `SHARED`, `DEV-ONLY`, or `SHIP`, with its release
coverage mapping and rationale. Regenerate after adding or removing tests:

```sh
python3 scripts/classify_tests.py docs/test-classification-v1.jsonl
python3 scripts/check_ship_suite.py
```

The 2026-08-12 baseline classifies 1,146 source tests: 1,003 development-only,
26 migration scaffolds, 105 shared-contract adapters, and 12 ship tests.
