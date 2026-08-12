# Cont-2 formal regression

Optional RADC Wave-5 Continuation 2 certificate pack. This is a **formal
regression only**. It does not gate product release, does not set a runtime
limit of 18 tools, and does not promote formal-gauge numbers into production
savings claims. See [docs/radc-non-claims.md](../../docs/radc-non-claims.md).

Theorem `W5-SOL-MDC-Q4-FULL-18-19` (DR+EC): the parity ledger dominates the
full no-recovery prefix hull on Theta4 down/cap at `(40,20)` iff `1 <= m <= 18`;
it fails for `m >= 19`. That motivates multi-demand / multi-expand cost
accounting. It is not a product cap.

Out of scope: BP1 general-n (still OPEN), arbitrary-n Cont-2, rewriting the
checkers in Rust.

## Reproduce

`/xtask/` is gitignored in this repo, so the tracked runner is:

```bash
python3 scripts/radc-check
```

That command:

1. Verifies every vendored file against `26_SOLPRO_CONT2_SHA256.txt`.
2. Re-runs the Python exact checker and requires the frozen PASS lines
   (`C_16(r)` table, `p10` fraction, m=17/18 margins, m>=19 obstruction).
3. Compiles and re-runs the independent C++ exact checker and requires the
   same PASS lines.

The default run also re-executes one Q4 subset-tree DP cell and checks the
frozen W4 split count `21457825` against [`formal/w4/SPLIT_COUNT.txt`](../w4/SPLIT_COUNT.txt).

`python3 scripts/radc-check --grid` additionally compiles `sol_m_demand_grid.cpp`
and compares the denominator-20 grid against `Q4_GRID20_FULL_DP.out`. That step
is supporting EC, not the product gate.

The original pack recipe is in `README_CONTINUATION_2.md` / `RUN_ALL.sh`.
The proof write-up is `RADC_W5_SOLPRO_CONTINUATION_2.md`.

## Optional CI

`.github/workflows/radc-check.yml` is `workflow_dispatch` only. It never runs
on push or pull_request and is not part of the product CI job.
