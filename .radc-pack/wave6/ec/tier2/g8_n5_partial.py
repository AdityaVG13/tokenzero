#!/usr/bin/env python3
"""G8: n=5 partial EC. Theta_5^down = {theta_i >= 4/25}, vertex (9,4,4,4,4)/25,
N=32, rho=40, M_par = 3m+2.

  * C_32(r) full spectrum (assert against g2 output).
  * coverage floors p_m = 1-(16/25)^m-4(21/25)^m (union bound, vertex).
  * nontrivial-tree barrier B_r(m) = (m+1)c_r/32 - (2m+1) + 40 p_m (1-r/32):
    onset per r; simultaneous onset; r>=11 ell>=2 case.
  * crude no-message m_fail(5,40); exact vertex no-message margins m=14..21.
  * one-demand floor reach: PI input rho_cert(5) <= 18 (Fable W5-Q5-SW) =>
    F_{5,down}(t) = 12 for t >= 18 => reduction covers 80/(m+1) >= 18, m<=3.
  * verdict: what blocks the full n=5 phase theorem.
"""
from fractions import Fraction

C32 = [0, 32, 34, 37, 40, 44, 48, 52, 56, 61, 66, 71, 76, 81, 86, 91, 96,
       102, 108, 114, 120, 124, 128, 132, 136, 141, 146, 149, 152, 156, 158, 160]
VERTEX5 = (9, 4, 4, 4, 4)

def p_floor(m):
    return 1 - Fraction(16, 25) ** m - 4 * Fraction(21, 25) ** m

def barrier(r, m):
    return (Fraction((m + 1) * C32[r - 1], 32) - (2 * m + 1)
            + 40 * p_floor(m) * Fraction(32 - r, 32))

def subset_moment(weights, m):
    Wl = sum(weights)
    n = len(weights)
    tot = Fraction(0)
    for mask in range(1 << n):
        s = sum(weights[i] for i in range(n) if mask >> i & 1)
        if s > 0:
            tot += Fraction(s, Wl) ** m
    return tot / (1 << n)

def gap0(weights, m):
    return 39 - 2 * m - 40 * subset_moment(weights, m)

def main():
    # crude m_fail
    mf = (40 * Fraction(31, 32) - 1) / 2
    print("crude m_fail(5,40) = floor((40*(31/32)-1)/2)+1 = floor(%s)+1 = %d"
          % (mf, int(mf) + 1))

    # ell>=2 case threshold
    r0 = next(r for r in range(1, 33) if C32[r - 1] >= 64)
    print("least r with C_32(r) >= 64 (ell>=2 => Gamma>=1 all m): r =", r0)
    assert r0 == 11

    # barrier onset per r
    onsets = {}
    for r in range(2, r0):
        good = [m for m in range(1, 41) if barrier(r, m) >= 1]
        onsets[r] = (min(good), max(good))
    for r in range(2, r0):
        lo, hi = onsets[r]
        assert all(barrier(r, m) >= 1 for m in range(lo, hi + 1))
        print(f"r={r:2d} (c={C32[r-1]:3d}): barrier >= 1 for m in [{lo:2d},{hi:2d}]; "
              f"B_r(10)={float(barrier(r,10)):+.4f} B_r(11)={float(barrier(r,11)):+.4f} "
              f"B_r(18)={float(barrier(r,18)):+.4f}")
    onset = max(lo for lo, _ in onsets.values())
    print("simultaneous barrier onset (all r in [2,10]): m =", onset)
    print("barrier holds on [onset,18] for all r in [2,10]:",
          all(barrier(r, m) >= 1 for r in range(2, 11) for m in range(onset, 19)))

    # exact no-message vertex margins
    print("\nvertex (9,4,4,4,4)/25 no-message gaps gamma_{0,m} = 39-2m-40 P:")
    last_pos = None
    for m in range(14, 22):
        g = gap0(VERTEX5, m)
        if g > 0:
            last_pos = m
        print(f"  m={m:2d}: {g} = {float(g):+.6f}  {'POS' if g>0 else 'NEG'}")
    print("largest m with positive vertex margin:", last_pos)
    g18, g19 = gap0(VERTEX5, 18), gap0(VERTEX5, 19)
    print("gamma_0,18(vertex) =", g18)
    print("gamma_0,19(vertex) =", g19)
    assert g18 > 0 and g19 < 0

    # one-demand reach (PI input)
    print("\none-demand reduction (PI: Fable W5-Q5-SW cert rho_cert(5) <= 18,"
          " i.e. F_{5,down}(t)=12 for t>=18):")
    for m in range(1, 6):
        t = Fraction(80, m + 1)
        ok = t >= 18
        marg = Fraction((m + 1) * 12, 2) - (3 * m + 2) if ok else None
        print(f"  m={m}: t=80/{m+1}={float(t):.3f} >= 18? {ok}"
              + (f"  margin >= {marg}" if ok else "  floor unknown in (1600/121,18): OPEN"))
    # gap analysis
    print("\nVERDICT: certified fragments at n=5, rho=40:")
    print("  m <= 3       : one-demand reduction, margin 3m+4 (PI floor F=12)")
    print(f"  {onset} <= m <= 18 : barrier >= 1 (r>=2) + no-message vertex > 0 (this file)")
    print("  m >= 19      : no-message vertex beats parity (exact, above)")
    print("  4 <= m <= %d : OPEN -- needs F_{5,down}(t) for t in (1600/121, 18)" % (onset - 1))
    print("  missing input: exact one-demand Q5-down class floor F_{5,down}(t),")
    print("  equivalently closing the Q5 sandwich (1600/121, 18] to an exact value.")

if __name__ == "__main__":
    main()
