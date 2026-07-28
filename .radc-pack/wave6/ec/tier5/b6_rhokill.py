#!/usr/bin/env python3
"""W6 Tier-5 job B6: rho_kill laws reconciliation + e_anti monotonicity EC."""
from fractions import Fraction
from math import comb

def e_anti(n):
    B = Fraction(sum(comb(n - 1, k) * max(0, 8 * k - 5 * n) for k in range(n)), 2 ** (n - 1))
    return Fraction(2 * (n - 1) - B, 5 * n)

print("=" * 78)
print("B6a. Crossing certificates and kill table")
print("=" * 78)
e7, e8 = e_anti(7), e_anti(8)
print(f"  e_anti(7) = {e7} ; vs 1/3: 3*145 = {3*145} < 448 -> e7 < 1/3: {e7 < Fraction(1,3)}")
print(f"  e_anti(8) = {e8} ; vs 1/3: 3*43 = {3*43} > 128 -> e8 > 1/3: {e8 > Fraction(1,3)}")
print(f"  margin e8 - 1/3 = {e8 - Fraction(1,3)} (claimed min margin 1/384 at n=8)")
print()
print("  n | e_anti | 4/e_anti (Fable one-bit component) | rho_kill full law (Kimi)")
for n in range(3, 13):
    e = e_anti(n)
    comp = 4 / e
    full = comp if n <= 7 else Fraction(12)
    full = max(comp, Fraction(12)) if n <= 7 else Fraction(12)
    print(f"  {n:2d} | {str(e):10s} | {str(comp):12s} | {full}")
print("  limit of 4/e_anti as n->inf: 4/(2/5) = 10 (antipodal branch only);")
print("  full kill = max(12, 4/e_anti) = 12 for n>=8; lim rho_kill = 12, NOT 10.")
print()
print("  zero-message witness (n>=8): L_b = 1 + rho/4 < 4  iff  rho < 12:")
for r in [11, 12, Fraction(1792,145)]:
    print(f"    rho={r}: L_b = {1 + Fraction(r)/4} (< 4 iff rho<12) -> {1 + Fraction(r)/4 < 4}")
print()
print("=" * 78)
print("B6b. e_anti > 1/3 for all n = 8..101 (Kimi EC claim), monotonicity n=8..20")
print("=" * 78)
min_margin, argmin = None, None
viol = []
prev = None
mono_ok = True
for n in range(8, 102):
    e = e_anti(n)
    mg = e - Fraction(1, 3)
    if mg <= 0:
        viol.append((n, e))
    if min_margin is None or mg < min_margin:
        min_margin, argmin = mg, n
    if n <= 20:
        if prev is not None and not e > prev:
            mono_ok = False
            print(f"  *** not strictly increasing at n={n}: {prev} -> {e}")
        prev = e
print(f"  violations of e_anti > 1/3 in n=8..101: {viol if viol else 'NONE'}")
print(f"  min margin = {min_margin} at n={argmin} (claim: 1/384 at n=8: {min_margin == Fraction(1,384) and argmin == 8})")
print(f"  strictly increasing on n=8..20: {mono_ok}")
print("  values n=8..20:")
for n in range(8, 21):
    print(f"    e_anti({n}) = {e_anti(n)} = {float(e_anti(n)):.6f}")
print()
print("Reconciliation (DR): Fable's 4/e_n^anti is the ONE-BIT COMPONENT: no one-bit")
print("no-recovery policy beats the candidate on M for rho >= 4/e_anti. Kimi's full")
print("law adds the zero-message L-witness: for rho<12, L_b = 1+rho/4 < 4 = L(candidate),")
print("so SOMETHING in the hull stays alive below 12 regardless of n; for n>=8 the")
print("one-bit branch 4/e_anti < 12 (since e_anti > 1/3) and the binding constraint")
print("is the zero-message line at 12. For 3<=n<=7, 4/e_anti >= 12 and the one-bit")
print("(antipodal) killer is binding. Consistent: rho_kill(n) = max(12, 4/e_anti(n)).")
