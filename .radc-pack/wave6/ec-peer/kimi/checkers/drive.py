#!/usr/bin/env python3
# drive.py — exact-arithmetic driver for RADC Wave-5 certificates.
# Orchestrates the C++ exact DP (w5dp) and pair enumerator (pairs),
# reconstructs lower envelopes exactly with Fraction arithmetic, and checks
# every Wave-4 number plus the new Wave-5 two-demand floors.
import subprocess, math, sys
from fractions import Fraction

BIN = "/tmp/w5/w5dp"
PAIRS = "/tmp/w5/pairs"
LOG = open("/tmp/w5/results.txt", "w")

def out(s=""):
    print(s)
    LOG.write(s + "\n")

def run_dp(mode, n, c, t, w):
    """Exact DP. Returns (V, D, E, count, scale). t = Fraction P/Q."""
    P, Q = t.numerator, t.denominator
    args = [BIN, str(mode), str(n), str(c), str(P), str(Q)] + [str(x) for x in w]
    r = subprocess.run(args, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(r.stderr + r.stdout)
    V, D, E, cnt, scale = r.stdout.split()
    return int(V), int(D), int(E), int(cnt), int(scale)

class Line:
    def __init__(self, a, c, n, Wk, D, E):
        self.a, self.c, self.n, self.Wk, self.D, self.E = a, c, n, Wk, D, E
        self.b = Fraction(a) + Fraction(c * D, 1 << n)   # intercept  a + c*D/2^n
        self.s = Fraction(E, Wk * (1 << n))              # slope = e (per unit t)
    def val(self, t):
        return self.b + self.s * t
    def key(self):
        return (self.D, self.E)
    def __repr__(self):
        return f"Line(D={self.D},E={self.E},b={self.b},s={self.s})"

def eval_dp(mode, n, c, a, t, w, Wk):
    V, D, E, cnt, scale = run_dp(mode, n, c, t, w)
    L = Line(a, c, n, Wk, D, E)
    v = Fraction(V, scale * (1 << n)) + a
    assert L.val(t) == v, f"witness mismatch {L} at t={t}: {L.val(t)} vs {v}"
    return v, L, cnt

def discover(mode, n, c, a, w, Wk, tag):
    """Exact lower-envelope discovery via DP oracle + gift wrapping on breakpoints."""
    W = sum(w)
    evals = []
    def ev(t):
        v, L, cnt = eval_dp(mode, n, c, a, t, w, Wk)
        evals.append((t, v, L.D, L.E))
        return v, L
    v0, L0 = ev(Fraction(0))
    vR, LR = ev(Fraction(10**9))
    assert LR.E == 0, f"{tag}: right end not identity"
    adj = []          # (La, Lb, t*) certified adjacencies
    work = [(L0, LR)]
    while work:
        La, Lb = work.pop()
        if La.s == Lb.s:
            assert La.key() == Lb.key()
            continue
        ts = (Lb.b - La.b) / (La.s - Lb.s)
        assert ts > 0, f"{tag}: nonpositive intersection {ts} for {La} {Lb}"
        v, Lm = ev(ts)
        tent = La.val(ts)
        assert v <= tent
        if v == tent:
            adj.append((La, Lb, ts))   # envelope completeness certificate at t*
        else:
            work.append((La, Lm))
            work.append((Lm, Lb))
    # order the chain from L0 (steepest) to LR (flat)
    nxt = {}
    for La, Lb, ts in adj:
        nxt.setdefault(La.key(), []).append((ts, Lb, La))
    chain = [L0]
    bps = []
    cur = L0
    seen = {L0.key()}
    while cur.key() != LR.key():
        cands = sorted(nxt.get(cur.key(), []))
        assert cands, f"{tag}: chain broken at {cur}"
        ts, Lb, La = cands[0]
        bps.append(ts)
        cur = Lb
        assert cur.key() not in seen
        seen.add(cur.key())
        chain.append(cur)
    # verify breakpoints equal consecutive-line intersections and are increasing
    for i in range(len(chain) - 1):
        ti = (chain[i+1].b - chain[i].b) / (chain[i].s - chain[i+1].s)
        assert ti == bps[i], f"{tag}: breakpoint mismatch {ti} vs {bps[i]}"
        if i: assert bps[i] > bps[i-1]
    return chain, bps, evals

def floor_at(chain, bps, t):
    """Evaluate the envelope at t; return (value, active line, straddle interval)."""
    for i, L in enumerate(chain):
        lo = Fraction(0) if i == 0 else bps[i-1]
        hi = bps[i] if i < len(bps) else None
        if lo <= t and (hi is None or t <= hi):
            return L.val(t), L, (lo, hi)
    raise AssertionError("unreachable")

# ---------------- pure-python independent DP (cross-check) ----------------
def py_dp(mode, n, c, t, w):
    """Independent exact Fraction DP (slow; used as cross-check)."""
    N = 1 << n
    SZ = 1 << N
    W = sum(w)
    if mode == 1:
        M = [0]*SZ
        for i in range(n):
            maski = sum(1 << x for x in range(N) if (x >> i) & 1)
            for A in range(SZ):
                c1 = bin(A & maski).count("1")
                c0 = bin(A).count("1") - c1
                M[A] += w[i] * min(c0, c1)
        Wk = W
    else:
        m2 = [[0]*N for _ in range(N)]
        for p in range(N):
            for x in range(N):
                m = sum(w[i] for i in range(n) if ((p >> i) & 1) == ((x >> i) & 1))
                m2[p][x] = m*m
        # incremental subset sums S(p,A) = S(p, A\lsb) + m2[p][lsb]
        S = [[0]*SZ for _ in range(N)]
        M = [0]*SZ
        pc = 0
        for A in range(1, SZ):
            low = A & (-A)
            x = low.bit_length()-1
            Ap = A ^ low
            pc = bin(A).count("1")
            best = 0
            for p in range(N):
                v = S[p][Ap] + m2[p][x]
                S[p][A] = v
                if v > best: best = v
            M[A] = pc*W*W - best
        Wk = W*W
    popc = [bin(A).count("1") for A in range(SZ)]
    # exact integer scaling: t*M/Wk with t=P/Q -> units 1/scale, scale=Q*Wk/gcd(P,Q*Wk)
    P, Q = t.numerator, t.denominator
    g = math.gcd(P % (Q*Wk), Q*Wk)
    scale = Q*Wk // g
    Pq = P // g
    G = [0]*SZ
    order = sorted(range(SZ), key=lambda A: popc[A])
    cnt = 0
    for A in order:
        k = popc[A]
        best = Pq * M[A]
        if k >= 2:
            base = c * k * scale
            low = A & (-A)
            R = A ^ low
            # submask loop over proper subsets S of R (skip S=R so B != A)
            S = (R - 1) & R
            while True:
                B = S | low
                cnt += 1
                v = base + G[B] + G[A ^ B]
                if v < best:
                    best = v
                if S == 0:
                    break
                S = (S - 1) & R
        G[A] = best
    return Fraction(G[SZ-1], scale), cnt

out("=== RADC Wave-5 exact certificate run ===")
out("harness: g++ -O2 w5dp.cpp (C++ __int128/long long exact integers) + Python Fraction driver")
out("cross-check: independent pure-Python Fraction DP (py_dp)")
out("")

# ============================================================
# A1. Comparison count
# ============================================================
out("--- A1. split-comparison count ---")
V, D, E, cnt, scale = run_dp(1, 4, 2, Fraction(10), (2,2,3,3))
expect = sum(math.comb(16,k)*((1<<(k-1))-1) for k in range(2,17))
out(f"Q4 run split comparisons: {cnt}  (closed form sum_{{k=2}}^16 C(16,k)(2^(k-1)-1) = {expect})  "
    f"{'PASS' if cnt==21457825==expect else 'FAIL'}")

# ============================================================
# A2. Wave-4 supported pairs, floors, envelope completeness
# ============================================================
out("--- A2. Wave-4 envelope reproduction (leaf1, c=2, a=2) ---")

VERTICES = [
    dict(tag="Theta4cap", n=4, w=(2,2,3,3), theta="(1/5,1/5,3/10,3/10)",
         pairs=[(0,80),(16,48),(32,28),(64,0)],
         bps=[Fraction(10), Fraction(16), Fraction(160,7)],
         fchecks=[(Fraction(10), Fraction(7)), (Fraction(16), Fraction(44,5)),
                  (Fraction(40,3), Fraction(8)), (Fraction(120,7), Fraction(9)),
                  (Fraction(160,7), Fraction(10)), (Fraction(40), Fraction(10))]),
    dict(tag="Theta4down", n=4, w=(2,1,1,1), theta="(2/5,1/5,1/5,1/5)",
         pairs=[(0,40),(16,22),(32,12),(64,0)],
         bps=[Fraction(80,9), Fraction(16), Fraction(80,3)],
         fchecks=[(Fraction(40), Fraction(10))]),
    dict(tag="Q4unif", n=4, w=(1,1,1,1), theta="(1/4,1/4,1/4,1/4)",
         pairs=[(0,32),(16,20),(32,12),(42,8),(64,0)],
         bps=[Fraction(32,3), Fraction(16), Fraction(20), Fraction(22)],
         fchecks=[(Fraction(20), Fraction(39,4)), (Fraction(40), Fraction(10))]),
    dict(tag="Theta3down", n=3, w=(7,4,4), theta="(7/15,4/15,4/15)",
         pairs=[(0,60),(8,30),(15,16),(24,0)],
         bps=[Fraction(8), Fraction(15), Fraction(135,8)],
         fchecks=[(Fraction(40), Fraction(8))]),
]

A2_pass = A2_fail = 0
for V_ in VERTICES:
    n, w = V_["n"], V_["w"]
    W = sum(w); Wk = W
    chain, bps, evals = discover(1, n, 2, 2, list(w), Wk, V_["tag"])
    got_pairs = [(L.D, L.E) for L in chain]
    ok_pairs = got_pairs == V_["pairs"]
    ok_bps = bps == V_["bps"]
    out(f"[{V_['tag']}] theta={V_['theta']}  W={W}")
    out(f"  supported pairs (D=2^n*l, E=W*2^n*e): got {got_pairs}")
    out(f"    claimed {V_['pairs']}  ->  {'PASS' if ok_pairs else 'FAIL'}")
    out(f"  breakpoints: got {[str(b) for b in bps]}")
    out(f"    claimed {[str(b) for b in V_['bps']]}  ->  {'PASS' if ok_bps else 'FAIL'}")
    out(f"  policy lines: " + "; ".join(f"2+2*{Fraction(L.D,1<<n)} + t*{Fraction(L.E,Wk*(1<<n))}"
                                       for L in chain))
    A2_pass += ok_pairs + ok_bps; A2_fail += (not ok_pairs) + (not ok_bps)
    # variable-length witness check for (42,8) and (15,16)
    for L in chain:
        if L.D % (1<<n) != 0:
            out(f"  variable-length code certified: D={L.D} -> ell={Fraction(L.D,1<<n)} "
                f"(not a multiple of 2^{n}, hence NOT fixed-depth)  PASS")
            A2_pass += 1
    # envelope completeness: DP value at each breakpoint equals both adjacent lines
    for i, ts in enumerate(bps):
        v, Lm, _ = eval_dp(1, n, 2, 2, ts, list(w), Wk)
        lhs, rhs = chain[i].val(ts), chain[i+1].val(ts)
        ok = (v == lhs == rhs)
        out(f"  envelope completeness @ t={ts}: DP={v} line{i}={lhs} line{i+1}={rhs}  "
            f"{'PASS' if ok else 'FAIL'}")
        A2_pass += ok; A2_fail += not ok
    # claimed floor values
    for t, fexp in V_["fchecks"]:
        v, Lm, _ = eval_dp(1, n, 2, 2, t, list(w), Wk)
        ok = (v == fexp)
        out(f"  F({t}) = {v}  (claimed {fexp})  {'PASS' if ok else 'FAIL'}")
        A2_pass += ok; A2_fail += not ok
    out(f"  DP oracle evaluations for discovery: {len(evals)}")
    out("")

out(f"A2 subtotal: PASS={A2_pass} FAIL={A2_fail}")
out("")

# ============================================================
# A3. e_anti(n) exact rationals, n=3..20
# ============================================================
out("--- A3. e_anti(n) = sum_k C(n-1,k) min{4k,5n-4k} / (5n*2^(n-1)) ---")
TABLE = {3: Fraction(1,4), 4: Fraction(11,40), 5: Fraction(121,400),
         6: Fraction(5,16), 7: Fraction(145,448), 8: Fraction(43,128)}
A3_pass = A3_fail = 0
eanti = {}
for n in range(3, 21):
    num = sum(math.comb(n-1,k) * min(4*k, 5*n-4*k) for k in range(n))
    e = Fraction(num, 5*n*(1 << (n-1)))
    eanti[n] = e
    line = f"  n={n:2d}: e_anti = {e} = {float(e):.6f}"
    if n in TABLE:
        ok = (e == TABLE[n])
        line += f"   table {TABLE[n]}  {'PASS' if ok else 'FAIL'}"
        A3_pass += ok; A3_fail += not ok
    out(line)
out(f"A3 subtotal: PASS={A3_pass} FAIL={A3_fail}")
out("")

# ============================================================
# A4 + A5. one-bit codebook enumeration
# ============================================================
out("--- A4/A5. one-bit two-prototype enumeration at Theta_n down heavy vertex ---")
A45_pass = A45_fail = 0
for n in range(3, 9):
    r = subprocess.run([PAIRS, str(n)], capture_output=True, text=True)
    out("  " + r.stdout.strip())
    kv = dict(tok.split("=") for tok in r.stdout.split() if "=" in tok and "(" not in tok)
    Nstrict, Nmulti = int(kv["Nstrict"]), int(kv["Nmulti"])
    Emin, Eanti = int(kv["Emin"]), int(kv["Eanti"])
    cntMin, cntAntiT, cntNonComp = int(kv["cntMin"]), int(kv["cntAntipodalType"]), int(kv["cntNonComplementTie"])
    N = 1 << n
    ok_cnt = (Nstrict == N*(N-1)//2) and (Nmulti == N*(N+1)//2)
    ok_opt = (Emin == Eanti)
    e_min = Fraction(Emin, 5*n*N)
    ok_table = (Fraction(Eanti, 5*n*N) == eanti[n])
    if n == 5:
        ok4 = (Nstrict == 496 and Emin == 242 and Eanti == 242)
        out(f"  A4 (Q5): C(32,2)=496 strict codebooks; antipodal E=242 at scale 32*25=800; "
            f"Emin={Emin}  {'PASS' if ok4 else 'FAIL'}")
        A45_pass += ok4; A45_fail += not ok4
    verdict = "ANTIPODAL EXACT OPTIMUM" if ok_opt else f"ANTIPODAL SUBOPTIMAL (Emin={Emin} < Eanti={Eanti})"
    out(f"  A5 n={n}: e_min={e_min}  e_anti={Fraction(Eanti,5*n*N)}  -> {verdict}; "
        f"argmins: total={cntMin}, complement-pair type={{p,~p}}={cntAntiT}, "
        f"NON-complement-symmetric ties={cntNonComp}; "
        f"counts {'OK' if ok_cnt else 'BAD'}; table {'PASS' if ok_table else 'FAIL'}")
    A45_pass += ok_opt + ok_cnt + ok_table
    A45_fail += (not ok_opt) + (not ok_cnt) + (not ok_table)
out(f"A4/A5 subtotal: PASS={A45_pass} FAIL={A45_fail}")
out("")

# ============================================================
# A6. Psi check 257*17^3 < 2^21
# ============================================================
out("--- A6. Psi_down_{4,40} > 8 certificate ---")
lhs = 257 * 17**3
rhs = 2**21
ok = (lhs == 1262641) and (lhs < rhs)
out(f"  257*17^3 = {lhs}  vs  2^21 = {rhs}: {lhs} < {rhs}  {'PASS' if ok else 'FAIL'}")
out("")

# ============================================================
# Cross-check: independent pure-Python Fraction DP vs C++
# ============================================================
out("--- Cross-check: py_dp (independent Python Fractions) vs C++ w5dp ---")
XC_pass = XC_fail = 0
# full Theta3-down envelope check at many t
import random
random.seed(5)
tests3 = [Fraction(0), Fraction(8), Fraction(15), Fraction(135,8), Fraction(40),
          Fraction(1,10), Fraction(12), Fraction(100)]
for t in tests3:
    gpy, cntpy = py_dp(1, 3, 2, t, (7,4,4))
    fpy = 2 + gpy/8
    v, L, cntc = eval_dp(1, 3, 2, 2, t, [7,4,4], 15)
    ok = (fpy == v) and (cntpy == cntc == 3025)
    out(f"  Theta3down t={t}: py F={fpy}  C++ F={v}  splits py={cntpy} C++={cntc}  "
        f"{'PASS' if ok else 'FAIL'}")
    XC_pass += ok; XC_fail += not ok
# n=4 spot check at t=40/3 (cap) — the Wave-4 double-implementation point
t = Fraction(40,3)
gpy, cntpy = py_dp(1, 4, 2, t, (2,2,3,3))
fpy = 2 + gpy/16
v, L, cntc = eval_dp(1, 4, 2, 2, t, [2,2,3,3], 10)
ok = (fpy == v == Fraction(8)) and (cntpy == cntc == 21457825)
out(f"  Theta4cap t=40/3: py G*3={gpy*3} F={fpy}  C++ F={v} (claimed 8, scaled G=288)  "
    f"splits py={cntpy} C++={cntc}  {'PASS' if ok else 'FAIL'}")
XC_pass += ok; XC_fail += not ok
# two-demand leaf cross-check at n=3-ish scale is N/A (leaf2 defined n=4); cross-check at t=40 down4
t = Fraction(40)
gpy, cntpy = py_dp(2, 4, 2, t, (2,1,1,1))
fpy = 2 + gpy/16
v, L, cntc = eval_dp(2, 4, 2, 2, t, [2,1,1,1], 25)
ok = (fpy == v) and (cntpy == cntc == 21457825)
out(f"  two-demand Theta4down t=40: py F2_batch={fpy}  C++ F2_batch={v}  "
    f"splits py={cntpy} C++={cntc}  {'PASS' if ok else 'FAIL'}")
XC_pass += ok; XC_fail += not ok
out(f"Cross-check subtotal: PASS={XC_pass} FAIL={XC_fail}")
out("")

# ============================================================
# B7/B8/B9. Two-demand floors (leaf2)
# ============================================================
out("--- B7. two-demand BATCH floor F2_batch (c=2, a=2) ---")
B_pass = B_fail = 0
TWOD = [
    dict(tag="F2batch@Theta4down", mode=2, n=4, c=2, a=2, w=(2,1,1,1), theta="(2/5,1/5,1/5,1/5)"),
    dict(tag="F2batch@Theta4cap",  mode=2, n=4, c=2, a=2, w=(2,2,3,3), theta="(1/5,1/5,3/10,3/10)"),
]
results2 = {}
for V_ in TWOD:
    n, w, c, a = V_["n"], V_["w"], V_["c"], V_["a"]
    W = sum(w); Wk = W*W
    chain, bps, evals = discover(2, n, c, a, list(w), Wk, V_["tag"])
    results2[V_["tag"]] = (chain, bps)
    out(f"[{V_['tag']}] theta={V_['theta']}  W={W}, e2 denominator {Wk}")
    out(f"  supported pairs (D=16*ell, E={Wk}*16*e2): {[(L.D,L.E) for L in chain]}")
    for L in chain:
        ell = Fraction(L.D, 1<<n); e2 = Fraction(L.E, Wk*(1<<n))
        out(f"    ell={ell}  e2={e2}   line: {L.b} + t*{L.s}")
    out(f"  breakpoints: {[str(b) for b in bps]}")
    v, L, (lo, hi) = floor_at(chain, bps, Fraction(40))
    vdp, Lm, _ = eval_dp(2, n, c, a, Fraction(40), list(w), Wk)
    ok = (v == vdp)
    out(f"  F2_batch(40) = {vdp}  (active line ell={Fraction(L.D,1<<n)}, e2={L.s}; "
        f"t=40 in ({lo}, {hi if hi else 'inf'}))  {'PASS' if ok else 'FAIL'}")
    B_pass += ok; B_fail += not ok
    out(f"  discovery evals: {len(evals)}")
    out("")

out("--- B8. two-demand SEQUENTIAL floors at Theta4down: G2 (c=3,a=3), H2 (c=2,a=2) ---")
chainH, bpsH = results2["F2batch@Theta4down"]
chainG, bpsG, evalsG = discover(2, 4, 3, 3, [2,1,1,1], 25, "G2@Theta4down")
results2["G2@Theta4down"] = (chainG, bpsG)
out("[G2@Theta4down] line 3+3*ell+t*e2")
out(f"  supported pairs (D=16*ell, E=25*16*e2): {[(L.D,L.E) for L in chainG]}")
for L in chainG:
    out(f"    ell={Fraction(L.D,16)}  e2={L.s}   line: {L.b} + t*{L.s}")
out(f"  breakpoints: {[str(b) for b in bpsG]}")
v, L, (lo, hi) = floor_at(chainG, bpsG, Fraction(40))
vdp, Lm, _ = eval_dp(2, 4, 3, 3, Fraction(40), [2,1,1,1], 25)
ok = (v == vdp)
out(f"  G2(40) = {vdp}  (active line ell={Fraction(L.D,16)}, e2={L.s}; t=40 in ({lo}, {hi if hi else 'inf'}))  "
    f"{'PASS' if ok else 'FAIL'}")
B_pass += ok; B_fail += not ok
out("[H2@Theta4down] line 2+2*ell+t*e2  (identical DP weighting as F2_batch@Theta4down)")
out(f"  supported pairs: {[(L.D,L.E) for L in chainH]}")
out(f"  breakpoints: {[str(b) for b in bpsH]}")
v, L, (lo, hi) = floor_at(chainH, bpsH, Fraction(40))
vdp, Lm, _ = eval_dp(2, 4, 2, 2, Fraction(40), [2,1,1,1], 25)
ok = (v == vdp)
out(f"  H2(40) = {vdp}  (active line ell={Fraction(L.D,16)}, e2={L.s}; t=40 in ({lo}, {hi if hi else 'inf'}))  "
    f"{'PASS' if ok else 'FAIL'}")
B_pass += ok; B_fail += not ok
out("")

out("--- B9. frontier summary around t=40 ---")
for tag, (chain, bps) in results2.items():
    v, L, (lo, hi) = floor_at(chain, bps, Fraction(40))
    out(f"  {tag}: value(40)={v}; supported (D,E)={[(x.D,x.E) for x in chain]}; "
        f"breakpoints around 40: [{lo}, {hi if hi else '+inf'}]; all bps={[str(b) for b in bps]}")
out(f"B subtotal: PASS={B_pass} FAIL={B_fail}")
out("")
out("=== RUN COMPLETE ===")
LOG.close()
