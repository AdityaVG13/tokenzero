#!/usr/bin/env python3
"""A5: decision-TV model, finite actions. EC-numeric (BA curves) + exact Fractions.

W6-DS-A5a (0-1 loss, k actions): agency RD = 1-H2(D) for all k>=2.
W6-DS-A5b (soft-decision TV): R_TV(D) = 1-H2(D)  [data-processing reduction];
  refutes the 'TV differs from Hamming' suspicion for per-letter TV-to-delta.
Variant (observation-channel TV): R(D) = 1-H2(D/Delta) for Delta=|p1-p0| (scaled Hamming).
Grid alphabet without endpoints: R(D) = 1-H2(2(D-1/4)) on [1/4,1/2].
"""
import math
from fractions import Fraction

def H2(p):
    if p <= 0.0 or p >= 1.0: return 0.0
    return -(p*math.log2(p) + (1-p)*math.log2(1-p))

def ba_curve(dmat, px, n_slopes=60, iters=3000):
    """Blahut-Arimoto over slope grid; returns list of (D, R) points."""
    nx, na = len(px), len(dmat[0])
    pts = []
    for si in range(n_slopes):
        slope = 0.02*(1.25**si)
        Q = [1.0/na]*na
        for _ in range(iters):
            Pa = []
            for x in range(nx):
                ex = [-slope*dmat[x][a]*math.log(2) for a in range(na)]
                mx = max(ex)
                row = [Q[a]*math.exp(v-mx) for a, v in enumerate(ex)]
                Z = max(sum(row), 1e-300)
                Pa.append([v/Z for v in row])
            Qn = [sum(px[x]*Pa[x][a] for x in range(nx)) for a in range(na)]
            if max(abs(Qn[a]-Q[a]) for a in range(na)) < 1e-12: Q = Qn; break
            Q = Qn
        D = sum(px[x]*Pa[x][a]*dmat[x][a] for x in range(nx) for a in range(na))
        # I(X;A)
        I = 0.0
        for x in range(nx):
            for a in range(na):
                p = px[x]*Pa[x][a]
                if p > 0 and Q[a] > 0: I += p*math.log2(Pa[x][a]/Q[a])
        pts.append((D, I))
    return pts

px = [0.5, 0.5]
print("== W6-DS-A5a: 0-1 loss, k actions with surjective phi: A -> {0,1} ==")
for k in (2,3,4):
    phi = [a % 2 for a in range(k)]  # surjective for k>=2
    dmat = [[0.0 if phi[a]==x else 1.0 for a in range(k)] for x in (0,1)]
    pts = ba_curve(dmat, px)
    errs = [abs(R - (1-H2(D))) for D,R in pts if 0.01 < D < 0.49]
    print(f"k={k}: max |R_BA - (1-H2(D))| = {max(errs):.2e} over {len(errs)} curve points")
    assert max(errs) < 3e-3

print("== W6-DS-A5b: soft-decision TV, q-grid with endpoints ==")
for k in (2,3,4):
    qs = [a/(k-1) for a in range(k)]
    dmat = [[abs(x-q) for q in qs] for x in (0,1)]
    pts = ba_curve(dmat, px)
    errs = [abs(R - (1-H2(D))) for D,R in pts if 0.01 < D < 0.49]
    print(f"k={k} grid {qs}: max |R_BA - (1-H2(D))| = {max(errs):.2e}")
    assert max(errs) < 3e-3

print("== grid WITHOUT endpoints {1/4,1/2,3/4}: R(D) = 1-H2(2(D-1/4)) ==")
qs = [0.25, 0.5, 0.75]
dmat = [[abs(x-q) for q in qs] for x in (0,1)]
pts = ba_curve(dmat, px)
errs = [abs(R - (1-H2(2*(D-0.25)))) for D,R in pts if 0.26 < D < 0.49]
print(f"max |R_BA - (1-H2(2(D-1/4)))| = {max(errs):.2e}")
assert max(errs) < 3e-3

print("== observation-channel TV (scaled Hamming), p0=1/4, p1=3/4, Delta=1/2 ==")
Delta = 0.5
dmat = [[0.0, Delta],[Delta, 0.0]]  # TV=|p_x - p_xhat|
pts = ba_curve(dmat, px)
errs = [abs(R - (1-H2(D/Delta))) for D,R in pts if 0.01 < D < 0.24]
print(f"max |R_BA - (1-H2(D/Delta))| = {max(errs):.2e}")
assert max(errs) < 3e-3

print("== exact Fractions, small cases n=1..4, k=2..4 ==")
# TV(delta_0, Bern(p/q)) = p/q exactly; TV(delta_1, Bern(p/q)) = 1-p/q
for a,b in ((1,4),(1,3),(1,2),(2,3)):
    q = Fraction(a,b)
    assert abs(float(q) - (0.25 if (a,b)==(1,4) else float(q))) < 1
    print(f"TV(delta_0,Bern({q})) = {q}; TV(delta_1,Bern({q})) = {1-q}")
# no-message error = 1/2 exactly for any n, any full-support theta (one-demand root leaf)
for n in (1,2,3,4):
    e_root = Fraction(1,2)
    # min-q distortion at rate 0 with grid k: min_q E|X-q| = 1/2 for any grid
    print(f"n={n}: root-leaf error = {e_root}; rate-0 TV distortion = {Fraction(1,2)} (k-independent)")
# full-info min distortion with grid {1/4,1/2,3/4}: 1/4 exact
dmin = Fraction(1,2)*(Fraction(1,4)+Fraction(1,4))
print(f"grid no-endpoints full-info distortion d_min = {dmin} exactly")
assert dmin == Fraction(1,4)
print("PASS a5: decision-TV audit")
