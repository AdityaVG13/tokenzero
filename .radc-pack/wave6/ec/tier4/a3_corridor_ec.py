#!/usr/bin/env python3
"""A3 EC-numeric: W5-SOL-AGRD-THETA-CORRIDOR audit.

Claims (float => EC-numeric):
 (i)   G_theta(D) = R_NR,theta(D) - f(D): G(0)=n-1, G(1/2)=0, strictly decreasing.
 (ii)  s < n-1 => unique D* in (0,1/2) with G(D*)=s (single sign change).
 (iii) gamma_L(D) = G(D)-s > 0 for D < D*.
 (iv)  uniform theta: R_NR(D) = n f(D), D* = H2^{-1}(1 - s/(n-1)).
 (v)   gamma_M chain: G(D) > s => gamma_M = 2R_NR - f - 2h - q > q + 2c + f(D) >= 0.
"""
import math

def H2(p):
    if p <= 0.0 or p >= 1.0: return 0.0
    return -(p*math.log2(p) + (1-p)*math.log2(1-p))
def f(d): return 1.0 - H2(d)

def R_NR_at_D(D, th):
    # invert D(mu) = sum th_i/(1+2^{mu th_i}) by bisection on mu in (0, 400]
    if D <= 0: return float(len(th))
    if D >= 0.5: return 0.0
    lo, hi = 1e-12, 400.0
    for _ in range(200):
        mu = (lo+hi)/2
        Dm = sum(t/(1.0+2.0**(mu*t)) for t in th)
        if Dm > D: lo = mu
        else: hi = mu
    mu = (lo+hi)/2
    return sum(f(1.0/(1.0+2.0**(mu*t))) for t in th)

def G(D, th): return R_NR_at_D(D, th) - f(D)

thetas = {"unif4": [.25]*4, "skew": [.4,.2,.2,.2], "mid": [.3,.3,.2,.2]}
for name, th in thetas.items():
    n = len(th)
    # (i) endpoints and strict decrease
    assert abs(G(1e-9, th) - (n-1)) < 1e-3
    assert abs(G(0.5, th)) < 1e-9
    prev = None
    for k in range(1, 200):
        D = 0.5*k/200.0
        g = G(D, th)
        if prev is not None: assert g < prev, "G not strictly decreasing"
        prev = g
    # (ii) uniqueness: exactly one sign change of G(D)-s for s < n-1
    for s in (0.5, 1.0, 2.0):
        if s >= n-1: continue
        signs = [1 if G(0.5*k/400.0, th) - s > 0 else -1 for k in range(1,400)]
        changes = sum(1 for a,b in zip(signs,signs[1:]) if a != b)
        assert changes == 1, f"nonunique root for s={s}"
        # root by bisection
        lo, hi = 1e-9, 0.5
        for _ in range(100):
            mid = (lo+hi)/2
            if G(mid, th) > s: lo = mid
            else: hi = mid
        Dstar = (lo+hi)/2
        # (iii) gamma_L > 0 below D*
        for k in range(1, 20):
            D = Dstar*k/20.0
            assert G(D, th) - s > 0
        # (v) gamma_M chain; s on BOTH sides must equal h+q+c. Use (h,q,c)=(s,0,0),
        # and also the registered (1,0,1) when s=2.
        for (h,q,c) in ([(s,0.0,0.0)] + ([(1.0,0.0,1.0)] if abs(s-2.0)<1e-12 else [])):
            assert abs(h+q+c - s) < 1e-12
            for k in range(1, 20):
                D = Dstar*k/20.0
                gM = 2*R_NR_at_D(D, th) - f(D) - 2*h - q
                assert gM > q + 2*c + f(D) - 1e-9 and gM > 0
        print(f"theta={name} s={s}: D*={Dstar:.6f}, unique root, gamma_L>0 below D*, gamma_M chain ok")

# (iv) uniform specialization, n in {2,4,8}
def H2_inv(y):
    lo, hi = 0.0, 0.5
    for _ in range(200):
        mid = (lo+hi)/2
        if H2(mid) < y: lo = mid
        else: hi = mid
    return (lo+hi)/2
for n in (2,4,8):
    th = [1.0/n]*n
    # R_NR(D) = n f(D) check
    for D in (0.05, 0.13, 0.3, 0.45):
        assert abs(R_NR_at_D(D, th) - n*f(D)) < 1e-6
    for s in (0.25, 1.0, n-1.5):
        if s <= 0 or s >= n-1: continue
        lo, hi = 1e-9, 0.5
        for _ in range(100):
            mid = (lo+hi)/2
            if G(mid, th) > s: lo = mid
            else: hi = mid
        Dstar = (lo+hi)/2
        Dref = H2_inv(1.0 - s/(n-1))
        assert abs(Dstar - Dref) < 1e-6
        print(f"n={n} s={s}: D*={Dstar:.6f} == H2^-1(1-s/(n-1))={Dref:.6f}  [uniform endpoint]")
print("PASS a3: corridor endpoint audit")
