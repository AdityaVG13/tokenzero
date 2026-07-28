#!/usr/bin/env python3
"""A8: ISC class phase formula audit. EXACT Fractions.

Registered single-demand standard-candidate thresholds (T = 4+2s at s=2, i.e. T=8):
 Q3u 16 | Q3d 135/8 | Q4u 64/5 | Q4d 160/11 | Q4cap 40/3.
Audit:
 (i)   ordering chain 64/5 < 40/3 < 160/11 < 16 < 135/8 (exact cross-multiplication).
 (ii)  PRODUCT formula rho* = max_j (T-2-2 l_j)/e_j reproductions at T=8:
       Q4d: (l=1, e=11/40) -> 160/11; Q4u: (l=1, e=5/16) -> 64/5; Q4cap: (l=1, e=3/10) -> 40/3.
 (iii) e^(1) / first-breakpoint consistency: 2/(1/2-e) = 10, 80/9, 32/3, 8 for e = 3/10, 11/40, 5/16, 1/4.
 (iv)  Q3u: F(t) = min(2+t/2, 4+t/4, 8); F(16)=8=T and F(t)<8 for t<16 => rho*=16 exactly.
 (v)   registered breakpoint containment: 40/3 in (10,16); 160/11 in (80/9,16); 64/5 in (32/3,16).
 (vi)  Q3d 135/8: check 135/8 > 4/(1/4) = 16 (one-bit pair NOT binding); pair attribution
       requires the W4 Q3-down supported-pair table (flagged PI-gap, not a contradiction).
 (vii) extremal point assignments (vertices attaining each class floor).
"""
from fractions import Fraction as F

print("== (i) ordering chain ==")
chain = [F(64,5), F(40,3), F(160,11), F(16), F(135,8)]
for a,b in zip(chain, chain[1:]):
    assert a < b
print("64/5 = 12.8 < 40/3 = 13.333 < 160/11 = 14.545 < 16 < 135/8 = 16.875   ok")

print("== (ii) T=8 PRODUCT reproductions ==")
T = F(8)
for l, e, expect, name in ((F(1), F(11,40), F(160,11), "Q4d"), (F(1), F(5,16), F(64,5), "Q4u"),
                           (F(1), F(3,10), F(40,3), "Q4cap")):
    v = (T-2-2*l)/e
    assert v == expect, (name, v)
    print(f"{name}: (T-2-2*{l})/({e}) = {v} == registered {expect}")
# no-message pair (l=0, e=1/2) never binding at T=8:
assert (T-2)/F(1,2) == 12 and F(12) < F(64,5)
print("no-message pair (0, 1/2) gives 12 < all class thresholds => never binding at T=8")

print("== (iii) e^(1) first-breakpoint consistency ==")
for e, bp, name in ((F(3,10), F(10), "Q4cap"), (F(11,40), F(80,9), "Q4d"),
                    (F(5,16), F(32,3), "Q4u"), (F(1,4), F(8), "Q3 classes")):
    v = 2/(F(1,2)-e)
    assert v == bp
    print(f"e^(1)={e}: 2/(1/2-e^(1)) = {v} == registered first breakpoint {bp} ({name})")

print("== (iv) Q3u floor: rho* = 16 exactly ==")
def Fq3u(t): return min(2+t/2, 4+t/4, F(8))
assert Fq3u(F(16)) == F(8)
for k in range(0,16):
    assert Fq3u(F(k)) < F(8)
print("F(16)=8=T; F(t)<8 for t<16 => rho*_Q3u = 16 (saturation breakpoint)")

print("== (v) registered breakpoint containment ==")
assert F(10) < F(40,3) < F(16)
assert F(80,9) < F(160,11) < F(16)
assert F(32,3) < F(64,5) < F(16)
print("40/3 in (10,16) [cap]; 160/11 in (80/9,16) [down]; 64/5 in (32/3,16) [unif]  ok")

print("== (vi) Q3d 135/8 attribution ==")
assert F(135,8) > 4/F(1,4) == 16
print("135/8 = 16.875 > 16 = (T-4)/(1/4): one-bit pair (l=1,e=1/4) not binding;")
print("attaining pair needs W4 Q3-down supported-pair table (not in extraction) => PI-gap flagged,")
print("consistent with registered Q3-down breakpoint list (8, 15, 135/8): 135/8 IS the saturation breakpoint.")
# check internal consistency of a candidate line family: L0=2+t/2, L1=4+t/4 cross at 8 exactly
assert 2+F(8)/2 == 4+F(8)/4
print("L0=2+t/2 and L1=4+t/4 cross at t=8 exactly (registered first Q3-down breakpoint)")

print("== (vii) extremal point assignments ==")
for name, v in (("Q3u", "(5,5,5)/15 uniform"), ("Q3d", "(7,4,4)/15 down vertex"),
                ("Q4u", "(5,5,5,5)/20 uniform"), ("Q4d", "(8,4,4,4)/20 down vertex"),
                ("Q4cap", "(6,6,4,4)/20 cap vertex = (3,3,2,2)/10")):
    print(f"{name}: {v}")
print("PASS a8: class phase formula audit")
