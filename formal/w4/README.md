# W4 subset-tree DP split-count

Optional formal hygiene next to the Cont-2 lane. Not a product gate.

Frozen figure: **21,457,825** subset-tree DP splits on the Q4 `n=4`
adaptive prefix hull (independent of `m` and of the five denominator-20
weight orbits). Both peers claimed this count; merge policy is to re-run
the DP once rather than pick a peer transcript.

Reproduce:

```bash
python3 scripts/radc-check
```

That compiles vendored `formal/cont2/sol_m_demand_grid.cpp` and runs one
representative cell (`n=4 m=10 alpha=11 rho=40/1 weights=4 4 4 8`). The
printed `splits` field must equal `SPLIT_COUNT.txt`. The same figure
appears on every line of `formal/cont2/Q4_GRID20_FULL_DP.out`.
`python3 scripts/radc-check --grid` re-runs the full 45-cell grid.

This does not block product release. The optional workflow
`.github/workflows/radc-check.yml` is `workflow_dispatch` only.
