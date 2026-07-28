# RADC Wave-5 Continuation 2 certificate pack

This pack closes the full Q4 sequential prefix-hull claim at the registered gauge.

## Reproduce

```bash
python3 W5_FULL_PREFIX_CHECKS.py

g++ -std=c++20 -O2 -Wall -Wextra -pedantic \
  w5_full_prefix_check.cpp -o w5_full_prefix_check
./w5_full_prefix_check

g++ -std=c++20 -O2 -Wall -Wextra -pedantic \
  sol_m_demand_grid.cpp -o sol_m_demand_grid

for w in "4 4 4 8" "4 4 5 7" "4 4 6 6" "4 5 5 6" "5 5 5 5"; do
  for m in $(seq 10 18); do
    ./sol_m_demand_grid 4 "$m" "$((m+1))" 40 1 $w
  done
done
```

The proof itself is in `RADC_W5_SOLPRO_CONTINUATION_2.md`. The denominator-20 grid run is independent supporting EC; the continuum theorem follows from the coverage-leaf and prefix-length lemmas.
