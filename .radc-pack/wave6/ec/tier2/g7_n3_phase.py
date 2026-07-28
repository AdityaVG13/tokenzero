#!/usr/bin/env python3
"""G7 (headline): full Cont-2-style sequential prefix-hull phase theorem
attempt at n=3, Theta_3^down = {theta_i >= 4/15}, gauge (rho,lambda)=(40,20).

Statement lock: M_T = (m+1)(1+ell) + 40 e_T, M_par = 3m+2, L_par = 4,
D_par = 0; N = 8; vertex (7,4,4)/15 (heavy n+4=7, lights 4, total 5n=15).

Components:
  (a) exact no-message gap gamma_{0,m} = 39-2m-40 P_{0,m} at the vertex,
      P_{0,m} = 2^-3 sum_B theta(B)^m; sign table m=1..18; obstruction.
  (b) general-n nontrivial-tree barrier
      Gamma_T >= (m+1) c_r/N - (2m+1) + rho p_m (1-r/N),  c_r = C_8(r),
      p_m = 1-(8/15)^m-2(11/15)^m (union bound, Schur-max at vertex);
      r>=5 ell>=2 case; find exact m-range with margin >= 1.
  (c) assembly: one-demand floor reduction for small m via exact
      subset-tree DP F_theta(t) = min 2(1+ell) + t e_1 over 2^8 subsets;
      full adaptive m-demand subset-tree DP (sol_m_demand_grid analogue)
      over a rational grid of Theta_3^down as supporting EC.
"""
from fractions import Fraction
from itertools import combinations

N = 8
C8 = [0, 8, 10, 13, 16, 20, 22, 24]  # verified in g2_spectra.py

VERTEX = (7, 4, 4)
UNIFORM = (5, 5, 5)
W = 15

# ---------------- demand-law grids over Theta_3^down ----------------
def grid(den):
    """Integer weight triples w_i >= 4*den/15, sum = den (den multiple of 15)."""
    lo = 4 * den // 15
    pts = []
    for a in range(lo, den - 2 * lo + 1):
        for b in range(lo, den - lo - a + 1):
            c = den - a - b
            if c >= lo:
                pts.append((a, b, c))
    return pts

GRID15 = grid(15)
assert len(GRID15) == 10 and VERTEX in GRID15 and UNIFORM in GRID15

# ---------------- (a) no-message face ----------------
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

# ---------------- (b) barrier ----------------
def p_floor(m):
    return 1 - Fraction(8, 15) ** m - 2 * Fraction(11, 15) ** m

def barrier(r, m):
    """Lower bound on Gamma_T for a tree with r leaves: ell >= c_r/8 and
    1 - P_T >= p_cov (1 - r/8) >= p_m (1-r/8)."""
    return (Fraction((m + 1) * C8[r - 1], 8) - (2 * m + 1)
            + 40 * p_floor(m) * Fraction(8 - r, 8))

# ---------------- one-demand floor DP (exact, 256 subsets) ----------------
def err_unnorm(weights, A):
    """W*e(A)*|A| = W|A| - sum_i w_i max_b N_{i,b}(A), integer."""
    Wl = sum(weights)
    tot = Wl * len(A)
    n = len(weights)
    for i in range(n):
        nb = [0, 0]
        for x in A:
            nb[(x >> i) & 1] += 1
        tot -= weights[i] * max(nb)
    return tot

def one_demand_floor(weights, t, alpha=2):
    """F_theta(t) = min over prefix policies of alpha(1+ell) + t e_1.
    Exact DP over all subsets of {0,1}^3. Returns Fraction."""
    Wl = sum(weights)
    # subsets as frozensets of ints 0..7; V[A] = min(t*E(A), min_split alpha|A|W + V[B]+V[C])
    V = {}
    E = {}
    elems = range(8)
    for size in range(1, 9):
        for A in combinations(elems, size):
            A = frozenset(A)
            E[A] = err_unnorm(weights, A)
            best = t * E[A]
            if size > 1:
                As = sorted(A)
                # splits: B nonempty proper subset containing As[0]
                rest = As[1:]
                for k in range(0, len(rest)):
                    for sub in combinations(rest, k):
                        B = frozenset((As[0],) + sub)
                        C = A - B
                        if not C:
                            continue
                        cand = alpha * size * Wl + V[B] + V[C]
                        if cand < best:
                            best = cand
            V[A] = best
    O = frozenset(elems)
    return Fraction(alpha) + Fraction(V[O], 8 * Wl)

# ---------------- full m-demand adaptive subset-tree DP ----------------
def cm_m(weights, m):
    """CM_m(A) = max over adaptive answer strategies of the demand-weighted
    joint-success mass, unnormalized: CM_0(A) = |A|,
    CM_m(A) = sum_i w_i max_b CM_{m-1}(A^{i,b}).  Integer recursion."""
    n = len(weights)
    subs = []
    for size in range(1, 9):
        subs += [frozenset(A) for A in combinations(range(8), size)]
    prev = {A: len(A) for A in subs}
    if m == 0:
        return prev
    for _ in range(m):
        cur = {}
        for A in subs:
            tot = 0
            for i in range(n):
                A0 = frozenset(x for x in A if not (x >> i) & 1)
                A1 = A - A0
                best = 0
                if A0:
                    best = prev[A0]
                if A1 and prev[A1] > best:
                    best = prev[A1]
                tot += weights[i] * best
            cur[A] = tot
        prev = cur
    return prev

def m_demand_optimum(weights, m, rho=40):
    """min over adaptive subset trees of (m+1)(1+ell) + rho e_T, joint
    m-demand error. Returns (Fraction value, L_ext, leaves) of an optimum."""
    Wl = sum(weights)
    alpha = m + 1
    Wm = Wl ** m
    CM = cm_m(weights, m)
    O = None
    V, choice = {}, {}
    for size in range(1, 9):
        for A in combinations(range(8), size):
            A = frozenset(A)
            E = len(A) * Wm - CM[A]
            best, ch = rho * E, ("stop",)
            if size > 1:
                As = sorted(A)
                rest = As[1:]
                for k in range(0, len(rest)):
                    for sub in combinations(rest, k):
                        B = frozenset((As[0],) + sub)
                        C = A - B
                        if not C:
                            continue
                        cand = alpha * size * Wm + V[B] + V[C]
                        if cand < best:
                            best, ch = cand, ("split", B, C)
            V[A], choice[A] = best, ch
    O = frozenset(range(8))
    # reconstruct
    L_ext = 0
    leaves = 0
    stack = [(O, 0)]
    while stack:
        A, d = stack.pop()
        if choice[A][0] == "stop":
            leaves += 1
            L_ext += len(A) * d
        else:
            _, B, C = choice[A]
            stack.append((B, d + 1))
            stack.append((C, d + 1))
    val = Fraction(alpha) + Fraction(V[O], 8 * Wm)
    return val, L_ext, leaves

def main():
    print("=== (a) no-message face, vertex (7,4,4)/15, rho=40 ===")
    print(" m | P_{0,m}(vertex) (decimal)      | gamma_{0,m} = 39-2m-40P")
    last_pos = None
    for m in range(1, 19):
        g = gap0(VERTEX, m)
        P = subset_moment(VERTEX, m)
        tag = "POS" if g > 0 else ("ZERO" if g == 0 else "NEG")
        if g > 0:
            last_pos = m
        print(f" {m:2d} | {float(P):.10f} | {g} = {float(g):+.6f}  {tag}")
    print(f"largest m with positive vertex no-message margin: {last_pos}")
    g16, g17 = gap0(VERTEX, 16), gap0(VERTEX, 17)
    print("gamma_0,16(vertex) =", g16, "=", float(g16))
    print("gamma_0,17(vertex) =", g17, "=", float(g17))
    assert g16 > 0 and g17 < 0
    # m=17 failure at EVERY theta: max gamma at min P = uniform (Schur)
    g17u = gap0(UNIFORM, 17)
    print("gamma_0,17(uniform) =", g17u, "=", float(g17u), "(<0 => m=17 fails at every theta)")
    assert g17u < 0
    # m>=18 universal: P >= 1/8 => gamma <= 34 - 2m <= -2
    print("m>=18 universal bound: gamma <= 39-2m-40/8 = 34-2m <= -2")
    # Schur EC support: P_{0,16} maximized at vertex over denom-15 grid
    Pv = subset_moment(VERTEX, 16)
    for pt in GRID15:
        assert subset_moment(pt, 16) <= Pv
    print("PASS EC: P_{0,16} maximized at vertex over denom-15 grid (10 pts)")

    print()
    print("=== (b) nontrivial-tree barrier, N=8, c_r =", C8, "===")
    # r >= 5: ell >= 2 => Gamma >= 1 exactly
    assert all(C8[r - 1] >= 16 for r in range(5, 9))
    print("r>=5: c_r >= 16 => ell >= 2 => Gamma_T >= (m+1)*2-(2m+1) = 1 for ALL m")
    # r in {2,3,4}: exact scan
    lo_m, hi_m = {}, {}
    for r in (2, 3, 4):
        good = [m for m in range(1, 41) if barrier(r, m) >= 1]
        lo_m[r], hi_m[r] = min(good), max(good)
        # contiguity check
        assert all(barrier(r, m) >= 1 for m in range(lo_m[r], hi_m[r] + 1))
        print(f"r={r}: barrier >= 1 exactly for m in [{lo_m[r]}, {hi_m[r]}] "
              f"(B_r(3)={barrier(r,3)} = {float(barrier(r,3)):.4f}, "
              f"B_r(4)={barrier(r,4)} = {float(barrier(r,4)):.4f}, "
              f"B_r(16)={barrier(r,16)} = {float(barrier(r,16)):.4f})")
    onset = max(lo_m.values())
    print(f"all r in {{2,3,4}} simultaneously >= 1 for m in [{onset}, {min(hi_m.values())}]")
    assert onset == 4
    assert all(barrier(r, m) >= 1 for r in (2, 3, 4) for m in range(4, 17))
    print("PASS: every r>=2 nontrivial tree has Gamma_T >= 1 for 4 <= m <= 16")

    print()
    print("=== (c1) one-demand floor DP, exact F_theta(t), alpha=2 ===")
    for t in (Fraction(20), Fraction(80, 3), Fraction(40)):
        Fv = one_demand_floor(VERTEX, t)
        print(f"F_vertex({t}) = {Fv}")
        assert Fv == 8
    # class support: min over grid
    for t in (Fraction(20), Fraction(80, 3), Fraction(40)):
        Fs = [one_demand_floor(pt, t) for pt in GRID15]
        assert all(F == 8 for F in Fs), (t, Fs)
    print("PASS: F_theta(t) = 8 for t in {20, 80/3, 40} at vertex and all 10 grid laws")
    print("=> m<=3 reduction: M_T >= ((m+1)/2)*8 = 4(m+1), margin vs M_par = m+2 >= 3")
    # t = 16 (m=4) NOT saturated at vertex: documents why strip ends at m=3
    F16 = one_demand_floor(VERTEX, Fraction(16))
    print("F_vertex(16) =", F16, "=", float(F16), "(< 8: reduction stops at m=3; barrier covers m>=4)")
    # breakpoint check: least dyadic-grid t with F = 8 near 135/8 = 16.875
    for j in range(130, 140):
        t = Fraction(j, 8)
        print(f"  F_vertex({t}) = {one_demand_floor(VERTEX, t)}")
    # L-side floor at registered gauge
    F40s = [one_demand_floor(pt, Fraction(40)) for pt in GRID15]
    assert all(F == 8 for F in F40s)
    print("PASS L-side: F_theta(40) = 8 on grid => 2 L_T >= 2+2ell+40 e_1 >= 8 => L_T >= 4 = L_par (tie)")

    print()
    print("=== (c2) full adaptive m-demand subset-tree DP over Theta_3^down grid ===")
    print("theta (w/15) | m | min M_T (exact) | margin vs 3m+2 | L_ext | leaves")
    for pt in sorted(GRID15, reverse=True):
        for m in range(1, 18):
            val, Lx, lv = m_demand_optimum(pt, m)
            marg = val - (3 * m + 2)
            flag = ""
            if marg <= 0:
                flag = "  <-- PARITY BEATEN"
            print(f"{pt} | {m:2d} | {float(val):.6f} | {float(marg):+.6f} | {Lx} | {lv}{flag}")
        print()
    print("key lines at vertex (7,4,4)/15:")
    for m in (3, 4, 15, 16, 17):
        val, Lx, lv = m_demand_optimum(VERTEX, m)
        print(f"  m={m}: opt M_T = {val} = {float(val):.6f}, margin = {val-(3*m+2)} = {float(val-(3*m+2)):+.6f}, L_ext={Lx}, leaves={lv}")

if __name__ == "__main__":
    main()
