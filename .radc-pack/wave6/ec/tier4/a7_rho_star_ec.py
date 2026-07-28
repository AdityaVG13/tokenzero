#!/usr/bin/env python3
"""A7: corridor rho*(s) map audit. EXACT Fractions.

Kimi W5-SMC-3: rho*(h,q,c) = max{4+4s, 20s/3, 80(s-1)/7} for s = h+q+c <= 3, +inf beyond (obstruction s=3).
Audit:
 (i)   branch structure: breakpoints s=3/2 (L1=L2), s=12/5 (L2=L3); subordinate crossing 27/13.
 (ii)  reproduces W4-PHASE-Q4-H: max{8+4h, (20/3)(1+h), 80h/7} == map(1+h).
 (iii) registered landmarks on the half-integer grid:
       s=1/2 -> 6 = rho_M; s=2 -> 40/3 = rho_RADC = rho*_cap; s=5/2 -> 120/7 = rho_advertised;
       s=3 -> 160/7 = rho_identity (obstruction boundary).
 (iv)  class thresholds are evaluations of the map: 64/5 at s=48/25; 160/11 at s=24/11;
       16 at s=12/5 (kink); 135/8 at s=317/128.
"""
from fractions import Fraction as F

def rho_star(s):
    if s > 3: return None  # +inf obstruction
    return max(4+4*s, F(20,3)*s, F(80,7)*(s-1))

print("== (i) branch structure ==")
# L1 = 4+4s, L2 = 20s/3, L3 = 80(s-1)/7
b12 = F(3,2)   # L1==L2: 4+4s = 20s/3 => 12+12s = 20s => s=3/2
assert 4+4*b12 == F(20,3)*b12 == 10
b23 = F(12,5)  # L2==L3: 140s = 240(s-1) => s=12/5
assert F(20,3)*b23 == F(80,7)*(b23-1) == 16
b13 = F(27,13) # L1==L3 subordinate
assert 4+4*b13 == F(80,7)*(b13-1)
assert F(20,3)*b13 > 4+4*b13, "branch2 dominates at 27/13"
print(f"L1==L2 at s=3/2 (value 10); L2==L3 at s=12/5 (value 16); L1==L3 at 27/13 subordinate (L2 larger)")
# verify active branches on samples
for s, expect in ((F(0), F(4)), (F(1), F(8)), (F(7,4), F(35,3)), (F(11,4), F(140,7))):
    assert rho_star(s) == expect
print("active branch check: s<=3/2 -> 4+4s; 3/2<=s<=12/5 -> 20s/3; 12/5<=s<=3 -> 80(s-1)/7  ok")

print("== (ii) W4-PHASE-Q4-H reproduction ==")
for h in (F(0), F(1,4), F(1,2), F(1), F(3,2), F(2)):
    w4 = max(8+4*h, F(20,3)*(1+h), F(80,7)*h)
    assert rho_star(1+h) == w4
    print(f"h={h}: W4 curve {w4} == map(1+h) = {rho_star(1+h)}")

print("== (iii) rho*(s) table on requested grid ==")
landmarks = {F(1,2): ("rho_M", F(6)), F(2): ("rho_RADC = rho*_cap", F(40,3)),
             F(5,2): ("rho_advertised", F(120,7)), F(3): ("rho_identity", F(160,7))}
for s in (F(0), F(1,2), F(1), F(3,2), F(2), F(5,2), F(3)):
    v = rho_star(s)
    tag = ""
    if s in landmarks:
        name, val = landmarks[s]
        assert v == val
        tag = f"  == {name} ({val})"
    print(f"s={str(s):>4}: rho* = {str(v):>7} = {float(v):.6f}{tag}")
assert rho_star(F(7,2)) is None
print("s=7/2 > 3: +inf (obstruction; infeasible beyond s=3)")

print("== (iv) class thresholds as map evaluations ==")
for thr, name in ((F(64,5),"Q4u"), (F(160,11),"Q4d"), (F(16),"Q3u"), (F(135,8),"Q3d"), (F(40,3),"Q4cap")):
    sols = []
    for branch, dom in ((lambda s: 4+4*s, (F(0), F(3,2))), (lambda s: F(20,3)*s, (F(3,2), F(12,5))),
                        (lambda s: F(80,7)*(s-1), (F(12,5), F(3)))):
        lo, hi = dom
        # linear solve by sample: find s with branch(s)=thr in domain
        # branch(s) = a*s + b
        a = branch(F(1))-branch(F(0)); b = branch(F(0))
        s = (thr-b)/a
        if lo <= s <= hi and rho_star(s) == thr:
            sols.append(s)
    print(f"{name} threshold {thr} = {float(thr):.4f} attained at s = {sols}")
print("PASS a7: rho*(s) map audit")
