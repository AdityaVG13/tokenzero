#!/usr/bin/env python3
"""A4 EC-numeric: hybrid lossy+expand constructions (headline).

Models:
 RATE-SIDE: hybrid time-shares soft (rate 1-H2(D0), distortion D0, fraction beta=D/D0)
   with exact expand (rate cost rho_exp, distortion 0, fraction 1-beta).
   R_h(beta,D0) = beta*f(D0) + (1-beta)*rho_exp,  D = beta*D0.
 Claims:
  (1) Grok chord theorem (rho_exp=1): every hybrid has R >= f(D).  [EC verify on grid]
  (2) W6-DS-A4a sharp threshold: soft optimal at D iff rho_exp >= rho*(D) = 1+log2(1-D).
      Phi(D,D0) = (D0 f(D) - D f(D0))/(D0-D) strictly decreasing in D0, sup = limit at D0->D+.
  (3) W6-DS-A4b: rho_exp=0 (latency-charged expand) => optimal hybrid rate = 0
      (coin-flip hybrid D0=1/2, beta=2D); R_ag NOT optimal under that accounting.
  (4) closed form: for 0<=rho<=rho*(D): optimal D0* = 1-2^{rho-1},
      R_opt(D;rho) = rho - D*log2((1-D0*)/D0*).
 LEDGER-SIDE (Model H, carried-token accounting consistent with Cont-1 corridor and
  the EDC ledger at endpoints): m=1,
   M_NR=2+2R_NR(D),      L_NR=1+R_NR(D)
   M_RA=2+f(D)+2h+q,     L_RA=1+f(D)+s          (s=h+q+c)
   M_H =2+2R_NR(D0)+a(1+2h+q),  L_H=1+R_NR(D0)+a(1+s),   D=(1-a)D0
  Coin-flip hybrid CF (D0=1/2, a=1-2D):
   dM(CF-RA)=H2(D)-2D(1+2h+q), dL(CF-RA)=H2(D)-2D(1+s).
  (5) W6-DS-A4c: unique crossover Ddagger: RA dominates CF below, CF dominates RA above.
  (6) corridor-endpoint fragment: uniform theta, s<n-1, D*=H2^{-1}(1-s/(n-1)):
      RA at D* dominated by CF iff 1-s/(n-1) < 2*min(1+s,1+2h+q)*H2^{-1}(1-s/(n-1)).
"""
import math

def H2(p):
    if p <= 0.0 or p >= 1.0: return 0.0
    return -(p*math.log2(p) + (1-p)*math.log2(1-p))
def f(d): return 1.0 - H2(d)
def H2_inv(y):
    lo,hi=0.0,0.5
    for _ in range(200):
        mid=(lo+hi)/2
        if H2(mid)<y: lo=mid
        else: hi=mid
    return (lo+hi)/2

print("== (1) Grok chord theorem rho_exp=1: hybrid rate >= f(D) on grid ==")
worst = 1e9
for iD in range(1,20):
    D0 = 0.5*iD/20.0
    for ib in range(1,20):
        beta = ib/20.0
        D = beta*D0
        R = beta*f(D0) + (1-beta)*1.0
        worst = min(worst, R - f(D))
        assert R >= f(D) - 1e-12
print(f"min [R_hybrid - f(D)] = {worst:.6f} >= 0   [W6-GROK-AG-HYBRID-TV confirmed]")
print(f"point certificate: H2(1/4) = {H2(0.25):.6f} > 1/2")

print("== (2) sharp threshold rho*(D) = 1 + log2(1-D) ==")
def Phi(D,D0): return (D0*f(D) - D*f(D0))/(D0-D)
maxerr = 0.0
for iD in range(1,25):
    D = 0.5*iD/25.0
    # Phi strictly decreasing in D0
    prev=None
    for k in range(1,60):
        D0 = D + (0.5-D)*k/60.0
        v = Phi(D,D0)
        if prev is not None: assert v < prev + 1e-12, "Phi not decreasing"
        prev=v
    lim = Phi(D, D+1e-7)
    thr = 1.0 + math.log2(1.0-D)
    maxerr = max(maxerr, abs(lim-thr))
    # verify: rho slightly below threshold => improving hybrid exists; at/above => none
    import random
    random.seed(1)
    for rho in (thr-0.05, thr+0.05):
        best = min( (D/D0)*f(D0) + (1-D/D0)*rho for D0 in [D+(0.5-D)*k/200.0 for k in range(1,201)] )
        if rho < thr: assert best < f(D) - 1e-9, "expected improving hybrid"
        else: assert best >= f(D) - 1e-9, "expected soft optimal"
print(f"sup Phi == 1+log2(1-D): max |err| = {maxerr:.2e}; above/below-threshold behavior certified on grid")

print("== (3) rho_exp=0 collapse: optimal hybrid rate = 0 ==")
for D in (0.05, 0.2, 0.4, 0.499):
    beta = 2*D  # D0 = 1/2 coin flip
    R = beta*f(0.5) + (1-beta)*0.0
    print(f"D={D}: coin-flip hybrid rate R={R} vs R_ag=f(D)={f(D):.6f}")
    assert R == 0.0 and f(D) > 0

print("== (4) closed-form optimal hybrid for 0 <= rho <= rho*(D) ==")
for D in (0.1, 0.25, 0.4):
    thr = 1.0 + math.log2(1.0-D)
    for rho in (0.0, 0.25*thr, 0.5*thr, 0.9*thr):
        D0s = 1.0 - 2.0**(rho-1.0)
        Rclosed = rho - D*math.log2((1.0-D0s)/D0s)
        Rgrid = min( (D/D0)*f(D0) + (1-D/D0)*rho for D0 in [D+(0.5-D)*k/400.0 for k in range(1,401)] )
        assert abs(Rclosed-Rgrid) < 1e-4, (D,rho,Rclosed,Rgrid)
    print(f"D={D}: rho*(D)={thr:.6f}; closed form R_opt(D;rho)=rho-D*log2((1-D0*)/D0*), D0*=1-2^(rho-1) matches grid (1e-4)")

print("== (5) ledger Model H: coin-flip hybrid vs recovery-aware soft, (h,q,c)=(1,0,1) ==")
h,q,c = 1.0,0.0,1.0
s = h+q+c
assert 1+s == 1+2*h+q  # both 3 here: single crossover
# crossover Ddagger: H2(D) = 2D*3
lo,hi=1e-9,0.5
for _ in range(100):
    mid=(lo+hi)/2
    if H2(mid) > 6*mid: lo=mid
    else: hi=mid
Dd=(lo+hi)/2
print(f"Ddagger solving H2(D)=2D(1+s)=6D: {Dd:.6f}")
for D in (0.01, 0.02, 0.1, 0.2, 0.35):
    dM = H2(D) - 2*D*(1+2*h+q)
    dL = H2(D) - 2*D*(1+s)
    rel = "CF dominates RA" if (dM<0 and dL<0) else ("RA dominates CF" if (dM>0 and dL>0) else "incomparable")
    print(f"D={D}: dM(CF-RA)={dM:+.6f} dL(CF-RA)={dL:+.6f}  -> {rel}")
    if D < Dd: assert dM>0 and dL>0
    else: assert dM<0 and dL<0

print("== (6) corridor-endpoint fragment: uniform theta, RA at D* vs CF ==")
for n in (2,4,8):
    for s_ in (0.5, 1.0, 2.0, 2.5):
        if s_ >= n-1: continue
        Dstar = H2_inv(1.0 - s_/(n-1))
        lhs = 1.0 - s_/(n-1)         # H2(D*)
        rhs = 2.0*min(1+s_, 1+2*h+q)*Dstar
        dom = "CF dominates RA at D*" if lhs < rhs else ("RA dominates CF at D*" if lhs > 2.0*max(1+s_,1+2*h+q)*Dstar else "split margins (incomparable)")
        print(f"n={n} s={s_}: D*={Dstar:.6f}  H2(D*)={lhs:.6f}  2*min(1+s,1+2h+q)*D*={rhs:.6f}  -> {dom}")

print("== (7) full hybrid frontier scan, n=4 uniform, (h,q,c)=(1,0,1): does ANY hybrid dominate RA? ==")
def R_NR_unif4(D0): return 4.0*f(D0)
for D in (0.02, 0.05, 0.1, 0.2, 0.3, 0.45):
    M_RA = 2 + f(D) + 2*h + q
    L_RA = 1 + f(D) + s
    found = False
    for k in range(1, 200):
        D0 = D + (0.5-D)*k/200.0
        a = 1.0 - D/D0
        M_H = 2 + 2*R_NR_unif4(D0) + a*(1+2*h+q)
        L_H = 1 + R_NR_unif4(D0) + a*(1+s)
        if (M_H <= M_RA and L_H < L_RA) or (M_H < M_RA and L_H <= L_RA):
            found = True; break
    print(f"D={D}: RA (M={M_RA:.4f},L={L_RA:.4f}); exists hybrid dominating RA on (M,L): {found}")
print("PASS a4: hybrid audit + threshold theorem")
