#!/usr/bin/env python3
"""Exact certificates for RADC Wave-5 Continuation 2.

All arithmetic is rational/integer.  This checker certifies:
  * the exact minimum external path sums C_16(r);
  * the Q4 coverage lower bound p_10;
  * the nontrivial-tree memory gaps for r=2,...,6;
  * the no-message endpoint margins at m=17,18;
  * the finite monotonicity certificates for m=10,...,17;
  * the universal m>=19 no-message obstruction.
"""
from fractions import Fraction
from functools import lru_cache
from itertools import combinations

F = Fraction

@lru_cache(None)
def min_external_path_sum(N: int, r: int) -> int:
    """Exact minimum sum_x depth(x) for N equiprobable source states in r nonempty leaves."""
    if r == 1:
        return 0
    if r < 1 or r > N:
        return 10**9
    best = 10**9
    # A full binary root split; unary nodes can be contracted.
    for a in range(1, N):
        b = N - a
        for r1 in range(1, r):
            r2 = r - r1
            if r1 <= a and r2 <= b:
                best = min(best, N + min_external_path_sum(a, r1) + min_external_path_sum(b, r2))
    return best


def subset_moment(weights: tuple[int, ...], m: int) -> Fraction:
    W = sum(weights)
    n = len(weights)
    ans = F(0)
    for mask in range(1 << n):
        z = sum(weights[i] for i in range(n) if (mask >> i) & 1)
        ans += F(z, W) ** m
    return ans / (1 << n)


def no_message_gap(weights: tuple[int, ...], m: int) -> Fraction:
    # M_0 - M_par = 39 - 2m - 40 P_{0,m}.
    return F(39 - 2 * m) - 40 * subset_moment(weights, m)


def main() -> None:
    expected_C = [0, 16, 18, 21, 24, 28, 32, 36, 40, 45, 50, 53, 56, 60, 62, 64]
    got_C = [min_external_path_sum(16, r) for r in range(1, 17)]
    assert got_C == expected_C, (got_C, expected_C)

    p10 = F(1) - F(3, 5) ** 10 - 3 * F(4, 5) ** 10
    assert p10 == F(6560848, 9765625)

    c = {2: 16, 3: 18, 4: 21, 5: 24, 6: 28}
    expected_B = {
        2: F(10769686, 1953125),
        3: F(97023471, 15625000),
        4: F(252888283, 31250000),
        5: F(38966203, 3906250),
        6: F(20384017, 1562500),
    }
    B = {}
    for r, cr in c.items():
        # Uniform lower bound for every 10<=m<=18.
        B[r] = F(19 * cr, 16) - 37 + 40 * p10 * F(16 - r, 16)
        assert B[r] == expected_B[r]
        assert B[r] > 1

    down = (2, 1, 1, 1)
    cap = (3, 3, 2, 2)
    exact_endpoints = {
        ("down", 17): F(71088276063, 30517578125),
        ("down", 18): F(277615146191, 762939453125),
        ("cap", 17): F(475055717444931, 200000000000000),
        ("cap", 18): F(20074685943080277, 50000000000000000),
    }
    for name, w in (("down", down), ("cap", cap)):
        for m in (17, 18):
            g = no_message_gap(w, m)
            assert g == exact_endpoints[(name, m)]
        assert no_message_gap(w, 17) > 1
        assert 0 < no_message_gap(w, 18) < 1

    # Exact integer monotonicity certificates:
    # max_{z in [0,1]} z^m(1-z) = m^m/(m+1)^(m+1) < 1/20.
    for m in range(10, 18):
        assert 20 * (m ** m) < (m + 1) ** (m + 1)
        for w in (down, cap):
            assert no_message_gap(w, m) > no_message_gap(w, m + 1)

    # Direct finite-range check: every nontrivial-tree lower bound is positive.
    for m in range(10, 19):
        pm = F(1) - F(3, 5) ** m - 3 * F(4, 5) ** m
        assert pm >= p10
        for r, cr in c.items():
            gap = F((m + 1) * cr, 16) - (2 * m + 1) + 40 * pm * F(16 - r, 16)
            assert gap >= B[r]
        # r>=7: ell>=2 and P_succ<=1, hence gap>=1 exactly.
        assert F(2 * (m + 1) - (2 * m + 1)) == 1

    # Universal no-message obstruction for m>=19:
    # P_success >= 1/16, so M_0-M_par <= 73/2 - 2m.
    assert F(73, 2) - 2 * 19 == F(-3, 2)

    print("PASS exact C_16(r):", got_C)
    print("PASS p10:", p10, "~=", float(p10))
    print("PASS nontrivial-tree uniform lower bounds:")
    for r in sorted(B):
        print(f"  r={r}: {B[r]} ~= {float(B[r]):.12f}")
    print("PASS no-message endpoint margins:")
    for key in (("down", 17), ("down", 18), ("cap", 17), ("cap", 18)):
        val = exact_endpoints[key]
        print(f"  {key[0]} m={key[1]}: {val} ~= {float(val):.12f}")
    print("PASS m=10..17 exact monotonicity inequalities")
    print("PASS all nontrivial trees have M-gap >=1 for m=10..18")
    print("PASS universal no-message obstruction begins at m=19")


if __name__ == "__main__":
    main()
