# Boot Cost Benchmark — TokenZero

> Bead: `tokenzero-65s` — "Measure exactly what cold-boot RACC costs in tokens
> and wall time across repo sizes. Attribute cost to each boot component."
>
> Sub-100-token ceiling on first provider request after process start.

## Scope

Cold boundary = first provider request after `tokenzero` process start.
Cold = recovery cache cleared before run.
Warm = cache populated, no clear between runs.
Repo sizes: small (current workspace), synthetic 23k-file, synthetic 100k-file.
Runner: `hyperfine` with `--warmup 3 --runs 50` (falls back to `/usr/bin/time` × 50 if absent).

## Components measured

1. **Process start** — `tokenzero --help` (pure binary launch; no cache touch)
2. **Store open** — `tokenzero mem` (recovery-cache init, lazy file scan)
3. **First read** — `tokenzero read --end-line 1 <anchor-file>` (RACC ingest pass)
4. **First expand** — `tokenzero expand tz://local/<ref>` (cold path; ref present/empty)

Total = sum of the four rows.

## Results (median ms / Δtokens)

| Component | small cold (ms / tok) | small warm (ms / tok) | 23k cold (ms / tok) | 23k warm (ms / tok) | 100k cold (ms / tok) | 100k warm (ms / tok) |
|---|---:|---:|---:|---:|---:|---:|
| `process_start` (`--help`) | … | … | … | … | … | … |
| `store_open`     (`mem`)    | … | … | … | … | … | … |
| `first_read`     (`read`)   | … | … | … | … | … | … |
| `first_expand`   (`expand`) | … | … | … | … | … | … |
| **Total boot** | … | … | … | … | … | … |
| **Sub-100 tok target** | ✓/✓ | — | ✓/✓ | — | ✓/⚠ | — |

Legend: `✓` = within budget, `⚠` = over budget, `STRETCH` = known-but-acceptable
overshoot for the 100k-file synthetic case (target is the small+23k codepath).

## How to reproduce

```bash
# Build the binary under test (one-time)
cargo build --release --bin tokenzero

# Small repo only (uses current workspace, no synth generation)
./benchmarks/boot-cost.sh --small-only > small.md

# Full sweep (generates 23k + 100k synthetic repos in /tmp/tz-bench-synth/)
./benchmarks/boot-cost.sh > full.md

# Optional overrides
WARMUP=1 RUNS=10 TOKENZERO_BIN=$PWD/target/release/tokenzero \
  ./benchmarks/boot-cost.sh > ci-fast.md
```

Artifacts: per-run JSON dropped under `${TMPDIR:-/tmp}/tz-bench.*` during sweep.
Synth repos persist across runs under `${SYNTH_DIR:-/tmp/tz-bench-synth}`.

## Token attribution

Δtokens captured via `tokenzero mem --json` snapshots pre/post run, diffing
`input_tokens` + `output_tokens` fields reported by `pulse` telemetry. Cold runs
incurs the boundary sweep cost (full repo classification); warm runs read only
delta from the cache and should trend toward zero tokens.

## Pass/Fail rubric

| Codepath | Token ceiling | Wall-time ceiling | Action if failed |
|---|---|---|---|
| small (this repo)         | ≤ 100 tok | ≤ 250 ms cold | Profile `process_start` + `store_open`; defer `mem` warmup. |
| synthetic 23k-file repo   | ≤ 100 tok | ≤ 600 ms cold | Move repo classification off the boot path; lazy on first read. |
| synthetic 100k-file repo  | ≤ 200 tok | ≤ 1500 ms cold | Stretch goal only — flag, do not block. |

## Open questions

- Does `tz://local/<ref>` carry enough cache state to skip re-classification
  on warm runs? (verify by comparing `first_expand` warm vs cold cell.)
- Is `mem` triggering the full RACC sweep, or only the index load?
- Synthetic line content: should mimic Rust source for realistic RACC
  classification (currently plain `line N`).
