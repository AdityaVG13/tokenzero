#!/usr/bin/env python3
"""G10: lambda decoupling, exact EC at n=3 (down vertex) + registered-instance
verification at n=4 via the one-demand floor DP of g7.

L_T = 1 + ell + c + lambda e_T >= 1 + ell + lambda e_1  (c>=0, e_T>=e_1)
    >= G_theta(lambda) := min over prefix trees of (1+ell) + lambda e_1
    = F_theta(2 lambda)/2     (alpha=1 floor = alpha=2 floor at double t)

So the certified latency margin vs L_par = 4 is
    gamma_L >= F_theta(2 lambda)/2 - 4,
and at m=1 the minimum of L_T over the hull IS G_theta(lambda) exactly
(e_T = e_1 for m=1), so the lambda-phase from G is exact, not a bound.

This file computes G_theta(lambda) exactly at the Q3-down vertex (7,4,4)/15
via a Pareto-frontier DP over the 256 subsets (frontier of (L_ext, E) pairs),
recovers the full lower envelope, and finds the exact least lambda with
G(lambda) >= 4 (= L_par), i.e. lambda* = max over supported pairs with
L_ext < 3N of W(3N - L_ext)/E.
"""
from fractions import Fraction
from itertools import combinations

VERTEX = (7, 4, 4)
N = 8
W = 15

def err_unnorm(weights, A):
    Wl = sum(weights)
    tot = Wl * len(A)
    for i in range(len(weights)):
        nb = [0, 0]
        for x in A:
            nb[(x >> i) & 1] += 1
        tot -= weights[i] * max(nb)
    return tot

def frontier(weights):
    """Per subset A: Pareto-minimal set of (L_ext, E) over trees on A,
    where L_ext = sum |leaf| depth and E = unnormalized one-demand error.
    stop: (0, E(A)); split B,C: (|A| + L_B + L_C, E_B + E_C)."""
    F = {}
    for size in range(1, 9):
        for At in combinations(range(8), size):
            A = frozenset(At)
            pts = {(0, err_unnorm(weights, A))}
            if size > 1:
                As = sorted(A)
                rest = As[1:]
                for k in range(0, len(rest)):
                    for sub in combinations(rest, k):
                        B = frozenset((As[0],) + sub)
                        C = A - B
                        if not C:
                            continue
                        for lb, eb in F[B]:
                            for lc, ec in F[C]:
                                pts.add((size + lb + lc, eb + ec))
            # pareto prune (minimize both coordinates)
            pts = sorted(pts)
            kept = []
            beste = None
            for l, e in pts:
                if beste is None or e < beste:
                    kept.append((l, e))
                    beste = e
            F[A] = kept
    return F[frozenset(range(8))]

def main():
    fr = frontier(VERTEX)
    print("Pareto frontier (L_ext, E) at root, Q3-down vertex (7,4,4)/15:")
    for l, e in fr:
        print(f"  L_ext={l:3d}  E={e:4d}   (ell={Fraction(l,8)}, e={Fraction(e,8*W)})")
    # G(lambda) = 1 + min over frontier of (L_ext/8 + lambda E/(8*15))
    # G(lambda) >= 4  <=>  lambda E >= 15(24 - L_ext) for all frontier pairs
    lam_star = Fraction(0)
    argmax = None
    for l, e in fr:
        if e > 0 and l < 24:
            t = Fraction(W * (24 - l), e)
            if t > lam_star:
                lam_star, argmax = t, (l, e)
    print()
    print(f"exact lambda* = {lam_star} = {float(lam_star):.6f}, binding pair (L_ext,E) = {argmax}")
    print(f"  i.e. G(lambda) >= 4 = L_par  iff  lambda >= {lam_star};")
    print(f"  strict gamma_L > 0 iff lambda > {lam_star}; at equality a tie is attained.")
    print(f"  W4 landmark check: rho*_Q3down = 135/8, so lambda* should = (135/8)/2 = 135/16 = {Fraction(135,16)}")
    assert lam_star == Fraction(135, 16)
    print("PASS: exact lambda* = 135/16 reproduces rho*_Q3down / 2 (W4, PI) by independent DP")

    # envelope of G at sample lambdas
    print()
    print("G(lambda) samples at Q3-down vertex:")
    for lam in (Fraction(1), Fraction(3), Fraction(45, 8), Fraction(7),
                Fraction(135, 16), Fraction(10), Fraction(20)):
        g = 1 + min(Fraction(l, 8) + lam * Fraction(e, 8 * W) for l, e in fr)
        print(f"  G({lam}) = {g} = {float(g):.6f}   gamma_L = {float(g - 4):+.6f}")

    # registered-instance check: lambda=20 deep inside safe region
    g20 = 1 + min(Fraction(l, 8) + 20 * Fraction(e, 8 * W) for l, e in fr)
    print()
    print(f"registered lambda=20: G(20) = {g20}, gamma_L >= {g20 - 4}")
    print("=> lambda never binds at the registered instance; the L-margin comes")
    print("   from the F(40)=10 (Q4) / F(40)=8 (Q3) floor inequality, lambda-free.")
    # no-message failure witness for small lambda (m=1): L_0 = 1 + lambda(1-max theta)
    print()
    print(f"no-message witness (m=1): L_0 = 1 + lambda*(8/15) < 4 iff lambda < 45/8 = {Fraction(45,8)}")
    print("   (actual L-dominance failure region; exact failure onset is lambda* above)")

if __name__ == "__main__":
    main()
