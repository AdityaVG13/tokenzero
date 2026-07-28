#!/usr/bin/env python3
"""W6 Tier-5 job B5: split-gain density obstruction audit.
gamma(A) = max over bipartitions B|C of A of E(A)-E(B)-E(C);
mass-density = gamma/(d*|A|). Verify max density = 1/2, attained ONLY at
antipodal pairs, in the five BP1 classes; full distribution for n=5 (|A|<=4).
"""
from fractions import Fraction
from math import comb
from itertools import combinations

CLASSES = [
    ("Q3-down",    3, [7, 4, 4]),
    ("Q3-uniform", 3, [5, 5, 5]),
    ("Q4-down",    4, [8, 4, 4, 4]),
    ("Q4-cap",     4, [6, 6, 4, 4]),
    ("Q4-uniform", 4, [5, 5, 5, 5]),
    ("Q5-down",    5, [9, 4, 4, 4, 4]),
]

def col_masks(n):
    N = 2 ** n
    return [sum(1 << x for x in range(N) if (x >> (n - 1 - i)) & 1) for i in range(n)]

def E_of(A, sz, cm, w):
    e = 0
    for i in range(len(w)):
        c1 = bin(A & cm[i]).count("1")
        e += w[i] * min(c1, sz - c1)
    return e

def max_gain(A, sz, cm, w, Ecache):
    """max over bipartitions of E(A)-E(B)-E(C), and argmax."""
    low = A & (-A)
    EA = Ecache(A, sz)
    best, bestB = -1, None
    B = (A - 1) & A
    while B:
        if B & low:
            C = A ^ B
            g = EA - Ecache(B, bin(B).count("1")) - Ecache(C, bin(C).count("1"))
            if g > best:
                best, bestB = g, B
        B = (B - 1) & A
    return best, bestB

print("=" * 78)
print("B5. Split-gain density: max over ALL subsets (n=3,4) / |A|<=4 (n=5)")
print("=" * 78)
for name, n, w in CLASSES:
    N = 2 ** n
    d = sum(w)
    cm = col_masks(n)
    full = (1 << N) - 1
    from functools import lru_cache
    @lru_cache(maxsize=None)
    def Ecache(A, sz):
        return E_of(A, sz, cm, w)
    maxsize = N if n <= 4 else 4
    # antipodal map on SOURCES (flip the n coordinate bits)
    srcfull = (1 << n) - 1
    anti = {x: srcfull ^ x for x in range(N)}
    top = Fraction(-1)
    top_sets = []
    hist = {}
    second = Fraction(-1)
    for sz in range(2, maxsize + 1):
        for xs in combinations(range(N), sz):
            A = sum(1 << x for x in xs)
            g, _ = max_gain(A, sz, cm, w, Ecache)
            den = Fraction(g, d * sz)
            hist[den] = hist.get(den, 0) + 1
            if den > top:
                top, top_sets = den, [frozenset(xs)]
            elif den == top:
                top_sets.append(frozenset(xs))
    anti_pairs = {frozenset((x, anti[x])) for x in range(N)}
    anti_pairs = {p for p in anti_pairs if len(p) == 2}
    is_only_anti = set(top_sets) == anti_pairs
    distinct = sorted(hist.keys(), reverse=True)
    print(f"  {name} (d={d}): max density = {top} ; #attaining sets = {len(top_sets)} "
          f"; exactly the {len(anti_pairs)} antipodal pairs: {is_only_anti}")
    print(f"    next-highest distinct densities: {[str(x) for x in distinct[1:4]]}")
    print(f"    #distinct density values = {len(hist)}")
    if name == "Q5-down":
        print("    Q5-down full histogram (density: count), sizes 2..4 combined:")
        for k in sorted(hist.keys(), reverse=True):
            print(f"      {k}  ({float(k):.6f}) : {hist[k]}")

print()
print("Size-2 closed form (DR): E({x,y}) = sum_{i: x_i != y_i} w_i, split into")
print("singletons gains all of it; density = (diff weight)/(2d) <= 1/2 with equality")
print("iff x,y differ in all n coordinates, i.e. antipodal. EC above confirms.")
