#!/usr/bin/env python3
"""G4/G5/G6: independent exact-rational re-verification of the Cont-2 Q4
10<=m<=18 nontrivial-tree barrier B_r, the m=18 sharp no-message margins,
and the m>=19 obstruction. Python stdlib fractions only; written fresh
(Tier-2), not ported from the Cont-2 checker.

Ledger (statement lock): M_T = (m+1)(1+ell) + 40 e_T, M_par = 3m+2,
Gamma_T = M_T - M_par = (m+1) ell - (2m+1) + 40 (1 - P_T).
Coverage-leaf: 1 - P_T >= p_cov (1 - r/16).  Spectrum C_16 as asserted.
"""
from fractions import Fraction

# C_16(r), r = 1..16 (verified independently in g2_spectra.py)
C16 = [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64]

def p_floor(m):
    """Coverage floor on Theta_4^down (theta_i >= 1/5), union bound +
    Schur-convexity: max miss-sum at vertex (2/5,1/5,1/5,1/5)."""
    return 1 - Fraction(3, 5) ** m - 3 * Fraction(4, 5) ** m

def subset_moment(weights, m):
    """P_{0,m}(theta) = 2^-4 sum_B theta(B)^m with integer weights, W=sum."""
    n = len(weights)
    W = sum(weights)
    total = Fraction(0)
    for mask in range(1 << n):
        s = sum(weights[i] for i in range(n) if mask >> i & 1)
        if m == 0 or s > 0:
            total += Fraction(s, W) ** m
    return total / (1 << n)

def gap0(weights, m):
    """No-message gap M_0 - M_par = 39 - 2m - 40 P_{0,m}."""
    return 39 - 2 * m - 40 * subset_moment(weights, m)

def main():
    # ---- G4: coverage floor p_10 and barrier B_r, r=2..6 ----
    p10 = p_floor(10)
    print("p_10 =", p10, "=", float(p10))
    assert p10 == Fraction(6560848, 9765625)
    # p_m nondecreasing in m (floor valid at left endpoint of any range)
    assert all(p_floor(m) <= p_floor(m + 1) for m in range(2, 20))

    B = {}
    for r in range(2, 7):
        # B_r = min over m in [10,18] of (m+1)c_r/16 - (2m+1) + 40 p_m (1-r/16);
        # linear part decreasing, coverage part increasing: evaluate both ends
        # and report full grid min.
        vals = {}
        for m in range(10, 19):
            vals[m] = (Fraction((m + 1) * C16[r - 1], 16) - (2 * m + 1)
                       + 40 * p_floor(m) * Fraction(16 - r, 16))
        B[r] = vals
    # Cont-2's closed form uses the m=18 endpoint with p_10 (valid since
    # p_m >= p_10 and the linear part is minimized at m=18; the true min
    # over the range must be >= that closed form -- verify):
    for r in range(2, 7):
        closed = (Fraction(19 * C16[r - 1], 16) - 37
                  + 40 * p10 * Fraction(16 - r, 16))
        gridmin = min(B[r].values())
        print(f"r={r}: Cont-2 B_r = {closed} = {float(closed):.6f}; "
              f"true grid min over m in [10,18] = {gridmin} = {float(gridmin):.6f} "
              f"(attained m={min(B[r], key=B[r].get)})")
        assert gridmin >= closed, f"r={r}: grid min below Cont-2 closed form"
        assert closed > 1
    expected = {2: Fraction(10769686, 1953125), 3: Fraction(97023471, 15625000),
                4: Fraction(252888283, 31250000), 5: Fraction(38966203, 3906250),
                6: Fraction(20384017, 1562500)}
    for r in range(2, 7):
        closed = (Fraction(19 * C16[r - 1], 16) - 37
                  + 40 * p10 * Fraction(16 - r, 16))
        assert closed == expected[r], f"B_{r} mismatch vs Cont-2"
    print("PASS G4: all five B_r fractions reproduce Cont-2 exactly and exceed 1")

    # r >= 7 case: ell >= 2 => Gamma >= 1 exactly, all m
    for m in range(1, 25):
        assert (m + 1) * 2 - (2 * m + 1) == 1
    print("PASS G4: r>=7 barrier Gamma_T >= (m+1)*2-(2m+1) = 1 exactly (C_16(r)>=32 for r>=7: %s)"
          % (C16[6:],))

    # ---- G5: m=18 sharp margins at class-extreme no-message baselines ----
    down = (2, 1, 1, 1)   # theta = (2/5,1/5,1/5,1/5)
    cap = (3, 3, 2, 2)    # theta = (3/10,3/10,1/5,1/5)
    g18d, g18c = gap0(down, 18), gap0(cap, 18)
    g17d, g17c = gap0(down, 17), gap0(cap, 17)
    print("gamma_0,18 down =", g18d, "=", float(g18d))
    print("gamma_0,18 cap  =", g18c, "=", float(g18c))
    print("gamma_0,17 down =", g17d, "=", float(g17d))
    print("gamma_0,17 cap  =", g17c, "=", float(g17c))
    assert g18d == Fraction(277615146191, 762939453125)
    assert g18c == Fraction(20074685943080277, 50000000000000000)
    assert g17d == Fraction(71088276063, 30517578125)
    assert g17c == Fraction(475055717444931, 200000000000000)
    assert 0 < g18d < 1 and 0 < g18c < 1 and g17d > 1 and g17c > 1
    print("PASS G5: m=18 margins exact, in (0,1) sharp; m=17 margins > 1")

    # monotonicity certificates 20 m^m < (m+1)^(m+1), m=10..17, and strict
    # gap decrease on [10,17] for both weight vectors
    for m in range(10, 18):
        assert 20 * m ** m < (m + 1) ** (m + 1)
        assert gap0(down, m) > gap0(down, m + 1)
        assert gap0(cap, m) > gap0(cap, m + 1)
    print("PASS G5: integer certificates 20 m^m < (m+1)^(m+1), m=10..17; "
          "no-message gap strictly decreasing on [10,18] both classes")

    # ---- G6: m >= 19 obstruction ----
    # any fixed-prototype no-message policy: P_0 >= 1/16 (event X = prototype)
    # => M_0 - M_par <= 73/2 - 2m; at m=19: -3/2.
    assert Fraction(73, 2) - 2 * 19 == Fraction(-3, 2)
    # exact negative gaps at m=19 (class-extreme laws)
    g19d, g19c = gap0(down, 19), gap0(cap, 19)
    print("gamma_0,19 down =", g19d, "=", float(g19d))
    print("gamma_0,19 cap  =", g19c, "=", float(g19c))
    assert g19d == Fraction(-1227337666073, 762939453125)
    assert g19c == Fraction(-157792852435281487, 100000000000000000)
    assert g19d < 0 and g19c < 0
    print("PASS G6: universal m>=19 obstruction, 73/2-2m = -3/2 at m=19; "
          "exact negative m=19 gaps reproduced")

    # ---- coverage-leaf sanity vs exact no-message P (r=1) ----
    # exact P_{0,m} must satisfy P <= 1 - p_cov(1 - 1/16); check at vertex
    for m in (2, 10, 18):
        P = subset_moment(down, m)
        bound = 1 - p_floor(m) * Fraction(15, 16)
        assert P <= bound, (m, P, bound)
    print("PASS G1-sanity: exact no-message P_{0,m} <= 1 - p_m(1-1/16) at down vertex, m=2,10,18")

if __name__ == "__main__":
    main()
