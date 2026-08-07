# TokenZero benchmarks

One question: how much context does TokenZero save versus the alternatives,
and what does that cost in latency.

## Run everything

```bash
benchmarks/run_all.sh
```

That is the only command. It runs every benchmark below, records the date,
commit, machine, and tool versions, and regenerates `docs/benchmarks.md`.
`RUNS` and `WARMUP` env vars control repetitions (default 5/1).

A `tokenzero` binary is resolved from `$TOKENZERO_BIN`, then PATH, then
`target/release/tokenzero`. Build one first with
`cargo build --release -p tokenzero-cli --bin tokenzero --no-default-features`.

## What each file is

| File | Role |
| :-- | :-- |
| `run_all.sh` | Single entry point; regenerates `docs/benchmarks.md` with provenance. |
| `cli-cold-read.sh` | Cold vs warm latency for process start, store open, first read, first expand (p50/p90/p99). |
| `competitor-bakeoff.sh` | TokenZero vs raw CLI and installed competitors (rtk, lean-ctx, headroom, ztk, context-mode) on identical tasks. Uninstalled tools are marked, never fabricated. |
| `million-line-nav.sh` | Navigation tasks on a generated million-line synthetic repo; visible tokens vs raw bytes. |
| `never_worse_gate.py` | Validates same-surface candidate and raw receipts without weakening failed rows. |
| `test_harness_refs.py` | Regression tests for strict durable-ref and glob-trie parsing. |
| `test_never_worse_gate.py` | Regression tests for never-worse receipt validation. |
| `harness.py` | Shared measurement library for the shell runners (binary resolution, timing cells, token counting). |
| `bench_common.py` | Shared helpers for the retained harness and `tokenzero-mcp-compat` benchmark scripts. |
| `__init__.py` | Lets runners use `python3 -m benchmarks.harness`. |

## Honesty requirements (keep these when editing)

- Every published number states the exact command, tool version, corpus, and machine.
- Report where TokenZero loses or ties; a sweep where it wins everything is not credible.
- Same corpus, same task, same measurement point on both sides of a comparison.
- Report spread (p50/p90/p99 or median/best), never a single best-of run.
- Results in `docs/benchmarks.md` are regenerated, dated, and machine-stamped; recorded outputs of past runs are not tracked in git.
