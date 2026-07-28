#!/usr/bin/env python3
"""W6 Tier-3 job M5: reduction attempt Kimi -> Fable, impossibility certificate.

Question: can PARITY-DUAL's ledgers be obtained from pi_EDC^2's ledger family
    M = 9 - p_c,  L = 11/2 - (3/2) p_c,  D = 0
by choosing theta (i.e. a p_c value), possibly after a timeline change?

Facts used (DR, elementary):
  (i)  p_c = sum theta_i^2 <= (sum theta_i)^2 = 1 on the simplex (cross terms >= 0);
  (ii) p_c = 1  <=>  sum theta_i (1 - theta_i) = 0  <=>  every theta_i in {0,1}
       <=>  theta is Dirac (excluded from every full-support polytope Theta_n^down,
       whose constraints theta_i >= 4/(5n) > 0 forbid it);
  (iii) on Theta_4^down, p_c <= 7/25 (vertex, certified in m3).

Equations (all exact):
  Kimi batch M=5:  9 - p_c = 5  <=>  p_c = 4      -> IMPOSSIBLE by (i).
  Kimi seq   M=8:  9 - p_c = 8  <=>  p_c = 1      -> Dirac only by (ii).
  Kimi L=4 (either): 11/2 - (3/2) p_c = 4  <=>  p_c = 1  -> Dirac only.
  Hypothetical batch-ified pi_EDC^2 (same candidate, 3-turn accounting)
      M = 2(1+h) + (1+q)(2 - p_c) = 6 - p_c at (h,q)=(1,0):
      6 - p_c = 5  <=>  p_c = 1  -> Dirac only.
  On Theta_4^down specifically (p_c <= 7/25):
      M(fable) in [218/25, 9) = [8.72, 9):  hits neither 8 (seq) nor 5 (batch).
"""
from fractions import Fraction as F

def p_c(theta):
    return sum(t * t for t in theta)

print("[EC] simplex bound p_c <= 1, exact grid check (denominator 24, full support):")
DEN = 24
mx = F(0)
cnt = 0
for a in range(1, DEN):
    for b in range(1, DEN - a):
        for c in range(1, DEN - a - b):
            d = DEN - a - b - c
            if d < 1:
                continue
            cnt += 1
            mx = max(mx, p_c((F(a, DEN), F(b, DEN), F(c, DEN), F(d, DEN))))
assert cnt > 0 and mx < 1, (mx, cnt)
print(f"  {cnt} full-support grid points: max p_c = {mx} < 1  PASS")

print("\n[EC] the four representation equations, solved over the rationals:")
eqs = [
    ("kimi batch M=5  : 9 - p_c = 5", F(9) - F(5), F(4)),
    ("kimi seq   M=8  : 9 - p_c = 8", F(9) - F(8), F(1)),
    ("kimi L=4        : 11/2 - (3/2) p_c = 4", (F(11, 2) - F(4)) / F(3, 2), F(1)),
    ("batch-ified fable M=5: 6 - p_c = 5", F(6) - F(5), F(1)),
]
for name, required, expect in eqs:
    assert required == expect
    feasible_full_support = required < 1
    print(f"  {name}  ->  p_c = {required}  "
          f"{'IMPOSSIBLE (>1)' if required > 1 else 'Dirac-only (=1)'}; "
          f"full-support feasible: {feasible_full_support}  PASS")

# Dirac characterization check: p_c = 1 <=> Dirac, exact on grid:
# among denominator-DEN grid points, p_c = 1 never occurs with all entries > 0.
# (grid already full support and max < 1 above).  Additionally verify the
# algebraic identity theta_i^2 <= theta_i with equality iff theta_i in {0,1}
# on rational sample points:
for t in (F(0), F(1), F(1, 3), F(2, 5), F(9, 10)):
    assert t * t <= t
    assert (t * t == t) == (t in (F(0), F(1)))
print("\n[EC] theta_i^2 <= theta_i, equality iff theta_i in {0,1}: sampled  PASS")

# On Theta_4^down (p_c <= 7/25 from m3): the reachable M interval:
lo_M = 9 - F(7, 25)
assert lo_M == F(218, 25)
assert lo_M > 8 and lo_M < 9
print(f"\n[EC] on Theta_4^down: M(fable) in [{lo_M}, 9) = [8.72, 9);")
print("  targets 8 (kimi seq) and 5 (kimi batch) are both OUTSIDE  PASS")

print("\nM5 RESULT: reduction Kimi -> Fable IMPOSSIBLE (EC+DR).")
print("Kimi's batch ledger needs p_c=4 (>1, off the simplex); Kimi's seq ledger")
print("and both L=4 need p_c=1 (Dirac, off every full-support polytope).  Even a")
print("batch-ified pi_EDC^2 meets M=5 only at p_c=1.  No reparameterization by")
print("theta, and no timeline change, represents PARITY-DUAL inside pi_EDC^2's")
print("ledger family.  QED.")
