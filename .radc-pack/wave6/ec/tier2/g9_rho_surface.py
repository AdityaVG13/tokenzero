#!/usr/bin/env python3
"""G9: alternate-gauge probe. n=4, Theta_4^down / Theta_4^cap, general rho.

No-message gap algebra (redone from the ledgers, not copied):
  M_0 - M_par = (m+1)(1+0) + rho(1-P_{0,m}) - (3m+2)
              = rho(1 - P_{0,m}) - 2m - 1.
At rho=40 this is 39-2m-40P, matching Cont-2's gamma_{0,m}.

Outputs:
  * exact table m_crit^nomsg(rho) = largest m with rho(1-P_m) > 2m+1
    at the down vertex (2,1,1,1)/5 and the cap vertex (3,3,2,2)/10,
    for rho in {20,24,28,32,36,40,48,56,64,80}; threshold pattern verified.
  * general-rho barrier B_r(rho,m) and the exact least rho for which the
    Cont-2 10<=m<=18 barrier argument survives unchanged.
  * exact least rho for the m=18 no-message endpoint to stay positive.
"""
from fractions import Fraction

C16 = [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64]
DOWN = (2, 1, 1, 1)
CAP = (3, 3, 2, 2)
RHOS = [20, 24, 28, 32, 36, 40, 48, 56, 64, 80]

def subset_moment(weights, m):
    Wl = sum(weights)
    n = len(weights)
    tot = Fraction(0)
    for mask in range(1 << n):
        s = sum(weights[i] for i in range(n) if mask >> i & 1)
        if s > 0:
            tot += Fraction(s, Wl) ** m
    return tot / (1 << n)

def gap(weights, m, rho):
    return rho * (1 - subset_moment(weights, m)) - 2 * m - 1

def p_floor(m):
    return 1 - Fraction(3, 5) ** m - 3 * Fraction(4, 5) ** m

def Bval(r, m, rho):
    return (Fraction((m + 1) * C16[r - 1], 16) - (2 * m + 1)
            + rho * p_floor(m) * Fraction(16 - r, 16))

def mcrit(weights, rho, mmax=60):
    """largest m with gap > 0; verify clean threshold pattern on [1,mmax]."""
    signs = [gap(weights, m, rho) > 0 for m in range(1, mmax + 1)]
    # find first False; assert everything after is False (threshold)
    first_neg = signs.index(False) + 1
    assert not any(signs[first_neg - 1:]), f"non-threshold pattern at rho={rho}"
    return first_neg - 1

def main():
    # sanity: rho=40 reproduces Cont-2 m_crit=18 at both vertices
    assert mcrit(DOWN, 40) == 18 and mcrit(CAP, 40) == 18
    print("rho | m_crit^nomsg(down vertex) | m_crit^nomsg(cap vertex) | crude floor((rho*15/16-1)/2)+1")
    for rho in RHOS:
        d, c = mcrit(DOWN, rho), mcrit(CAP, rho)
        crude = int((Fraction(rho) * Fraction(15, 16) - 1) / 2) + 1
        gd = gap(DOWN, d, rho)
        print(f"{rho:3d} | {d:3d}  (gap {gd} = {float(gd):+.6f}) | {c:3d}"
              f"  (gap {float(gap(CAP,c,rho)):+.6f}) | {crude}")
    print()
    # show exact gaps bracketing the crossing at each rho (down vertex)
    print("down vertex crossing brackets (exact):")
    for rho in RHOS:
        d = mcrit(DOWN, rho)
        print(f"  rho={rho:3d}: gamma({d}) = {gap(DOWN,d,rho)} > 0, "
              f"gamma({d+1}) = {gap(DOWN,d+1,rho)} < 0")
    print()
    print("cap vertex crossing brackets (exact):")
    for rho in RHOS:
        c = mcrit(CAP, rho)
        print(f"  rho={rho:3d}: gamma({c}) = {gap(CAP,c,rho)} > 0, "
              f"gamma({c+1}) = {gap(CAP,c+1,rho)} < 0")

    # ---- barrier survival threshold ----
    # B_r(rho,m) >= 1 forall r in 2..6, m in [10,18]
    #   <=> rho >= [16(2m+2) - (m+1)c_r] / [p_m (16-r)]   (denominator > 0)
    thresh = Fraction(0)
    argmax = None
    for r in range(2, 7):
        for m in range(10, 19):
            num = 16 * (2 * m + 2) - (m + 1) * C16[r - 1]
            den = p_floor(m) * (16 - r)
            t = Fraction(num, den)
            if t > thresh:
                thresh, argmax = t, (r, m)
    print()
    print(f"least rho for the Cont-2 barrier argument (r=2..6, m=10..18) unchanged:")
    print(f"  rho >= {thresh} = {float(thresh):.6f}, binding at (r,m) = {argmax}")
    # verify
    ok_at = all(Bval(r, m, thresh) >= 1 for r in range(2, 7) for m in range(10, 19))
    below = thresh - Fraction(1, 10 ** 6)
    fail_below = any(Bval(r, m, below) < 1 for r in range(2, 7) for m in range(10, 19))
    print(f"  check: B>=1 everywhere at rho={float(thresh):.6f}: {ok_at}; "
          f"some B<1 at rho-eps: {fail_below}")

    # ---- m=18 endpoint positivity threshold (both classes) ----
    P18d, P18c = subset_moment(DOWN, 18), subset_moment(CAP, 18)
    rd, rc = Fraction(37, 1 - P18d), Fraction(37, 1 - P18c)
    print()
    print("least rho keeping m=18 no-message margin > 0:")
    print(f"  down: rho > 37/(1-P_18) = {rd} = {float(rd):.6f}")
    print(f"  cap : rho > 37/(1-P_18) = {rc} = {float(rc):.6f}")
    print("  => full Cont-2 phase [1..18] survives for rho >= rho* where")
    print(f"     rho* = max(barrier, endpoint) = {max(thresh, rd)} (down), "
          f"{max(thresh, rc)} (cap)  [small-m strip via one-demand floors, PI]")

    # universal per-theta failure: m >= floor((rho*15/16-1)/2)+1 kills at every theta
    print()
    print("universal obstruction onset m_fail(4,rho) (P>=1/16, every theta):")
    for rho in RHOS:
        mf = int((Fraction(rho) * Fraction(15, 16) - 1) / 2) + 1
        print(f"  rho={rho:3d}: m_fail = {mf};  gap at down vertex there: "
              f"{float(gap(DOWN, mf, rho)):+.6f}")

if __name__ == "__main__":
    main()
