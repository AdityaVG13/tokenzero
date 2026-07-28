#!/usr/bin/env python3
"""W5 Part C: exact one-bit optimum at Theta_n^down heavy vertex, all n, via
(b,m)-class reduction. weights: heavy a=n+4, light 4, W=5n.
g(b,m) = 2^{n-1}(W - W_v) + 2^{n-(b+m)} h(b,m)."""
from math import comb
from fractions import Fraction as Fr

def classes(n):
    a = n + 4; W = 5*n
    out = {}
    for b in (0, 1):
        for m in range(0, n):
            if b == 0 and m == 0: continue
            Wv = a*b + 4*m
            h = 0
            for beta in ((0, 1) if b else (0,)):
                for k in range(m+1):
                    sig = a*beta + 4*k
                    h += comb(m, k) * min(sig, Wv - sig)
            g = (1 << (n-1))*(W - Wv) + (1 << (n - b - m)) * h
            out[(b, m)] = g
    return out

ok = True
print(" n  best(b,m)      g_anti      g_best   gap_to_runnerup   e1=g/(2^n*5n)")
for n in range(2, 61):
    out = classes(n)
    anti = out[(1, n-1)]
    best = min(out.values())
    argmin = sorted(bm for bm, g in out.items() if g == best)
    vals = sorted(out.values())
    gap = vals[1] - vals[0] if len(vals) > 1 else 0
    e1 = Fr(best, (1 << n)*5*n)
    flag = "OK" if (best == anti and argmin == [(1, n-1)]) else "***FAIL***"
    if n <= 12 or flag != "OK" or n in (16, 24, 32, 40, 48, 60):
        print(f"{n:3d}  {argmin}   {anti}   {best}   {gap}   {e1} = {float(e1):.6f} {flag}")
    if flag != "OK": ok = False
print(f"\nantipodal (b,m)=(1,n-1) is the UNIQUE class optimum for all n in 2..60: {ok}")

# closed-form e_anti check vs the sum formula
def e_anti(n):
    s = sum(comb(n-1, k)*min(4*k, 5*n-4*k) for k in range(n))
    return Fr(2*s, (1 << n)*5*n)
for n in (3, 4, 5, 6, 7, 8):
    print(f"  e_anti({n}) = {e_anti(n)}", "matches W4" if e_anti(n) in
          [Fr(1,4), Fr(11,40), Fr(121,400), Fr(5,16), Fr(145,448), Fr(43,128)] else "")

# structural data for the monotonicity proof: differences along m at b=1, and b=0 vs b=1
print("\n proof data: Delta_m(n) = g(1,m) - g(1,m+1) (want >= 0), and g(0,m)-g(1,m):")
for n in (3, 5, 8, 13, 21, 34, 55):
    out = classes(n)
    dm = [out[(1, m)] - out[(1, m+1)] for m in range(0, n-1)]
    db = [out[(0, m)] - out[(1, m)] for m in range(1, n)]
    print(f"  n={n}: min Delta_m = {min(dm)}, all >=0: {all(x >= 0 for x in dm)}; "
          f"min g(0,m)-g(1,m) = {min(db)}, all >0: {all(x > 0 for x in db)}")
