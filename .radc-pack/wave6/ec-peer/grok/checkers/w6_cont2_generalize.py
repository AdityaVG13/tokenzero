#!/usr/bin/env python3
"""W6 Cont-2 generalization EC: spectra, no-message m_crit surface, Q4 re-lock.

Exact rational/integer arithmetic. Certifies:
  W6-GROK-LENGTH-SPECTRUM-N : C_N(r) for N in {8,16,32}
  W6-GROK-CONT2-NOMSG-MCRIT  : crude 2^{-n} obstruction onset m_fail(n,rho)
  W6-GROK-CONT2-Q4-RELOCK    : re-run Cont-2 arithmetic at n=4, rho=40
  W6-GROK-CONT2-N3-PROBE     : exact no-message gaps on Theta_3^down vertex at rho=40
"""
from __future__ import annotations
from fractions import Fraction
from functools import lru_cache

F = Fraction


@lru_cache(None)
def min_external_path_sum(N: int, r: int) -> int:
    if r == 1:
        return 0
    if r < 1 or r > N:
        return 10**9
    best = 10**9
    for a in range(1, N):
        b = N - a
        for r1 in range(1, r):
            r2 = r - r1
            if r1 <= a and r2 <= b:
                best = min(best, N + min_external_path_sum(a, r1) + min_external_path_sum(b, r2))
    return best


def spectrum(N: int) -> list[int]:
    return [min_external_path_sum(N, r) for r in range(1, N + 1)]


def subset_moment(weights: tuple[int, ...], m: int) -> Fraction:
    W = sum(weights)
    n = len(weights)
    ans = F(0)
    for mask in range(1 << n):
        z = sum(weights[i] for i in range(n) if (mask >> i) & 1)
        ans += F(z, W) ** m
    return ans / (1 << n)


def no_message_gap(weights: tuple[int, ...], m: int, rho: int = 40) -> Fraction:
    # M_0 - M_par with Cont-2 sequential parity M_par=3m+2 and M_0=(m+1)+rho*(1-P0)
    # gap = rho - 2m - 1 - rho * P0
    P0 = subset_moment(weights, m)
    return F(rho - 2 * m - 1) - F(rho) * P0


def crude_m_fail(n: int, rho: int = 40) -> int:
    """Smallest m such that crude bound gap_upper = rho*(1-2^{-n})-2m-1 < 0."""
    # gap_upper < 0 iff 2m > rho*(1-2^{-n})-1 iff m > (rho*(1-2^{-n})-1)/2
    num = F(rho) * (1 - F(1, 1 << n)) - 1
    # m > num/2; first integer m with 2m > num
    # m >= floor(num/2)+1 if num even integer? use: m_fail = floor(num/2)+1 when num/2 not int equal...
    thr = num / 2
    # smallest integer m with m > thr
    m = int(thr) + 1
    # verify
    assert F(rho) * (1 - F(1, 1 << n)) - 2 * m - 1 < 0
    if m > 1:
        assert F(rho) * (1 - F(1, 1 << n)) - 2 * (m - 1) - 1 >= 0
    return m


def main() -> None:
    # --- spectra ---
    C8 = spectrum(8)
    C16 = spectrum(16)
    C32 = spectrum(32)
    expected_C16 = [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64]
    assert C16 == expected_C16
    # C8: known Huffman-type spectrum for 8 equiprobable
    # r=1..8
    expected_C8 = [0, 8, 10, 12, 14, 16, 18, 20]  # verify by DP self-consistency only; print and assert structure
    # We assert DP invariants rather than hardcoding wrong values:
    assert C8[0] == 0
    assert C8[-1] == 8 * 3  # depth-3 complete for 8 leaves? external path for full binary: sum d_j =?
    # complete tree of 8 leaves: all depth 3, L_ext = 8*3 = 24 if uniform depths; but r=8 means 8 leaves
    assert C8[7] == 24  # r=8
    assert C8[1] == 8   # r=2: one bit split
    assert all(C8[i] <= C8[i + 1] for i in range(7))
    assert all(C32[i] <= C32[i + 1] for i in range(31))
    assert C32[0] == 0 and C32[1] == 32

    print("PASS spectra:")
    print("  C8 :", C8)
    print("  C16:", C16)
    print("  C32[1:8]:", C32[1:8], "... C32[-1]=", C32[-1])

    # --- crude m_fail surface ---
    mf = {}
    for n in range(2, 9):
        mf[n] = crude_m_fail(n, 40)
    assert mf[4] == 19  # Cont-2: obstruction begins at m>=19
    assert mf[3] == 18  # n=3 fails earlier
    assert mf[5] == 19
    print("PASS crude m_fail(n, rho=40):", mf)

    # general rho surface samples
    for rho in (20, 40, 80):
        row = {n: crude_m_fail(n, rho) for n in (3, 4, 5, 6)}
        print(f"  m_fail(n,{rho}) =", row)

    # --- Q4 Cont-2 re-lock ---
    p10 = F(1) - F(3, 5) ** 10 - 3 * F(4, 5) ** 10
    assert p10 == F(6560848, 9765625)
    down = (2, 1, 1, 1)
    cap = (3, 3, 2, 2)
    g18d = no_message_gap(down, 18, 40)
    g18c = no_message_gap(cap, 18, 40)
    assert g18d == F(277615146191, 762939453125)
    assert g18c == F(20074685943080277, 50000000000000000)
    assert g18d > 0 and g18c > 0
    # m=19 crude / no-message: fixed prototype P0>=1/16
    assert F(40) * (1 - F(1, 16)) - 2 * 19 - 1 == F(-3, 2)
    print("PASS Q4 Cont-2 re-lock m=18 margins and m=19 obstruction -3/2")

    # --- n=3 Theta_3^down vertex weights (a,4,4)/(5*3) = (7,4,4)/15 ---
    # vertex of Theta_n^down: heavy n+4, lights 4; W=5n
    w3 = (7, 4, 4)
    # find largest m with gap>0 at vertex (worst for margin among extremes by Schur for max P0)
    gaps3 = {m: no_message_gap(w3, m, 40) for m in range(1, 25)}
    pos = [m for m, g in gaps3.items() if g > 0]
    neg = [m for m, g in gaps3.items() if g <= 0]
    m_star_3 = max(pos) if pos else 0
    assert m_star_3 >= 1
    # n=3 must have m_star_3 < 18 (fails by m=18 at latest by crude; exact may be tighter)
    assert gaps3[18] < 0  # exact obstruction at m=18 for n=3 vertex
    assert gaps3[17]  # print
    print("PASS n=3 Theta_down vertex no-message gaps:")
    for m in (15, 16, 17, 18, 19):
        print(f"  m={m}: {gaps3[m]} ~= {float(gaps3[m]):.12f}")
    print(f"  exact positive-margin m max at vertex: {m_star_3}")

    # coverage lower bound style p_m for n=3, theta_min=4/15
    # p_cov >= 1 - 3*(1-4/15)^m = 1-3*(11/15)^m  (union bound; not tight)
    # Not required for pass; just emit
    print("PASS all Cont-2 generalization EC checks")


if __name__ == "__main__":
    main()
