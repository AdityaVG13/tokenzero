#!/usr/bin/env python3
"""W5 Part A: independent exact verification of W4 single-demand floors."""
from fractions import Fraction as Fr

def make_tables_fast(n, w):
    N = 1 << n; size = 1 << N
    ones = [[0]*size for _ in range(n)]
    for i in range(n):
        oi = ones[i]
        for A in range(1, size):
            lb = A & (-A); x = lb.bit_length() - 1
            oi[A] = oi[A ^ lb] + ((x >> i) & 1)
    pc = [0]*size
    for A in range(1, size):
        pc[A] = pc[A & (A-1)] + 1
    E = [0]*size
    for A in range(size):
        tot = 0; k = pc[A]
        for i in range(n):
            c1 = ones[i][A]
            tot += w[i] * (c1 if 2*c1 <= k else k - c1)
        E[A] = tot
    return E, pc

def scalar_dp(n, E, pc, p, q, lencoef=2, count_ops=False):
    """min over prefix trees of lencoef*q*Ltot + p*Etot; returns (val, best-split table, ops)."""
    size = 1 << (1 << n)
    G = [0]*size; BS = [0]*size  # BS: 0 -> leaf, else submask of best split
    ops = 0
    lq = lencoef * q
    for A in range(1, size):
        k = pc[A]
        best = p * E[A]; bs = 0
        if k >= 2:
            base = lq * k
            lb = A & (-A)
            rest = A ^ lb
            s = rest
            while True:
                s = (s - 1) & rest
                Bm = s | lb
                if Bm != A:
                    if count_ops: ops += 1
                    val = base + G[Bm] + G[A ^ Bm]
                    if val < best:
                        best = val; bs = Bm
                if s == 0:
                    break
        G[A] = best; BS[A] = bs
    return G, BS, ops

def extract_pair(n, BS, pc):
    """walk best-split tree from full set -> (Ltot, Etot-needs-E) ; returns (Ltot, leaves list)."""
    full = (1 << (1 << n)) - 1
    Lt = 0; leaves = []
    stack = [full]
    while stack:
        A = stack.pop()
        b = BS[A]
        if b == 0:
            leaves.append(A)
        else:
            Lt += pc[A]
            stack.append(b); stack.append(A ^ b)
    return Lt, leaves

def solve_pair(n, E, pc, t, lencoef=2):
    G, BS, _ = scalar_dp(n, E, pc, t.numerator, t.denominator, lencoef)
    Lt, leaves = extract_pair(n, BS, pc)
    Et = sum(E[A] for A in leaves)
    return (Lt, Et)

def envelope(n, E, pc, lencoef=2, tmax=Fr(400)):
    def lv(pair, t): return Fr(lencoef)*pair[0] + t*pair[1]
    found = set()
    def rec(t0, p0, t1, p1):
        if p0 == p1 or p0[1] == p1[1]:
            return
        ts = Fr(lencoef)*(p1[0]-p0[0]) / (p0[1]-p1[1])
        if not (t0 < ts < t1):
            return
        pm = solve_pair(n, E, pc, ts, lencoef)
        if lv(pm, ts) < lv(p0, ts):
            found.add(pm)
            rec(t0, p0, ts, pm); rec(ts, pm, t1, p1)
    p0 = solve_pair(n, E, pc, Fr(0), lencoef)
    p1 = solve_pair(n, E, pc, tmax, lencoef)
    found.add(p0); found.add(p1)
    rec(Fr(0), p0, tmax, p1)
    # lower-envelope vertices among found lines
    P = sorted(found)
    keep = []
    for pr in P:
        # supported iff strictly best somewhere: check at midpoints of candidate ts grid
        ts_list = [Fr(0), tmax]
        for o in P:
            if o[1] != pr[1]:
                ts_list.append(Fr(lencoef)*(o[0]-pr[0])/(pr[1]-o[1]))
        ts_list = sorted(t for t in set(ts_list) if 0 <= t <= tmax)
        strict = False
        for ta, tb in zip(ts_list, ts_list[1:]):
            tm = (ta+tb)/2
            if all(lv(pr, tm) <= lv(o, tm) for o in P) and any(lv(pr, tm) < lv(o, tm) for o in P if o != pr):
                strict = True
        if strict or len(P) == 1:
            keep.append(pr)
    return keep

def report(name, n, w, d, T_targets=(5, 8, 9, 10)):
    E, pc = make_tables_fast(n, w)
    pairs = envelope(n, E, pc, 2)
    Nn = 1 << n
    lines = [(Fr(2)*(1+Fr(L, Nn)), Fr(Ee, Nn*d), L, Ee) for (L, Ee) in sorted(pairs)]
    print(f"\n{name}  (weights {w}/d={d})")
    print(f"  supported pairs (Ltot,Etot): {sorted(pairs)}")
    for a, b, L, Ee in lines:
        print(f"    F-line: {a} + {b}*t    [ell={Fr(L,Nn)}, e={Fr(Ee,Nn*d)}]")
    bps = []
    for (a1, b1, *_), (a2, b2, *_) in zip(lines, lines[1:]):
        bps.append((a2-a1)/(b1-b2))
    print(f"  breakpoints t: {bps}")
    def F(t): return min(a + b*t for a, b, *_ in lines)
    print(f"  F(40) = {F(Fr(40))}")
    for T in T_targets:
        cands = sorted(set((Fr(T)-a)/b for a, b, *_ in lines if b > 0))
        inv = next((c for c in cands if c >= 0 and F(c) >= T), None)
        print(f"  least t with F >= {T}: {inv}")
    return lines

if __name__ == "__main__":
    import time
    t0 = time.time()
    # op-count verification for cap vertex
    E, pc = make_tables_fast(4, (3, 3, 2, 2))
    G, BS, ops = scalar_dp(4, E, pc, 40, 1, 2, count_ops=True)
    full = (1 << 16) - 1
    print(f"Q4 split-comparison count per scalar run: {ops}  (W4 claims 21,457,825)")
    print(f"Q4 cap G at t=40 (scaled q=1): {G[full]}  -> F = {Fr(2) + Fr(G[full], 16*10)}")
    report("Q4 cap vertex", 4, (3, 3, 2, 2), 10)
    report("Q4 down vertex", 4, (2, 1, 1, 1), 5)
    report("Q4 uniform", 4, (1, 1, 1, 1), 4)
    report("Q3 down vertex", 3, (7, 4, 4), 15)
    report("Q3 uniform", 3, (1, 1, 1), 3)
    print(f"\nelapsed {time.time()-t0:.1f}s")
