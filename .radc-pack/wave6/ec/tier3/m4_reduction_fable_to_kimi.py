#!/usr/bin/env python3
"""W6 Tier-3 job M4: reduction attempt Fable -> Kimi, impossibility certificate.

Claim (DR): any gauge-respecting reduction between the two tracks must preserve
the carried-token-weighted expand-count distribution, because that distribution
IS the variable part of the M ledger under the (shared) carried-token accounting:

    sequential 4-turn:  M = 3(1+h) + 2(1+q) N1 + 1(1+q) N2
    batch      3-turn:  M = 2(1+h) + 1(1+q) N1
    (N1,N2 in {0,1}: expand on demand 1 / demand 2; carried factors 3,2,1 and
     2,1 are the locked accounting; verified in m7_accounting.py.)

Fable pi_EDC^2:  N1 = 1 a.s., N2 = 1[S2 != S1]  =>  P(N2=1) = 1 - p_c(theta),
                 E[#exp] = 2 - p_c,  carried count C = 2 + (1-p_c) = 3 - p_c.
Kimi PARITY-DUAL: exactly ONE expand a.s.  =>  #exp = 1 a.s., C = 2 (seq) / 1 (batch).

On any full-support theta (n >= 2), p_c < 1, hence P(#exp=2) = 1-p_c > 0 for
Fable while Kimi is Dirac at 1: the distributions differ => NO reduction.

EC: enumerate the exact distributions at several thetas; verify the invariant
ranges; verify that matching even the MEAN carried count forces p_c = 1.
"""
from fractions import Fraction as F

def p_c(theta):
    return sum(t * t for t in theta)

def fable_expand_dist(theta):
    """distribution of #expands for pi_EDC^2: {1: p_c, 2: 1-p_c}."""
    pc = p_c(theta)
    return {1: pc, 2: 1 - pc}

def fable_carried_dist(theta):
    """carried-token-weighted count C = 2*N1 + 1*N2 (seq): {2: p_c, 3: 1-p_c}."""
    pc = p_c(theta)
    return {2: pc, 3: 1 - pc}

KIMI_SEQ_EXP = {1: F(1)}
KIMI_SEQ_CARRIED = {2: F(1)}
KIMI_BATCH_CARRIED = {1: F(1)}

THETAS = {
    "uniform n=4 (1/4)^4": (F(1, 4),) * 4,
    "Theta_4^down vertex (2/5,1/5,1/5,1/5)": (F(2, 5), F(1, 5), F(1, 5), F(1, 5)),
    "near-Dirac (97/100,1/100,1/100,1/100)": (F(97, 100), F(1, 100), F(1, 100), F(1, 100)),
}

print("[EC] expand-count distributions (the invariant any reduction must preserve)")
for name, th in THETAS.items():
    pc = p_c(th)
    ed = fable_expand_dist(th)
    cd = fable_carried_dist(th)
    e_exp = sum(k * v for k, v in ed.items())
    e_car = sum(k * v for k, v in cd.items())
    assert e_exp == 2 - pc and e_car == 3 - pc
    assert sum(ed.values()) == 1 and sum(cd.values()) == 1
    print(f"  theta = {name}")
    print(f"    p_c = {pc};  Fable #exp dist {ed}  E = {e_exp};  carried C dist {cd}  E = {e_car}")
    print(f"    Kimi  #exp dist {KIMI_SEQ_EXP}  E = 1;  carried C dist {KIMI_SEQ_CARRIED} (seq)")
    if pc < 1:
        assert ed[2] > 0 and ed != KIMI_SEQ_EXP and cd != KIMI_SEQ_CARRIED
        print(f"    -> P(#exp=2) = {ed[2]} > 0 vs Kimi Dirac at 1:  DISTRIBUTIONS DIFFER  PASS")
    else:
        print("    -> p_c = 1 (Dirac): degenerate coincidence only")

# Mean-matching attempt: E[C_fable] = 3 - p_c = 2 = E[C_kimi seq]  <=>  p_c = 1.
# E[C_fable] = 1 = E[C_kimi batch] <=> p_c = 2, impossible (p_c <= 1).
print("\n[EC] mean-matching equations:")
print("  3 - p_c = 2  <=>  p_c = 1  (Dirac only, excluded by full support)")
print("  3 - p_c = 1  <=>  p_c = 2  (impossible: p_c = sum theta_i^2 <= (sum theta_i)^2 = 1)")
for pc_test in (F(7, 25), F(1, 4), F(1)):
    assert (3 - pc_test == 2) == (pc_test == 1)
print("  verified for p_c in {7/25, 1/4, 1}  PASS")

# Range of the invariant on full support: p_c in (0,1) achievable interior,
# p_c <= 7/25 on Theta_4^down (m3), so:
#   Fable E[#exp] in [2-7/25, 2) = [43/25, 2) on Theta_4^down, Kimi = 1 a.s.
assert 2 - F(7, 25) == F(43, 25) and 2 - F(1, 4) == F(7, 4)
print("\n[EC] invariant ranges on Theta_4^down: Fable E[#exp] in [43/25, 7/4], Kimi = 1")
print("  43/25 = 1.72 > 1 and 7/4 = 1.75 > 1:  NO overlap with Kimi's a.s. value  PASS")

print("\nM4 RESULT: reduction Fable -> Kimi IMPOSSIBLE (EC+DR).")
print("Gauge-respecting reductions preserve the carried expand-count distribution;")
print("Fable's is supported on {1,2} with P(2)=1-p_c>0 on full support, Kimi's is")
print("Dirac at 1.  Coincidence requires p_c=1 (Dirac theta), outside every")
print("full-support polytope Theta_n^down.  QED.")
