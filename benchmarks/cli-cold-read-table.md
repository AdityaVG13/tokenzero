# CLI Cold Read Profiling — TokenZero

> Bead: `tokenzero-f1z` — "Profile cold vs warm: hyperfine n>=50, release-perf,
> committed artifact. Break down: process start, store open, first read, expand.
> p50 cold read of a small file on M-series: target relative to `tokenzero --help`
> floor. Regression gate in CI or nightly."
>
> Startup tax = cold `first_read` p50 − cold `process_start` p50.

## Scope

- **Cold boundary**: recovery cache removed before each run
  (`rm -f ~/.tokenzero/recovery-cache.json`).
- **Warm boundary**: cache left in place, no removal between runs.
- **Small file**: `Cargo.toml` in the repository root.
- **Release binary**: `target/release/tokenzero` or `~/.tokenzero/bin/tokenzero`.
- **Runner**: `hyperfine` with `--warmup 3 --runs 50` (falls back to `/usr/bin/time`
  × 50 if absent).

## Components measured

1. **Process start** — `tokenzero --help` (pure binary launch floor).
2. **Store open** — `tokenzero mem` (recovery-cache init).
3. **First read** — `tokenzero read --end-line 1 Cargo.toml` (first useful bytes).
4. **First expand** — `tokenzero expand <ref>` (first ref expansion; ref resolved
   from a prior `read` of the small file).

## Results (ms)

| Component | cold p50 | cold p90 | cold p99 | warm p50 | warm p90 | warm p99 |
|---|---:|---:|---:|---:|---:|---:|
| `process_start` (`--help`) | … | … | … | … | … | … |
| `store_open` (`mem`) | … | … | … | … | … | … |
| `first_read` (`read`) | … | … | … | … | … | … |
| `first_expand` (`expand`) | … | … | … | … | … | … |
| **Startup tax** = cold first_read p50 − process_start p50 | **… ms** | — | — | — | — | — |

Legend: `…` = fill from script output. Values are wall-clock milliseconds.
`Startup tax` is the cold-read overhead above the pure binary launch floor.

## How to reproduce

```bash
# Build the binary under test (one-time)
cargo build --release --bin tokenzero

# Run the profiler (writes markdown table to stdout)
./benchmarks/cli-cold-read.sh > results.md

# Faster CI smoke run (fewer samples, no warmup)
WARMUP=0 RUNS=10 ./benchmarks/cli-cold-read.sh > ci-fast.md

# Override the binary under test
TOKENZERO_BIN=$PWD/target/release/tokenzero ./benchmarks/cli-cold-read.sh
```

Artifacts: per-run hyperfine JSON dropped under `${TMPDIR:-/tmp}/tz-cli-cold.*`
during the run and cleaned up after.

## Pass/Fail rubric

| Metric | Target | Action if failed |
|---|---|---|
| cold `first_read` p50 | within `help p50 + 2× startup tax` baseline | Profile `store_open`; defer cache init. |
| cold `first_expand` p50 | within `help p50 + 3× startup tax` baseline | Check ref-index load path. |
| warm vs cold p50 delta | warm p50 ≤ 0.5 × cold p50 | Verify recovery-cache is reused. |
| p99 / p50 ratio | ≤ 3.0 | Investigate outliers (disk, OS caches). |

## Regression gate (CI / nightly)

- Commit a baseline row into this table on a known-good release.
- CI runs `WARMUP=0 RUNS=10 ./benchmarks/cli-cold-read.sh` as a smoke gate;
  nightly runs the full `RUNS=50` sweep.
- Fail if `first_read` cold p50 regresses beyond the committed baseline + 10%.

## Open questions

- Is `~/.tokenzero/recovery-cache.json` removal sufficient to force cold, or
  does a secondary index persist?
- Does `tokenzero mem` trigger the full RACC sweep, or only the index load?
- Should `first_expand` use a guaranteed-present ref rather than one resolved
  from a prior `read`?
