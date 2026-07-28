#!/usr/bin/env python3
"""W6 Tier-5 job B4(c): n=5 Q5-down BP1 audits on the subcube-tree family.
Cells = all 3^5 subcubes; splits along free coordinates only; exact Pareto
frontier of (L, E) per cell; max gain/depth ratio over the family vs s1=79/400.
NOTE: ball-code extremals are NOT subcubes, so this family is expected to fall
strictly below s1; the one-bit ball code is checked separately (b4_frontier.c).
"""
from fractions import Fraction
from math import comb

n = 5
N = 32
w = [9, 4, 4, 4, 4]
d = sum(w)  # 25

def subcubes():
    out = []
    for fix in range(32):
        sub = fix
        while True:
            out.append((fix, sub))
            if sub == 0:
                break
            sub = (sub - 1) & fix
    return out

def E_sub(fix, bits):
    free = n - bin(fix).count("1")
    size = 1 << free
    e = 0
    for i in range(n):
        bi = n - 1 - i  # bit position of coordinate i (i=0 heavy)
        if (fix >> bi) & 1:
            n1 = size if (bits >> bi) & 1 else 0
        else:
            n1 = size // 2
        e += w[i] * min(n1, size - n1)
    return e

cells = subcubes()
Ec = {c: E_sub(*c) for c in cells}
size_of = {c: 1 << (n - bin(c[0]).count("1")) for c in cells}
EOm = Ec[(0, 0)]

def pareto(pts):
    pts = sorted(set(pts))
    out = []
    bestE = None
    for L, E in pts:  # increasing L; keep only strict E improvements
        if bestE is None or E < bestE:
            out.append((L, E))
            bestE = E
    return out

front = {}
for sz in [1, 2, 4, 8, 16, 32]:
    for c in cells:
        if size_of[c] != sz:
            continue
        fix, bits = c
        pts = [(0, Ec[c])]
        for i in range(n):
            bi = n - 1 - i
            if (fix >> bi) & 1:
                continue
            c0 = (fix | (1 << bi), bits)
            c1 = (fix | (1 << bi), bits | (1 << bi))
            for l0, e0 in front[c0]:
                for l1, e1 in front[c1]:
                    pts.append((sz + l0 + l1, e0 + e1))
        front[c] = pareto(pts)

print("=" * 78)
print("B4c. n=5 Q5-down subcube-tree family: exact frontier audit")
print("=" * 78)
fOm = front[(0, 0)]
print(f"  Omega frontier size = {len(fOm)} points")
print("  (L, E) frontier of Omega (subcube trees only):")
for L, E in fOm:
    ratio = Fraction(EOm - E, d * L) if L else None
    t1 = Fraction(2 * d * L, EOm - E) if E < EOm else None
    print(f"    L={L:3d} E={E:3d}  ratio={(str(ratio) if ratio else '-'):10s} "
          f"t1contrib={(str(t1) if t1 else '-')}")
s1 = Fraction(79, 400)
best = max((Fraction(EOm - E, d * L) for L, E in fOm if L > 0 and E < EOm), default=None)
t1fam = min((Fraction(2 * d * L, EOm - E) for L, E in fOm if E < EOm), default=None)
print(f"  max ratio on subcube family = {best} = {float(best):.6f}")
print(f"  s1 (ball-code, non-subcube) = {s1} = {float(s1):.6f}")
print(f"  subcube family strictly below s1: {best < s1}")
print(f"  family first breakpoint t1 = {t1fam} = {float(t1fam):.6f} "
      f"(conjectured true t1(5) = 800/79 = {float(Fraction(800,79)):.6f})")
# depth-1 coordinate split point check
print("  depth-1 coordinate-split points:")
for i in range(n):
    bi = n - 1 - i
    c0 = (1 << bi, 0); c1 = (1 << bi, 1 << bi)
    E1 = Ec[c0] + Ec[c1]
    print(f"    split coord {i} (w={w[i]}): E={E1}, ratio={Fraction(EOm-E1, d*32)}")
print()
print("  Interpretation: on the subcube family the best slope is w_heavy/(2d) = 9/50")
print("  = 0.18 < 79/400 = 0.1975 (depth-1, heavy split). The ball code (non-subcube)")
print("  beats every subcube tree; consistent with the BP1 conjecture whose extremal")
print("  is the antipodal ball code. Refutation would require a NON-subcube tree")
print("  with ratio > 79/400; depth-2 ball-outer + random-outer families checked in C.")
