#!/usr/bin/env python3
"""W6 Tier-3 job M3: SEPARATING EXAMPLE between MDC-FABLE and MDC-KIMI.

Locked instance: n=4, Q4 gauge (rho,lambda)=(40,20), theta on Theta_4^down,
sequential 4-turn timeline (and batch variant for completeness).

Certifies with exact rational arithmetic (stdlib fractions only):
  * p_c values at the heavy vertex (2/5,1/5,1/5,1/5) and at uniform (1/4)^4;
  * Fable pi_EDC^2 ledger (M,D,L) = (9-p_c, 0, 11/2 - 3*p_c/2)   [PI formulas];
  * Kimi PARITY-DUAL ledgers batch (5,0,4) / seq (8,0,4)          [PI formulas];
  * identity no-recovery baselines batch (10,0,5) / seq (15,0,5)  [PI formulas];
  * ALL pairwise dominance relations at both thetas x both timelines;
  * the verdict separation at the vertex/seq instance:
        Fable L = 127/25 > 5  => FAILS L-dominance vs identity
        Kimi  margins (15-8, 0, 5-4) = (7,0,1) => DOMINATES (given G2(40)=15 [PI]);
  * M-direction: pi_EDC^2 does NOT M-dominate anything PARITY-DUAL does not:
        min over full-support Delta_4 of M(fable) = 9 - max p_c > 8 = M(kimi seq),
        with equality only at the Dirac boundary (p_c=1);
  * max p_c on Theta_4^down = 7/25 (vertex), via exact grid majorization check.

Dominance convention (both camps, locked): weak in each of (M,D,L), >=1 strict.
"""
from fractions import Fraction as F
from itertools import product

# ---------------- locked objects (PI formulas, EC arithmetic) ----------------

def p_c(theta):
    return sum(t * t for t in theta)

def fable_ledger(theta):
    """pi_EDC^2, sequential 4-turn, (h,q,c0,c1)=(1,0,1/2,1/2)."""
    pc = p_c(theta)
    return (F(9) - pc, F(0), F(11, 2) - F(3, 2) * pc)

KIMI_BATCH = (F(5), F(0), F(4))
KIMI_SEQ = (F(8), F(0), F(4))
IDENT_BATCH = (F(10), F(0), F(5))
IDENT_SEQ = (F(15), F(0), F(5))

# Kimi floor data at gauge (40,20), peer-imported (PI), used only for margins:
F2_40, G2_40, H2_40 = F(10), F(15), F(10)

def dominates(a, b):
    """a dominates b: weak in all three coords, strict in >=1."""
    return all(x <= y for x, y in zip(a, b)) and any(x < y for x, y in zip(a, b))

def margins(a, b):
    """componentwise slack of b over a (positive = a better)."""
    return tuple(y - x for x, y in zip(a, b))

def fmt(v):
    return f"({v[0]}, {v[1]}, {v[2]})"

VERTEX = (F(2, 5), F(1, 5), F(1, 5), F(1, 5))
UNIFORM = (F(1, 4),) * 4

# ---------------- core integer checks (headline numbers) ----------------

pc_v, pc_u = p_c(VERTEX), p_c(UNIFORM)
assert pc_v == F(7, 25), pc_v
assert pc_u == F(1, 4), pc_u
print(f"[EC] p_c(vertex)  = 7/25            -> {pc_v}  PASS")
print(f"[EC] p_c(uniform) = 1/4             -> {pc_u}  PASS")

f_v, f_u = fable_ledger(VERTEX), fable_ledger(UNIFORM)
assert f_v == (F(218, 25), F(0), F(127, 25)), f_v
assert f_u == (F(35, 4), F(0), F(41, 8)), f_u
print(f"[EC] Fable M(vertex)  = 9-7/25  = 218/25 -> {f_v[0]}  PASS")
print(f"[EC] Fable L(vertex)  = 11/2-21/50 = 127/25 -> {f_v[2]}  PASS")
print(f"[EC] Fable (unif)     = (35/4, 0, 41/8) -> {fmt(f_u)}  PASS")

# headline separation, sequential timeline, vertex theta:
assert f_v[2] > IDENT_SEQ[2], "Fable L must EXCEED identity L=5"
print(f"[EC] Fable L-vertex 127/25 > 5 = L(identity seq) -> FAILS L-dominance  PASS")
m_kimi_seq = margins(KIMI_SEQ, IDENT_SEQ)
assert m_kimi_seq == (F(7), F(0), F(1)), m_kimi_seq
print(f"[EC] Kimi seq margins vs identity = (7,0,1) -> {m_kimi_seq}  PASS")
# margin consistency with PI floors G2(40)=15 (M-side), H2(40)=10 (2L-side):
assert G2_40 - KIMI_SEQ[0] == 7 and H2_40 / 2 - KIMI_SEQ[2] == 1
print(f"[EC] G2(40)-8 = 7, H2(40)/2-4 = 1  (floors PI, margin arithmetic EC)  PASS")
m_kimi_batch = margins(KIMI_BATCH, IDENT_BATCH)
assert m_kimi_batch == (F(5), F(0), F(1))
assert F2_40 - KIMI_BATCH[0] == 5
print(f"[EC] Kimi batch margins vs identity = (5,0,1); F2(40)-5 = 5  PASS")

# ---------------- full pairwise dominance tabulation ----------------

print("\n[EC] Pairwise dominance table (a DOM b means a weakly <= b in (M,D,L), strict somewhere)")
for tname, theta in (("vertex (2/5,1/5,1/5,1/5)", VERTEX), ("uniform (1/4)^4", UNIFORM)):
    for tl, ident, kimi in (("batch", IDENT_BATCH, KIMI_BATCH), ("seq  ", IDENT_SEQ, KIMI_SEQ)):
        fab = fable_ledger(theta)
        agents = {"identity": ident, "fable": fab, "kimi": kimi}
        print(f"  theta={tname}  timeline={tl}")
        for an, av in agents.items():
            row = []
            for bn, bv in agents.items():
                if an == bn:
                    row.append("  -- ")
                elif dominates(av, bv):
                    row.append(" DOM ")
                elif dominates(bv, av):
                    row.append(" dom ")
                else:
                    row.append(" inc ")
            print(f"    {an:9s} {fmt(av):24s} " + " ".join(row))
        # verdicts
        print(f"    VERDICT fable-vs-identity: "
              f"{'DOMINATES' if dominates(fab, ident) else 'FAILS (L=%s > 5)' % fab[2]} ; "
              f"kimi-vs-identity: {'DOMINATES' if dominates(kimi, ident) else 'fails'}")

# sanity asserts on the full table:
for theta in (VERTEX, UNIFORM):
    fab = fable_ledger(theta)
    for ident in (IDENT_BATCH, IDENT_SEQ):
        assert not dominates(fab, ident)          # Fable never dominates at n=4
        assert fab[0] < ident[0] and fab[2] > ident[2]  # M better, L worse: incomparable
    for kimi in (KIMI_BATCH, KIMI_SEQ):
        assert dominates(kimi, fab)               # Kimi ledger strictly dominates Fable's
print("\n[EC] all 8 (theta x timeline) dominance cells verified: "
      "fable FAILS everywhere at n=4; kimi DOMINATES identity everywhere; "
      "kimi strictly ledger-dominates fable everywhere  PASS")

# ---------------- M-direction audit ----------------
# Does pi_EDC^2 M-dominate anything PARITY-DUAL does not?
# On full-support Delta_4: p_c < 1, and on Theta_4^down p_c <= 7/25 (checked below),
# so M(fable) = 9-p_c >= 9-7/25 = 218/25 > 8 = M(kimi seq).  Answer: NO.
assert F(9) - F(7, 25) > F(8)                    # 218/25 > 8
assert F(9) - F(1) == F(8)                       # equality only at Dirac p_c=1
print("\n[EC] M-direction: min_{Theta_4^down} M(fable) = 218/25 > 8 = M(kimi seq);")
print("[EC] equality M(fable)=8 requires p_c=1 (Dirac, outside full-support polytope)  PASS")

# ---------------- max p_c on Theta_4^down = 7/25 (exact grid certificate) ----------------
# Theta_4^down = {theta_i >= 4/(5n) = 1/5, sum = 1}.  p_c is convex, so its max over
# the polytope is attained at a vertex; vertices are the 4 permutations of
# (2/5,1/5,1/5,1/5).  EC: exact enumeration on a denominator-60 grid.
DEN = 60
lo = DEN // 5  # 12
best = F(0)
count = 0
for a in range(lo, DEN - 3 * lo + 1):
    for b in range(lo, DEN - 2 * lo - a + 1):
        for c in range(lo, DEN - lo - a - b + 1):
            d = DEN - a - b - c
            if d < lo:
                continue
            count += 1
            th = (F(a, DEN), F(b, DEN), F(c, DEN), F(d, DEN))
            best = max(best, p_c(th))
assert count > 0 and best == F(7, 25), (best, count)
print(f"[EC] grid certificate (den={DEN}, {count} pts in Theta_4^down): max p_c = 7/25  PASS")

# min p_c on the open simplex is approached at uniform; exact on the same grid:
DEN2 = 60
bestmin = F(2)
for a in range(1, DEN2):
    for b in range(1, DEN2 - a):
        for c in range(1, DEN2 - a - b):
            d = DEN2 - a - b - c
            if d < 1:
                continue
            th = (F(a, DEN2), F(b, DEN2), F(c, DEN2), F(d, DEN2))
            bestmin = min(bestmin, p_c(th))
assert bestmin == F(1, 4)  # attained at uniform (15,15,15,15)/60
print(f"[EC] grid certificate (den={DEN2}): min p_c on full-support grid = 1/4 (uniform)  PASS")

print("\nM3 RESULT: separating instance CERTIFIED.  Same polytope (Theta_4^down), same gauge")
print("(40,20), same demand law (theta x theta): Fable pi_EDC^2 L=127/25>5 FAILS;")
print("Kimi PARITY-DUAL seq (8,0,4) DOMINATES with margins (7,0,1) [floors PI].")
