# RADC Wave-7 P1 -- exact general-\(n\) sequential full-prefix phase

**W7 ID:** \(\mathrm{W7\mbox{-}SOL\mbox{-}SEQ\mbox{-}DOWN\mbox{-}STAIRCASE}\)  
**Verdict:** PROVED + independently EC-attested under the locked formal ledger.  
**Scope:** \(\Theta_n^\downarrow\), \((\rho,\lambda)=(40,20)\), sequential, every variable-length no-recovery binary prefix policy, randomized hull, joint failure distortion.

## 0. Executive theorem: 16/18/19 resolved

Let \(\Theta_n^\downarrow=\{\theta\in\Delta_{n-1}:\theta_i\ge4/(5n)\}\) and \(v_n=(n+4,4,\ldots,4)/(5n)\). The parity/complement policy has locked ledger \((M,D,L)=(3m+2,0,4)\). It dominates the complete randomized no-recovery prefix hull uniformly over \(\Theta_n^\downarrow\) exactly on
\[
\boxed{\mathcal D_n=
\begin{cases}
\varnothing,&n=2,\\
\{1,\ldots,16\},&n=3,\\
\{1,\ldots,18\},&n=4,5,\\
\{1,\ldots,19\},&n\ge6.
\end{cases}}
\]
Thus, with \(\max\varnothing=0\),
\[
\boxed{m_{\rm crit}(2)=0,\ m_{\rm crit}(3)=16,\ m_{\rm crit}(4)=m_{\rm crit}(5)=18,\ m_{\rm crit}(n)=19\ (n\ge6).}
\]
The \(n=2\) phase is empty because latency kills parity. Values 16, 18, 19 are respectively Q3, Q4/Q5, and the stable all-\(n\ge6\) cutoffs.

## 1. W7 index

| W7 ID | Statement | Tags | Ambition |
|---|---|---|---|
| W7-SOL-OCCUPANCY-PROJECTION | exact \(r\)-leaf projection/no-message moment | PI\|DR\|EC | [F] |
| W7-SOL-PREFIX-SPECTRUM-N | exact \(C_N(r)\), DP checked through \(N=64\) | DR\|EC | [S] |
| W7-SOL-BLOCK-FANO-BARRIER | every nontrivial tree, \(n\ge4,2\le m\le19\) | DR\|EC | [F] |
| W7-SOL-Q3-FULLPREFIX | every Q3 tree through \(m=16\) | PI\|DR\|EC | [F] |
| W7-SOL-SEQ-DOWN-STAIRCASE | complete phase | PI\|DR\|EC | [M] |
| W7-SOL-P1-ATTEST | checker in §8 | EC | [S] |

PI = inherited lock; DR = deduction; EC = exact computation; BE = bounded experiment; SB = speculative bridge. No BE/SB supports the theorem.

## 2. Model lock

Let \(X\sim\mathrm{Unif}(\{0,1\}^n)\), \(S_1,\ldots,S_m\stackrel{\rm iid}{\sim}\theta\), independent. Conditioned on all randomness, the pre-demand prefix transcript partitions \(N=2^n\) sources into \(r\) leaves \(A_j\), depths \(d_j\), and \(\ell=N^{-1}\sum_j|A_j|d_j\). With joint error \(e_T\),
\[
M_T=(m+1)(1+\ell)+40e_T,\quad L_T=1+\ell+c_{\rm comp}+20e_T,\quad D_T=e_T,\quad c_{\rm comp}\ge0.
\]
No correctness oracle/source feedback is available. Dominance is weak in all coordinates with one strict.

## 3. No-message and all-\(n\) tail

Put \(K_m=|\{S_1,\ldots,S_m\}|\). For every deterministic \(r\)-leaf realization,
\[
\boxed{P_T\le\mathbb E_\theta\min\{1,r2^{-K_m}\}.}\tag{3.1}
\]
Conditional on \(K_m=k\), answers leave at most \(2^{n-k}\) compatible words per leaf, hence at most \(\min(2^n,r2^{n-k})\) successful words. For \(r=1\), a prototype attains
\[
\boxed{P_{0,m}(\theta)=\mathbb E2^{-K_m}=2^{-n}\sum_{B\subseteq[n]}\theta(B)^m.}\tag{3.2}
\]
This is symmetric convex/Schur-convex, maximized over \(\Theta_n^\downarrow\) at \(v_n\). Thus
\[
p_{n,m}={1\over2^n(5n)^m}\sum_{k=0}^{n-1}{n-1\choose k}[(n+4+4k)^m+(4k)^m],
\quad G_0(n,m)=40(1-p_{n,m})-(2m+1).\tag{3.3}
\]
For \(m\ge10\),
\[
p_{n,m}-p_{n,m+1}=\mathbb E[Z^m(1-Z)]\le m^m/(m+1)^{m+1}<1/20,
\quad\boxed{20m^m<(m+1)^{m+1}},\tag{3.4}
\]
so \(G_0(n,m)>G_0(n,m+1)\).

| \(n\) | last + \(m\): exact \(G_0\) | first - \(m\): exact \(G_0\) |
|---:|---:|---:|
| 3 | 16: \(845049722020265693/437893890380859375\) | 17: \(-22519522704133297/437893890380859375\) |
| 4 | 18: \(277615146191/762939453125\) | 19: \(-1227337666073/762939453125\) |
| 5 | 18: \(887975035189461090631639/582076609134674072265625\) | 19: \(-254541365995396231447867/582076609134674072265625\) |
| 6 | 19: \(2975301311635846283/19705225067138671875\) | 20: \(-2684852348710641308821/1477891880035400390625\) |

Pad \(v_n\) by zero. For every \(k\le n\),
\[
1/5+4k/(5n)>1/5+4k/[5(n+1)],\tag{3.5}
\]
so \((v_n,0)\succ v_{n+1}\), \(p_{n,m}\ge p_{n+1,m}\), and \(G_0(n,m)\le G_0(n+1,m)\). Exact \(n=6\) signs close every \(n\ge6\), not a bounded extrapolation.

## 4. Exact prefix spectrum

Let \(C_N(r)\) be minimum external path sum. Define \(U(s)=s(a+2)-2^{a+1}\), \(a=\lfloor\log_2s\rfloor\), \(U(1)=0\). For \(1\le d\le\lfloor\log_2r\rfloor\), set
\[
k=2^d,\ q=\lfloor(r-1)/(k-1)\rfloor,\ b=(r-1)-(k-1)q,\quad V_d=(k-1-b)U(q)+bU(q+1).
\]
Then
\[
\boxed{C_N(1)=0,\qquad C_N(r)=\min_d[Nd+V_d(r)].}\tag{4.1}
\]
For fixed depths, one source occupies each leaf and all \(N-r\) extras go to minimum depth \(d\). The \(2^d\) depth-\(d\) subtrees have leaf counts summing to \(r\), one equal to one. Discrete convexity of \(U\) balances the rest; complete subtrees attain the result. Root-split DP agrees for every \(1\le r\le N\le64\). In particular,
\[
C_{16}=(0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64).
\]

## 5. Every nontrivial tree

### 5.1 Q3

Set \(c_r=C_8(r)/8\), \(u_r=\mathbb E_{v_3}\min(1,r2^{-K_m})\), \(b_r=1-u_r\). Then
\[
M_T-(3m+2)\ge(m+1)c_r-(2m+1)+40b_r.\tag{5.1}
\]
The exact threshold
\[
\rho_{\rm PL}(3,m)=\max_{1\le r\le8}{[2m+1-(m+1)c_r]_+\over b_r}<40\quad(3\le m\le16)
\]
has terminal value
\[
\boxed{\rho_{\rm PL}(3,16)=144504983825683593750/3823887026147156267\approx37.790076651737.}\tag{5.2}
\]
For \(m=1,2\), \(M_T\ge[(m+1)/2]F_{3,\downarrow}(80/(m+1))\). Exact Q3 DP gives supported pairs \((0,60),(8,30),(15,16),(24,0)\), seams \(8,15,135/8\), and both required floors equal 8. Hence \(M_T\ge8>5\) and \(M_T\ge12>8\).

### 5.2 Block Fano for all \(n\ge4\)

At \(v_n\),
\[
\kappa_{n,m}=1-[4(n-1)/(5n)]^m+(n-1)[1-((5n-4)/(5n))^m]=\mathbb EK_m.
\]
With \(k=\min(n,m)\), conditional Fano on \(X_{Q_m}\) gives
\[
\boxed{\ell\ge\kappa_{n,m}-H_2(e_T)-ke_T.}\tag{5.3}
\]
A nontrivial prefix has \(\ell\ge1\). Put \(a=m+1\):
\[
\Gamma=M_T-(3m+2)=a(\ell-1)+40e_T-m.\tag{5.4}
\]
For \(m=2,3,4\), the entropy conjugate and \(\kappa_{n,m}\ge\kappa_{4,m}\) give exact positive certificates
\[
\boxed{16159/102400,\quad15561/8000,\quad14957/4000.}\tag{5.5}
\]
For \(5\le m\le19\), if \(e_T>m/40\), (5.4) is positive. Otherwise \(H_2(e_T)<1\), and
\[
\Gamma>\begin{cases}a(\kappa_{n,m}-2)-m,&40-ak\ge0,\\a(\kappa_{n,m}-2-km/40),&40-ak<0.\end{cases}\tag{5.6}
\]
Once \(n\ge m\), \(k=m\) and \(\kappa\) increases with \(n\), so checking \(4\le n\le m\le19\) suffices. The exact global minimum, at \(n=m=19\), is
\[
\boxed{331725854346589385191559240189443183/794428636916437084448554992675781250>0.}\tag{5.7}
\]
Every nontrivial tree therefore has strictly larger \(M\) for \(n\ge4,2\le m\le19\).

## 6. Other coordinates and \(n=2\)

Since joint failure contains first-answer failure,
\[
2L_T\ge2+2\ell+40e_1.\tag{6.1}
\]
At \(n=3\), \(F_{3,\downarrow}(40)=8\) gives \(L_T\ge4\). At \(n\ge4\), coordinate Fano gives
\[
2+2\ell+40e_1\ge\Phi^*_{n,40}=2+2n[1-\log_2(1+2^{-16/n})].
\]
This is nondecreasing, and \(\Phi^*_{4,40}=10-8\log_2(17/16)>8\), equivalently \(17^4<2\cdot16^4\). Thus \(L_T>4\), and the same floor closes \(m=1\): \(M_T>8>5\). Always \(D_T=e_T\ge0\).

At \(n=2\), the legal identity transcript has \((M,D,L)=(3m+3,0,3)\). Since \(3<4\), parity never dominates it, although the no-message memory face is positive through \(m=14\).

## 7. Assembly/randomization/failures

Q3: exact floors handle \(m=1,2\), (5.1)-(5.2) every nontrivial tree through 16, and §3 the no-message leaf. Q4/Q5: §6 handles \(m=1\), block Fano through 18, §3 the no-message leaf. For \(n\ge6\), the same works through 19 with the majorization tail.

Condition on the full policy seed. All deterministic realizations satisfy uniform affine bounds with strict positive \(M\) margin, so averaging preserves strict dominance over the randomized hull.

A no-message prototype succeeds with probability at least \(2^{-n}\), hence
\[
M_0-M_{\rm par}\le39-2m-40/2^n.\tag{7.1}
\]
At \(n=3,m=17\), the universal bound ties but the exact heavy-vertex gap is negative; (7.1) is negative from 18. It is negative from 19 at \(n=4,5\), and from 20 at \(n\ge6\). Together with the \(n=2\) identity, both directions follow.

## 8. Portable exact checker

Python 3 stdlib only; exact integer/`fractions.Fraction` assertions. It checks spectrum versus independent DP through \(N=64\), endpoint fractions, majorization arithmetic, Q3 projection, all block-Fano inequalities, obstructions, and latency. Symbolic obligations are (3.5), derivation (5.3), monotonicity after \(n\ge m\), and seed conditioning.

```python
#!/usr/bin/env python3
from fractions import Fraction as F
from functools import lru_cache
from math import comb


def U(r: int) -> int:
    if r == 1:
        return 0
    a = r.bit_length() - 1
    return r * (a + 2) - (1 << (a + 1))


def C_closed(N: int, r: int) -> int:
    if r == 1:
        return 0
    vals = []
    for d in range(1, r.bit_length()):
        k = 1 << d
        q, b = divmod(r - 1, k - 1)
        vals.append(N * d + (k - 1 - b) * U(q) + b * U(q + 1))
    return min(vals)


def C_dp(Nmax: int) -> list[list[int]]:
    inf = 10**100
    C = [[inf] * (Nmax + 1) for _ in range(Nmax + 1)]
    for N in range(1, Nmax + 1):
        C[N][1] = 0
    for N in range(2, Nmax + 1):
        for r in range(2, N + 1):
            C[N][r] = min(
                N + C[a][u] + C[N-a][r-u]
                for a in range(1, N)
                for u in range(max(1, r-(N-a)), min(a, r-1)+1)
            )
    return C


def occ_heavy(n: int, m: int) -> tuple[list[int], int]:
    H, L, W = n + 4, 4, 5 * n
    dp = {(0, 0): 1}
    for _ in range(m):
        nd = {}
        for (seen_h, j), z in dp.items():
            nd[(1, j)] = nd.get((1, j), 0) + z * H
            if j:
                nd[(seen_h, j)] = nd.get((seen_h, j), 0) + z * L * j
            if j < n - 1:
                nd[(seen_h, j+1)] = nd.get((seen_h, j+1), 0) + z * L * (n-1-j)
        dp = nd
    by_k = [0] * (n + 1)
    for (a, j), z in dp.items():
        by_k[a+j] += z
    return by_k, W**m


def proj(by_k: list[int], den: int, r: int) -> F:
    return sum(F(z * min(1 << k, r), den * (1 << k)) for k, z in enumerate(by_k))


def p0(n: int, m: int) -> F:
    by_k, den = occ_heavy(n, m)
    return proj(by_k, den, 1)


def gap0(n: int, m: int) -> F:
    return 40 * (1 - p0(n, m)) - (2 * m + 1)


def pl_rho(n: int, m: int) -> tuple[F, int]:
    N = 1 << n
    by_k, den = occ_heavy(n, m)
    best, arg = F(0), 1
    for r in range(1, N + 1):
        b = 1 - proj(by_k, den, r)
        c = F(C_closed(N, r), N)
        x = F(2*m+1) - (m+1)*c
        q = F(0) if x <= 0 else x / b
        if q > best:
            best, arg = q, r
    return best, arg


def kappa(n: int, m: int) -> F:
    H, L = F(n+4, 5*n), F(4, 5*n)
    return 1 - (1-H)**m + (n-1)*(1-(1-L)**m)


def q3_leaf_error(mask: int) -> int:
    weights = (7, 4, 4)
    size = mask.bit_count()
    return sum(weights[i] * min(
        sum(1 for x in range(8) if (mask >> x) & 1 and (x >> i) & 1),
        sum(1 for x in range(8) if (mask >> x) & 1 and not ((x >> i) & 1)),
    ) for i in range(3))


def prune(pairs):
    out, best_e = [], 10**100
    for L, E in sorted(pairs):
        if E < best_e:
            out.append((L, E))
            best_e = E
    return tuple(out)


@lru_cache(None)
def q3_pairs(mask: int):
    pairs = {(0, q3_leaf_error(mask))}
    first = mask & -mask
    sub = (mask - 1) & mask
    while sub:
        if (sub & first) and sub != mask:
            other = mask ^ sub
            for L1, E1 in q3_pairs(sub):
                for L2, E2 in q3_pairs(other):
                    pairs.add((mask.bit_count() + L1 + L2, E1 + E2))
        sub = (sub - 1) & mask
    return prune(pairs)


def q3_floor(t: F) -> F:
    return min(2 + F(2*L, 8) + t*F(E, 8*15) for L, E in q3_pairs(255))


assert q3_floor(F(40)) == 8
assert q3_floor(F(80,3)) == 8


C = C_dp(64)
assert all(C[N][r] == C_closed(N, r) for N in range(1,65) for r in range(1,N+1))
assert [C_closed(16,r) for r in range(1,17)] == [0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64]

expected = {
 (3,16): F(845049722020265693,437893890380859375),
 (3,17): F(-22519522704133297,437893890380859375),
 (4,18): F(277615146191,762939453125),
 (4,19): F(-1227337666073,762939453125),
 (5,18): F(887975035189461090631639,582076609134674072265625),
 (5,19): F(-254541365995396231447867,582076609134674072265625),
 (6,19): F(2975301311635846283,19705225067138671875),
 (6,20): F(-2684852348710641308821,1477891880035400390625),
}
assert {k: gap0(*k) for k in expected} == expected
assert all(gap0(3,m)>0 for m in range(1,17)) and gap0(3,17)<0
assert all(gap0(4,m)>0 for m in range(1,19)) and gap0(4,19)<0
assert all(gap0(5,m)>0 for m in range(1,19)) and gap0(5,19)<0
assert all(gap0(6,m)>0 for m in range(1,20)) and gap0(6,20)<0
assert all(20*m**m < (m+1)**(m+1) for m in range(10,20))
# (v_n,0) majorizes v_(n+1): first-k partial-sum difference is positive.
assert all(F(4*k,5*n) > F(4*k,5*(n+1)) for n in range(2,1001) for k in range(1,n+1))

rhos = [pl_rho(3,m) for m in range(3,17)]
assert all(rho < 40 for rho, _ in rhos)
assert rhos[-1][0] == F(144504983825683593750,3823887026147156267)

small_expected = {
 2: F(16159,102400),
 3: F(15561,8000),
 4: F(14957,4000),
}
for m, x_upper in [(2,F(1,2048)),(3,F(1,128)),(4,F(1,16))]:
    a = m+1
    cert = a*(kappa(4,m)-1)-m-a*F(3,2)*x_upper
    assert cert == small_expected[m] and cert > 0

minimum = None
for m in range(5,20):
    a = m+1
    for n in range(4,m+1):
        k = min(n,m)
        if 40-a*k >= 0:
            B = a*(kappa(n,m)-2)-m
        else:
            B = a*(kappa(n,m)-2-F(k*m,40))
        assert B > 0
        if minimum is None or B < minimum[0]:
            minimum = (B,n,m)
assert minimum == (F(331725854346589385191559240189443183,
                     794428636916437084448554992675781250),19,19)

# Exact failures from one legal no-message prototype.
assert F(39-2*17)-F(40,8) == 0 and gap0(3,17) < 0
assert F(39-2*19)-F(40,16) < 0
assert F(39-2*19)-F(40,32) < 0
assert F(39-2*20)-F(40,64) < 0
# n=2 identity latency is 3 versus parity latency 4.
assert 1+2 < 4
# For n>=4, Phi_4,40 > 8 follows from log2(17/16)<1/4.
assert 17**4 < 2*16**4

print('PASS W7-SOL-P1 checker')
print('mcrit: n=2 empty/0; n=3 16; n=4,5 18; n>=6 19')
print('Q3 terminal rho_PL:', rhos[-1][0])
print('block-Fano minimum:', minimum)
for k in expected:
    print('G0', k, expected[k])

```

Observed: `PASS W7-SOL-P1 checker`; `mcrit: n=2 empty/0; n=3 16; n=4,5 18; n>=6 19`; Q3 terminal threshold `144504983825683593750/3823887026147156267`; block-Fano minimum (5.7).

## 9. Peer adjudication and review findings

| Peer/path | Contribution | Verdict |
|---|---|---|
| SOLPRO_W5_CONT2: `10_SOLPRO_W5_CONT2.md`, `12...py`, `13...cpp` | Q4 18/19 | survives; PASS |
| SOLPRO_W6: `21_SOLPRO_W6_THEORY.txt:520-1944`, `23_SOLPRO_W6_CHECKS.py` | staircase/block Fano | strongest complete peer proof |
| KIMI_W6: `31_KIMI_W6_PACKAGE.md:161-280` | same staircase/leaf barrier | conclusion right; attestation incomplete |
| KIMIK3: `41_KIMIK3_THINKING_W6_PACKAGE.md:241-260` | full Q3, Q5 partial | Q3 survives; Q5 superseded |
| DEEPSEEK: `42_DEEPSEEK_W6_PACKAGE.md:151-200`, `checkers/tier2/g2_spectra.py`, `g7_n3_phase.py` | finite spectra/Q3 | survives |
| GROK: `54_GROK_W6_03_PROOFS.md:41-205`, `checkers/w6_cont2_generalize.py` | coverage/Q3 no-message | correct partial |
| QWEN: `61_QWEN_W6_PACKAGE.md:181-400,461-650` | \(n=3..6\) endpoints | values right; proof holes repaired |

1. **major:** `peers/KIMI_W6/w6/w6_genn_checks.py:173-218` checks only through \(n=2000\), while `31_KIMI_W6_PACKAGE.md:201-208` states all \(n\ge6\). Exact majorization repairs it.
2. **major:** `peers/KIMI_W6/w6/w6_genn_checks.py:571-589,641-692` uses only 256 leaf-mass values for \(n\ge10\), then `31_KIMI_W6_PACKAGE.md:221-240` promotes to every tree. Block Fano repairs it.
3. **major:** `61_QWEN_W6_PACKAGE.md:521-535` stores the largest passing \(m\), not a contiguous phase. Its coverage barrier fails at \(n=6,m=10..14\); `61...:361-365` inherits only \(m\le9\). §5.2 closes the gap.
4. **minor:** `25_SOLPRO_W6_CHECKS_CPP.out` has no corresponding W6 C++ source. Python, W5 C++, and §8 independently cover arithmetic.
5. **minor:** `peers/KIMI_W6/w6/w6_genn_checks.py:1075` writes to absolute `/mnt/agents/output/w6/W6_GENN_EC_LOG.md`.

No blocker exists in Core/Cont-2 or the final theorem.

## 10. Dependencies/nonclaims

**Dependencies:** PI only for locked ledger/parity semantics; DR here for projection, Schur extremizer, spectrum, Fano, majorization, randomization; EC for Q3 floor/projection, endpoints, sweep, DP. `01_RADC_FORMAL_CORE_V1_FREEZE.md:73-96` is scope, not arbitrary-\(n\) proof.

**Nonclaims:** no \(n=1\), general cap/band class, other gauge, interior no-message optimality, production TokenZero/tokenizer/opacity/corridor/security/real-agent result, or Fable/Kimi identification.

## 11. Validation

- `python3 12_SOLPRO_W5_CONT2_CHECKS.py`: PASS, including \(C_{16}\), \(p_{10}=6560848/9765625\), five \(B_r\), endpoints, \(-3/2\).
- `c++ -std=c++20 -O2 13_SOLPRO_W5_CONT2_CHECKS.cpp ...`: PASS.
- `python3 23_SOLPRO_W6_CHECKS.py`: `ALL W6 THEORY CHECKS PASS`.
- Grok and DeepSeek/KimiK3 relevant P1 checkers: PASS.
- `python3 /tmp/w7_sol_p1_check.py`: PASS.

```acceptance-report
{
  "criteriaSatisfied": [{"id":"criterion-1","status":"satisfied","evidence":"Path/severity findings in section 9; theorem, proof, checker, and reruns in sections 0-8 and 11."}],
  "changedFiles": ["/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/95aaed91-4b81-4b2b-b1a5-73c1458670c3/analysis-xhigh/70_p1_general_n.md"],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {"command":"python3 12_SOLPRO_W5_CONT2_CHECKS.py","result":"passed","summary":"All exact Cont-2 certificates passed."},
    {"command":"c++ -std=c++20 -O2 13_SOLPRO_W5_CONT2_CHECKS.cpp -o /tmp/w7_cont2_cpp && /tmp/w7_cont2_cpp","result":"passed","summary":"Independent C++ Cont-2 certificate passed."},
    {"command":"python3 23_SOLPRO_W6_CHECKS.py","result":"passed","summary":"All W6 checks passed, including staircase and block Fano."},
    {"command":"python3 peers/GROK_W6/checkers/w6_cont2_generalize.py; python3 peers/DEEPSEEK_W6/checkers/tier2/g2_spectra.py; python3 peers/DEEPSEEK_W6/checkers/tier2/g7_n3_phase.py","result":"passed","summary":"Relevant peer checks passed."},
    {"command":"python3 /tmp/w7_sol_p1_check.py","result":"passed","summary":"Independent W7 checker certified 0/16/18/18/19."}
  ],
  "validationOutput": ["PASS W7-SOL-P1 checker","mcrit: n=2 empty/0; n=3 16; n=4,5 18; n>=6 19","Q3 rho_PL exact","block-Fano minimum exact positive"],
  "residualRisks": ["W6 C++ output lacks source; Python and embedded W7 checker cover arithmetic.","Result is conditional on locked formal semantics, not production."],
  "noStagedFiles": true,
  "diffSummary": "Added one analysis artifact; no source files changed.",
  "reviewFindings": ["major: peers/KIMI_W6/w6/w6_genn_checks.py:173-218 - finite tail promoted all-n; repaired.","major: peers/KIMI_W6/w6/w6_genn_checks.py:571-692 - restricted leaf grid promoted every-tree; repaired.","major: 61_QWEN_W6_PACKAGE.md:521-535 - noncontiguous checker leaves n=6,m=10..14 unproved; repaired.","minor: 25_SOLPRO_W6_CHECKS_CPP.out - no source.","minor: peers/KIMI_W6/w6/w6_genn_checks.py:1075 - absolute path.","no blockers in final theorem"],
  "manualNotes": "Source bundle remained read-only. Only this artifact was written."
}
```
