#!/usr/bin/env python3
"""W6 Tier-5 jobs B1+B2: audit Fable ANTI-OPT / Delta_m / mod-8 tie law and
Kimi LPP-OPT (Rademacher max-identity, codebook counts).
All arithmetic exact (Fraction). Sources: X ~ Unif({0,1}^n); Theta_n^down heavy
vertex weights w = (n+4, 4,...,4), d = 5n.
"""
from fractions import Fraction
from math import comb, gcd

def S_fable(m, a):
    return sum(comb(m, k) * min(4 * k, a + 4 * m - 4 * k) for k in range(m + 1))

def e_anti_fable(n):
    a = n + 4
    return Fraction(2 * S_fable(n - 1, a), (2 ** n) * 5 * n)

def B_kimi(n):
    # B(n) = E[(8K - 5n)^+], K ~ Bin(n-1, 1/2)
    return Fraction(sum(comb(n - 1, k) * max(0, 8 * k - 5 * n) for k in range(n)),
                    2 ** (n - 1))

def e_anti_kimi(n):
    return Fraction(2 * (n - 1) - B_kimi(n), 5 * n)

def gval(b, m, n):
    """Rademacher support functional value g_n(S) for S = (b heavy coords) + m lights."""
    a = n + 4
    if b == 1:
        # E max(a, 4|Y_m|) via max-identity E|aR + 4Y| = E max(a, 4|Y|)
        return Fraction(sum(comb(m, k) * max(a, 4 * abs(2 * k - m)) for k in range(m + 1)),
                        2 ** m)
    else:
        return Fraction(sum(comb(m, k) * 4 * abs(2 * k - m) for k in range(m + 1)),
                        2 ** m)

def e_class(b, m, n):
    """Error of codebook {p,q} with supp(p XOR q) = (b heavy) + (m lights)."""
    return Fraction(5 * n, 10 * n) - gval(b, m, n) / Fraction(10 * n)

print("=" * 78)
print("B1a. e_anti(n): Fable 2S(n-1)/(2^n*5n)  vs  Kimi [2(n-1)-B(n)]/(5n), n=2..30")
print("=" * 78)
ok = True
for n in range(2, 31):
    ef, ek = e_anti_fable(n), e_anti_kimi(n)
    if ef != ek:
        ok = False
        print(f"  MISMATCH n={n}: fable={ef} kimi={ek}")
print(f"  bridge identity holds exactly for all n=2..30: {ok}")
print("  (algebra: min(u,v)=(u+v-|u-v|)/2 gives Fable e=(5n-E|8K-5n|)/(10n);")
print("   E|Z|=2E[Z^+]-E[Z] with E[8K-5n]=-(n+4) gives Kimi form. Both verified.)")
print()
print("  n | e_anti exact      | s1=1/2-e  | t1=2/s1 (BP1 target) | 4/e (kill comp.)")
claim = {3: Fraction(1,4), 4: Fraction(11,40), 5: Fraction(121,400),
         6: Fraction(5,16), 7: Fraction(145,448), 8: Fraction(43,128)}
for n in range(2, 16):
    e = e_anti_fable(n)
    s1 = Fraction(1, 2) - e
    t1 = 2 / s1
    rk = 4 / e
    tag = ""
    if n in claim:
        tag = "  [matches claimed]" if e == claim[n] else "  *** CLAIM MISMATCH ***"
    print(f"  {n:2d} | {str(e):16s} | {str(s1):9s} | {str(t1):20s} | {str(rk):12s}{tag}")
print()
print("  NOTE: mission brief's t1(5)=800/159 uses 1/2-121/400=159/400 -- WRONG.")
print("  Correct: 1/2-121/400 = 79/400, so t1(5) = 800/79 =", Fraction(800,79), float(Fraction(800,79)))

print()
print("=" * 78)
print("B1b. Enumeration minima numerator 2*S(n-1)  (claimed 30,88,242,600,1450,3440 for n=3..8)")
print("=" * 78)
for n in range(3, 9):
    print(f"  n={n}: 2*S(n-1) = {2*S_fable(n-1, n+4)}")

print()
print("=" * 78)
print("B1c/B2c. Class errors e(b,m), tie law, and Delta_m formula (n<=24)")
print("=" * 78)
print("  tie law check: e(1,n-1) == e(1,n-2)  iff  8|n ;  e(0,m) > e(1,m) strict")
tie_ok, strict_ok = True, True
for n in range(3, 25):
    e1 = e_class(1, n - 1, n)
    e2 = e_class(1, n - 2, n)
    tie = (e1 == e2)
    if tie != (n % 8 == 0):
        tie_ok = False
        print(f"  *** TIE LAW FAIL n={n}: e(1,{n-1})={e1} e(1,{n-2})={e2} tie={tie}")
    if e1 != e_anti_fable(n):
        print(f"  *** e(1,n-1) != e_anti at n={n}")
        tie_ok = False
    for m in range(1, n):
        if not e_class(0, m, n) > e_class(1, m, n):
            strict_ok = False
            print(f"  *** STRICTNESS FAIL n={n} m={m}")
print(f"  mod-8 tie law verified n=3..24: {tie_ok};  b=0 strictly worse: {strict_ok}")
print()
print("  Delta_m := e(1,m)-e(1,m+1) (error scale). Formula: 2^{n-m-1}*C(m,k0)*(4-r)")
print("  with c=a+4m, a=n+4, k0 unique in [0,m] with |8k0-c|<4, r=|8k0-c|, else 0.")
for n in [5, 8, 12]:
    scale = None
    rows = []
    for m in range(0, n - 1):
        dm = e_class(1, m, n) - e_class(1, m + 1, n)
        c = (n + 4) + 4 * m
        ks = [k for k in range(0, m + 1) if abs(8 * k - c) < 4]
        if len(ks) == 1:
            k0 = ks[0]; r = abs(8 * k0 - c)
            formula = Fraction(2 ** (n - m - 1) * comb(m, k0) * (4 - r))
        else:
            formula = Fraction(0)
        if formula > 0 and dm > 0 and scale is None:
            scale = dm / formula
        pred = (scale * formula) if scale is not None else None
        rows.append((m, dm, formula, pred))
    # verify with recovered scale (rows before first nonzero carry pred=None; treat as 0==0)
    good = all((pr is None and dmv == 0) or pr == dmv for (_, dmv, _, pr) in rows)
    print(f"  n={n}: recovered scale = {scale} ; all m match formula*scale: {good}")
    for m, dmv, formula, pr in rows:
        print(f"    m={m}: Delta(error)={dmv}  formula={formula}  scale*formula={pr}")

print()
print("=" * 78)
print("B2a. Rademacher max-identity E|Y+wR| = E max(|Y|,|w|): EC sanity (exact)")
print("=" * 78)
import itertools
def rademacher_check(trials=2000):
    import random
    random.seed(5)
    for _ in range(trials):
        m = random.randint(0, 6)
        ys = [random.randint(-9, 9) for _ in range(m + 1)]
        w = random.randint(1, 12)
        # Y = sum ys_j R_j over independent Rademachers
        lhs = Fraction(0); rhs = Fraction(0)
        for bits in itertools.product([1, -1], repeat=m + 1):
            y = sum(a * b for a, b in zip(ys, bits))
            lhs += Fraction(abs(y + w) + abs(y - w), 2)
            rhs += max(abs(y), w)
        lhs /= 2 ** (m + 1); rhs /= 2 ** (m + 1)
        if lhs != rhs:
            return False
    return True
print(f"  2000 random discrete Y, exact: identity holds = {rademacher_check()}")
print("  Proof (DR): condition on Y=y: (|y+w|+|y-w|)/2 = max(|y|,|w|) since")
print("  |y+w|+|y-w| = (|y|+|w|) + ||y|-|w|| = 2 max(|y|,|w|). QED.")
print("  Monotonicity: g_n(S+{i}) = E max(g-r.v., w_i) >= g_n(S); equality iff")
print("  |Y_S| >= w_i a.s. Hence argmax S=[n] (antipodal). Strictness per-coord EC'd below.")

print()
print("=" * 78)
print("B2b. Direct-enumeration audit of support reduction e({0,q}) = (5n-g_n(supp q))/(10n)")
print("      and codebook counts (n<=8 brute force over prototypes p=0 fixed)")
print("=" * 78)
def popcount(x): return bin(x).count("1")

def code_error(n, q, ties_to_q):
    """Error of codebook {0^n,q} under Hamming-nearest partition (deterministic ties)."""
    N = 2 ** n
    wts = [n + 4] + [4] * (n - 1)
    E = 0
    for cell in (0, 1):
        members = []
        for x in range(N):
            d0 = popcount(x); d1 = popcount(x ^ q)
            c = 0 if d0 < d1 else 1 if d1 < d0 else (1 if ties_to_q else 0)
            if c == cell:
                members.append(x)
        for i in range(n):
            n1 = sum((x >> (n - 1 - i)) & 1 for x in members)
            n0 = len(members) - n1
            E += wts[i] * min(n0, n1)
    return Fraction(E, 5 * n * (2 ** n))

def code_error_wm(n, q, ties_to_B):
    """Error of codebook {0^n,q} under WEIGHTED-majority partition on supp(q)
    (weights w_i; the optimal-assignment partition behind the support functional)."""
    N = 2 ** n
    wts = [n + 4] + [4] * (n - 1)
    S = [i for i in range(n) if (q >> (n - 1 - i)) & 1]
    E = 0
    for cell in (0, 1):
        members = []
        for x in range(N):
            v = sum(wts[i] * (2 * ((x >> (n - 1 - i)) & 1) - 1) for i in S)
            c = 0 if v < 0 else 1 if v > 0 else (0 if ties_to_B else 1)
            if c == cell:
                members.append(x)
        for i in range(n):
            n1 = sum((x >> (n - 1 - i)) & 1 for x in members)
            n0 = len(members) - n1
            E += wts[i] * min(n0, n1)
    return Fraction(E, 5 * n * (2 ** n))

for n in range(2, 9):
    N = 2 ** n
    eanti = e_anti_fable(n)
    opt_q = []
    mismatch = 0
    nearest_worse = 0
    for q in range(1, N):
        S = [i for i in range(n) if (q >> (n - 1 - i)) & 1]
        b = 1 if 0 in S else 0
        m = len(S) - b
        formula = e_class(b, m, n)
        wm = min(code_error_wm(n, q, True), code_error_wm(n, q, False))
        if wm != formula:
            mismatch += 1
            if mismatch <= 3:
                print(f"  n={n} q={q}: weighted-majority={wm} formula={formula}")
        e_p = code_error(n, q, ties_to_q=False)
        if e_p > formula:
            nearest_worse += 1
        if formula == eanti:
            opt_q.append(q)
    expect = n if n % 8 == 0 else 1
    total_pairs = (2 ** (n - 1)) * len(opt_q)
    print(f"  n={n}: weighted-majority vs formula mismatches={mismatch}; "
          f"Hamming-nearest strictly worse in {nearest_worse} cases; "
          f"#optimal q (p=0 fixed) = {len(opt_q)} (expect {expect}); "
          f"total optimal codebooks = {total_pairs} "
          f"= {'n*2^(n-1)' if n % 8 == 0 else '2^(n-1)'}")
print("  n=8 check: 8*2^7 =", 8 * 2 ** 7)
print("  FINDING: support functional e=(5n-g_n(S))/10n = error under WEIGHTED-majority")
print("  (optimal) assignment; Hamming-nearest deterministic tie rules are strictly")
print("  worse for supports containing the heavy coord with 1..n-2 lights (and at")
print("  antipodal support for even n). Mod-8 tie law + codebook counts stand, with")
print("  achievability by weighted-majority codes (n=8: enumerator 3440 attained).")
