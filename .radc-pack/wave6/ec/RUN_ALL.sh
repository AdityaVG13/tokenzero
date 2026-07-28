#!/usr/bin/env bash
set -euo pipefail
python3 W5_FULL_PREFIX_CHECKS.py | tee W5_FULL_PREFIX_CHECKS.reproduced.out
g++ -std=c++20 -O2 -Wall -Wextra -pedantic w5_full_prefix_check.cpp -o w5_full_prefix_check.reproduced
./w5_full_prefix_check.reproduced | tee w5_full_prefix_check.reproduced.out
g++ -std=c++20 -O2 -Wall -Wextra -pedantic sol_m_demand_grid.cpp -o sol_m_demand_grid.reproduced
: > Q4_GRID20_FULL_DP.reproduced.out
for w in "4 4 4 8" "4 4 5 7" "4 4 6 6" "4 5 5 6" "5 5 5 5"; do
  echo "weights $w" >> Q4_GRID20_FULL_DP.reproduced.out
  for m in $(seq 10 18); do
    printf 'm %s ' "$m" >> Q4_GRID20_FULL_DP.reproduced.out
    ./sol_m_demand_grid.reproduced 4 "$m" "$((m+1))" 40 1 $w >> Q4_GRID20_FULL_DP.reproduced.out
  done
done
printf 'PASS all continuation-2 checks\n'
