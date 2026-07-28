#!/usr/bin/env python3
"""W5 Part B: two-demand sequential floors F2_alpha (NEW).
Leaf distortion for two i.i.d. demands S1,S2 ~ theta with the FULL adaptive
decoder: answer a1 may depend on S1; answer a2 on (S1,S2,a1).
e2(leaf A) = 1 - sum_i th_i max_a sum_j th_j max_b P(x_i=a, x_j=b | A).
Unnormalized: E2(A) = d^2 |A| - CM(A),
CM(A) = sum_i w_i max_a sum_j w_j max_b N_ij^ab(A).
"""
from fractions import Fraction as Fr
import time

def build(n, w, d):
    N = 1 << n; size = 1 << N
    pc = [0]*size
    for A in range(1, size):
        pc[A] = pc[A & (A-1)] + 1
    # N_ij^ab tables, ordered pairs incl i=j (for i=j: N_ii^ab = N_i^a * [a==b])
    # store as dict of arrays
    idx = {}
    tabs = []
    k = 0
    for i in range(n):
        for j in range(n):
            for a in (0, 1):
                for b in (0, 1):
                    idx[(i, j, a, b)] = k; k += 1
                    tabs.append([0]*size)
    for A in range(1, size):
        lb = A & (-A); x = lb.bit_length() - 1
        prev = A ^ lb
        for i in range(n):
            xa = (x >> i) & 1
            for j in range(n):
                xb = (x >> j) & 1
                for a in (0, 1):
                    for b in (0, 1):
                        t = tabs[idx[(i, j, a, b)]]
                        t[A] = t[prev] + (1 if (xa == a and xb == b) else 0)
    E2 = [0]*size
    for A in range(1, size):
        cm = 0
        for i in range(n):
            best = 0
            for a in (0, 1):
                s = 0
                for j in range(n):
                    n0 = tabs[idx[(i, j, a, 0)]][A]
                    n1 = tabs[idx[(i, j, a, 1)]][A]
                    s += w[j] * (n0 if n0 >= n1 else n1)
                if s > best: best = s
            cm += w[i] * best
        E2[A] = d*d*pc[A] - cm
    return E2, pc

def scalar_dp(n, E, pc, p, q, lencoef):
    size = 1 << (1 << n)
    G = [0]*size; BS = [0]*size
    lq = lencoef * q
    for A in range(1, size):
        k = pc[A]
        best = p * E[A]; bs = 0
        if k >= 2:
            base = lq * k
            lb = A & (-A); rest = A ^ lb; s = rest
            while True:
                s = (s - 1) & rest
                Bm = s | lb
                if Bm != A:
                    val = base + G[Bm] + G[A ^ Bm]
                    if val < best:
                        best = val; bs = Bm
                if s == 0:
                    break
        G[A] = best; BS[A] = bs
    return G, BS

def solve_pair(n, E, pc, t, lencoef):
    G, BS = scalar_dp(n, E, pc, t.numerator, t.denominator, lencoef)
    full = (1 << (1 << n)) - 1
    Lt = 0; Et = 0
    st = [full]
    while st:
        A = st.pop()
        b = BS[A]
        if b == 0:
            Et += E[A]
        else:
            Lt += pc[A]; st.append(b); st.append(A ^ b)
    return (Lt, Et)

def envelope(n, E, pc, lencoef, tmax=Fr(400)):
    def lv(pr, t): return Fr(lencoef)*pr[0] + t*pr[1]
    found = set()
    def rec(t0, p0, t1, p1):
        if p0 == p1 or p0[1] == p1[1]: return
        ts = Fr(lencoef)*(p1[0]-p0[0])/(p0[1]-p1[1])
        if not (t0 < ts < t1): return
        pm = solve_pair(n, E, pc, ts, lencoef)
        if lv(pm, ts) < lv(p0, ts):
            found.add(pm); rec(t0, p0, ts, pm); rec(ts, pm, t1, p1)
    p0 = solve_pair(n, E, pc, Fr(0), lencoef)
    p1 = solve_pair(n, E, pc, tmax, lencoef)
    found.add(p0); found.add(p1)
    rec(Fr(0), p0, tmax, p1)
    return sorted(found)

def run(name, n, w, d):
    t0 = time.time()
    E2, pc = build(n, w, d)
    Nn = 1 << n
    out = {}
    for alpha in (3, 2):
        pairs = envelope(n, E2, pc, alpha)
        lines = [(Fr(alpha)*(1+Fr(L, Nn)), Fr(Ee, Nn*d*d), L, Ee) for (L, Ee) in pairs]
        # prune to lower envelope vertices
        def F(t): return min(a+b*t for a, b, *_ in lines)
        out[alpha] = lines
        print(f"\n{name} alpha={alpha}: pairs={pairs}")
        for a, b, L, Ee in lines:
            print(f"    line {a} + {b}*t   [ell={Fr(L,Nn)}, e2={Fr(Ee,Nn*d*d)}]")
        for T in ([Fr(9)-  Fr(sum(x*x for x in w), d*d), Fr(8)] if alpha == 3 else
                  [Fr(11) - 3*Fr(sum(x*x for x in w), d*d)]):
            cands = sorted(set((T-a)/b for a, b, *_ in lines if b > 0))
            inv = next((c for c in cands if c >= 0 and F(c) >= T), None)
            print(f"    least t with F2_{alpha} >= {T}: {inv}")
    pc2 = Fr(sum(x*x for x in w), d*d)
    M2 = 9 - pc2
    L2 = Fr(5, 2) + Fr(3, 2)*(2 - pc2)
    print(f"  candidate at this vertex: p_c={pc2}  M2={M2}  L2={L2}  2L2={2*L2}")
    print(f"  identity: M={3*(1+n)}  L={1+n}   [n={n}]")
    print(f"  ({time.time()-t0:.0f}s)")
    return out

if __name__ == "__main__":
    run("Q4 uniform", 4, (1, 1, 1, 1), 4)
    run("Q4 down vertex", 4, (2, 1, 1, 1), 5)
    run("Q3 down vertex", 3, (7, 4, 4), 15)
    run("Q3 uniform", 3, (1, 1, 1), 3)
