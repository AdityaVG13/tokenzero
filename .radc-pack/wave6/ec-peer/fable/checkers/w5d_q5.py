#!/usr/bin/env python3
"""W5 Part D: Q5 restricted-class kill lines at Theta_5^down vertex."""
from fractions import Fraction as Fr
from itertools import combinations
n = 5; w = (9, 4, 4, 4, 4); d = 25; N = 1 << n
Dst = [[sum(w[i] for i in range(n) if ((x ^ p) >> i) & 1) for p in range(N)] for x in range(N)]

# 1-bit: all prototype pairs (includes p=q)
best1 = None; arg1 = None
for p in range(N):
    for q in range(p, N):
        tot = sum(min(Dst[x][p], Dst[x][q]) for x in range(N))
        if best1 is None or tot < best1: best1, arg1 = tot, (p, q)
e1 = Fr(best1, N*d)
print(f"1-bit optimum: E={best1}, e1={e1}  (antipodal 121/400? {e1==Fr(121,400)}, pair={arg1}, antipodal={set(arg1)=={0,N-1}})")
print(f"  1-bit kill line 4 + t*e1 hits 8 at t = {Fr(4)/e1} = {float(Fr(4)/e1):.4f}")

# count ties among all pairs
ties = [pq for p in range(N) for q in range(p, N)
        if sum(min(Dst[x][p], Dst[x][q]) for x in range(N)) == best1 for pq in [(p,q)]]
print(f"  optimal pairs: {ties}")

# 2-bit: 4 prototypes
best2 = None; arg2 = None
for quad in combinations(range(N), 4):
    tot = 0
    for x in range(N):
        tot += min(Dst[x][quad[0]], Dst[x][quad[1]], Dst[x][quad[2]], Dst[x][quad[3]])
    if best2 is None or tot < best2: best2, arg2 = tot, quad
e2 = Fr(best2, N*d)
print(f"2-bit optimum: E={best2}, e2={e2}={float(e2):.5f}, quad={arg2} "
      f"= {[format(x,'05b') for x in arg2]}")
print(f"  2-bit line 6 + t*e2 hits 8 at t = {Fr(2)/e2} = {float(Fr(2)/e2):.4f}")

# mixed depth: one leaf at depth 1, two at depth 2: minimize over prototypes for scalar t:
# unnormalized scaled objective sum_x min(2*1*d*tden + tnum*D[x][p1], 2*2*d*tden + tnum*D[x][p2/3])
def mixed_best(t):
    tn, td = t.numerator, t.denominator
    best = None; argb = None
    for p1 in range(N):
        c1col = [2*1*d*td + tn*Dst[x][p1] for x in range(N)]
        for p2 in range(N):
            c2col = [2*2*d*td + tn*Dst[x][p2] for x in range(N)]
            for p3 in range(p2, N):
                tot = 0
                for x in range(N):
                    c3 = 2*2*d*td + tn*Dst[x][p3]
                    c = c1col[x]
                    if c2col[x] < c: c = c2col[x]
                    if c3 < c: c = c3
                    tot += c
                if best is None or tot < best: best, argb = tot, (p1, p2, p3)
    return best, argb

# value F_pol(t) = 2 + best/(N*d*tden); find whether any mixed policy stays < 8 beyond 1600/121
for t in (Fr(1600,121), Fr(27,2), Fr(14), Fr(29,2), Fr(15)):
    b, ar = mixed_best(t)
    val = Fr(2) + Fr(b, N*d*t.denominator)
    print(f"mixed(1+2+2) at t={t}: policy value={val}={float(val):.5f}  protos={[format(x,'05b') for x in ar]}  "
          f"{'<8 KILLS' if val < 8 else '>=8'}")

# pure policies comparison at the exact previous kill threshold 1600/121:
t0 = Fr(1600, 121)
print(f"\nat t=1600/121={float(t0):.4f}: 1-bit value = {4 + t0*e1} (should be exactly 8)")
print(f"  2-bit value = {6 + t0*e2} = {float(6 + t0*e2):.5f}")
# best kill threshold from enumerated families: last t where min(family values) < 8
# 1-bit: t < 4/e1 = 1600/121 ~ 13.22 ; 2-bit: 2/e2 ; mixed: numeric scan
print(f"\n2-bit kill extends to t = 2/e2 = {Fr(2)/e2} = {float(Fr(2)/e2):.4f}"
      f"  -> improved rho_kill lower bound if > 1600/121={float(Fr(1600,121)):.4f}")
