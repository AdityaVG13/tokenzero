#!/usr/bin/env python3
"""A2 EC-numeric: audit W5-SOL-AGRD-WATERFILL (reproduces Cont-1 RD checks).

Claims under audit (float, math.log2 => EC-numeric):
 (i)   KKT logistic parameterization d_i(mu) = 1/(1+2^{mu theta_i}), f'(d_i) = -mu theta_i.
 (ii)  D(mu) strictly decreasing in mu (uniqueness of multiplier).
 (iii) envelope dR_NR/dD = -mu (exact parametric ratio).
 (iv)  strict advantage G = R_NR - f(D) > 0 for n>1, full support, D<1/2.
 (v)   Jensen chain sum f(d_i) > sum theta_i f(d_i) >= f(D).
 (vi)  G'(D) < 0 argument: d_i(mu) > 1/(1+2^mu) for 0<theta_i<1.
"""
import math

def H2(p):
    if p <= 0.0 or p >= 1.0: return 0.0
    return -(p*math.log2(p) + (1-p)*math.log2(1-p))
def f(d): return 1.0 - H2(d)
def fp(d): return math.log2(d/(1.0-d))
LN2 = math.log(2.0)

thetas = [[.25]*4, [.4,.2,.2,.2], [.3,.3,.2,.2]]
mus = [0.1, 0.5, 1, 2, 5, 10, 20]

minG = 1e9
for th in thetas:
    n = len(th)
    assert all(t > 0 for t in th) and abs(sum(th)-1) < 1e-12
    for mu in mus:
        ds = [1.0/(1.0+2.0**(mu*t)) for t in th]
        # (i) KKT stationarity
        for t,d in zip(th,ds):
            assert abs(fp(d) + mu*t) < 1e-9, "KKT fail"
        D = sum(t*d for t,d in zip(th,ds))
        R = sum(f(d) for d in ds)
        G = R - f(D)
        minG = min(minG, G)
        assert G > 0, "strict advantage fail"
        # (v) Jensen chain
        s_f  = sum(f(d) for d in ds)
        st_f = sum(t*f(d) for t,d in zip(th,ds))
        assert s_f > st_f >= f(D) - 1e-12
        # (vi) d_i(mu) > 1/(1+2^mu) since 0 < theta_i < 1
        base = 1.0/(1.0+2.0**mu)
        assert all(d > base for d in ds)
        # (iii) envelope: dR/dD = (dR/dmu)/(dD/dmu) = -mu
        dd  = [-t*LN2*d*(1-d) for t,d in zip(th,ds)]           # d d_i / d mu
        dR  = sum(fp(d)*v for d,v in zip(ds,dd))
        dD  = sum(t*v for t,v in zip(th,dd))
        assert abs(dR/dD + mu) < 1e-9, "envelope fail"
    # (ii) D(mu) strictly decreasing on a fine grid
    Dprev = None
    for k in range(1, 2000):
        mu = 20.0*k/1999.0
        D = sum(t/(1.0+2.0**(mu*t)) for t in th)
        if Dprev is not None: assert D < Dprev, "D(mu) not strictly decreasing"
        Dprev = D
    print(f"theta={th}: KKT ok, D(mu) strictly decreasing, envelope -mu exact, G>0")

print(f"min G over grid = {minG:.6f} > 0   (strict recovery advantage, all three thetas)")
print("RD checks: ok for all three thetas   [reproduces Cont-1 §3.6 expected line]")
print("PASS a2: water-filling audit")
