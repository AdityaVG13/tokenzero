# Wave 7 Sol Pro P2: MDC dual-track master theorem

## 0. Verdict

**PROVED+EC, with two source corrections.** The strongest true result is a common **rank-area ledger theorem** with permanently separate candidate strata:

- **MDC-KIMI** is the positive residual-rank-one parity/complement stratum. Its two-demand sequential critical dimension is exactly **3**.
- **MDC-FABLE** is the opaque per-unique-demand stratum. Its two-demand sequential critical dimension is exactly **5**.
- Among positive-rank **uniform** residual matroids realizable by binary linear handles, the only strata for n >= 3 are \(U_{1,n},U_{n-1,n},U_{n,n}\). The last two have the Fable two-demand ledger and critical dimension 5.
- The tracks share the ledger engine, not the candidate, handle information, expand process, phase theorem, or leaf law. They must retain separate IDs.
- The reported no-recovery envelope coincidence is exact only at six computed rational vertices. It is not policy or theorem equivalence.

No source file was edited. This report is the only written artifact.

## 1. Review findings

1. **medium -- literal rank-stratification scope defect:**
   [31_KIMI_W6_PACKAGE.md:318-323](/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT/31_KIMI_W6_PACKAGE.md#L318) says \(U_{r,n}\) is realizable iff \(r\in\{1,n-1,n\}\), without stating \(1\le r\le n\). Literally, \(U_{0,n}\) is also realized by \(K=\{0\}\). The corrected theorem below is for **positive residual rank**, and “exactly three” also needs n >= 3.
2. **low -- n=2 M-side comparison typo:**
   [peers/KIMI_W6/w6/W6_MDC_EC_LOG.md:109-115](/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT/peers/KIMI_W6/w6/W6_MDC_EC_LOG.md#L109) says \(F^{(2)}_3(40)=9<12\) and “both sides fail.” The sequential parity candidate has \(M=8\), not 12, so its M-side actually passes \(9\ge8\). The L-side \(6<8=2L_{par}\) still kills dominance, so \(n_{crit}=3\) is unaffected.
3. **medium -- provenance gap in the flat bundle:**
   [substrate/cont1/](/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT/substrate/cont1) is empty, and the original W5 Fable/Kimi package prose named by the Core manifest is absent. Cont-1 is present as [17_SOLPRO_W5_CONT1.md](/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT/17_SOLPRO_W5_CONT1.md); Fable/Kimi claims survive through verbatim peer quotations and rerun checkers under [peers/DEEPSEEK_W6/ec-peer-reruns/](/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT/peers/DEEPSEEK_W6/ec-peer-reruns). This is enough to rederive the theorem, but not a perfect primary-source chain.

No blocker was found in the corrected theorem.

## 2. Locked model and names

Use the registered two-demand sequential model:

- \(X\sim\mathrm{Unif}(\mathbb F_2^n)\), \(n\ge2\).
- \(S_1,S_2\stackrel{iid}{\sim}\theta\), independent of X.
- \(\Theta_n^\downarrow=\{\theta:\theta_i\ge4/(5n)\}\), with heavy vertex proportional to \((n+4,4,\ldots,4)\).
- \((h,q,c_0,c_1)=(1,0,1/2,1/2)\), \((\rho,\lambda)=(40,20)\).
- Joint distortion is \(D=\Pr[\)at least one demanded answer is wrong\(]\).
- A no-recovery prefix baseline T has
  \[
  M_T=3(1+\ell_T)+40e_T,\qquad
  L_T=1+\ell_T+c_T+20e_T,\qquad D_T=e_T,
  \]
  where \(c_T\ge0\).
- Dominance is weak in all displayed coordinates and strict in at least one.

**Name lock:**

| ID | Exact candidate | Sequential ledger | Pre-demand information |
|---|---|---:|---:|
| MDC-FABLE | opaque exact reference; expand on each distinct demand; dedup only if \(S_2=S_1\) | \((9-p_c,0,11/2-3p_c/2)\) | \(I_{pre}=0\) |
| MDC-KIMI | parity/complement alias; one residual bit collapses the antipodal fiber | \((8,0,4)\) | \(I_{pre}=n-1\) |

Here \(p_c(\theta)=\sum_i\theta_i^2=\Pr[S_1=S_2]\). Kimi batch \((5,0,4)\) is a separate three-turn ledger. Never compare it to Fable sequential without naming the timeline.

## 3. Theorem index

| New ID | Statement | Status/tags | Dependencies |
|---|---|---|---|
| W7-SOL-MDC-RANK-AREA | Exact linear-handle sequential ledger from residual projection rank | PROVED [DR] | W4-LINEAR-ALIAS-RANK; W5-SOL-RANK-AREA |
| W7-SOL-MDC-UNIFORM-STRAT | Positive-rank binary uniform strata are exactly \(U_1,U_{n-1},U_n\) for n >= 3 | PROVED+EC [DR,EC] | W6-MDC-MDS |
| W7-SOL-MDC-CRIT | \(n_{crit}(U_1)=3\), \(n_{crit}(U_{n-1})=n_{crit}(U_n)=5\) | PROVED+EC [DR,EC] | W4 floors; Fable MDC-3/4; W6 checkers |
| W7-SOL-MDC-SEP | Exact four-objective and phase separation | PROVED+EC [DR,EC] | W6-MDC-OPAQUE-RANK-SEPARATION; W6-GROK-MDC-SEP |
| W7-SOL-MDC-NONRED-FK | No admissible Fable-to-Kimi morphism | PROVED [DR] | data processing |
| W7-SOL-MDC-NONRED-KF | No admissible Kimi-to-Fable morphism preserving rank-one service | PROVED [DR] | conditional entropy |
| W7-SOL-MDC-LEAFCOIN | Scoped exact root-envelope coincidence plus explicit leaf inequivalence | PROVED+EC [EC] | W6-MDC-LEAFCOIN; 66-check DP |
| W7-SOL-MDC-MASTER | Conjunction of the preceding results with permanent dual IDs | PROVED+EC [DR,EC] | all above |

Tags: PI = inherited published input; DR = deductive result; EC = exact computation; BE = bounded experiment; SB = speculative bridge. No result here uses BE or SB as proof.

## 4. Proofs

### 4.1 W7-SOL-MDC-RANK-AREA

Let a binary linear visible handle be \(Z=AX\), with kernel \(K=\ker A\). For a coordinate set \(Q\), define
\[
r_K(Q)=\dim \pi_Q(K).
\]
Conditioned on \(Z=z\), X is uniform on an affine coset \(x_0+K\), so \(X_Q\) is uniform on \(\pi_Q(x_0)+\pi_Q(K)\). Therefore
\[
H(X_Q\mid Z,Q)=r_K(Q).
\]
Every exact binary prefix-free residual message has expected length at least this entropy. Sending coordinates in a basis of \(\pi_Q(K)\) attains it. Thus the exact residual payload for Q is \(r_K(Q)\).

Let \(Q_k=\{S_1,\ldots,S_k\}\). The carried-token schedule then gives
\[
A_K^{(m)}=\mathbb E\sum_{k=1}^m r_K(Q_k),\qquad
B_K^{(m)}=\mathbb E r_K(Q_m),
\]
\[
M_K^{seq}=(m+1)(1+h)+(1+q)A_K^{(m)},
\]
\[
L_K^{seq}=1+h+c_0+(1+q+c_1)B_K^{(m)},\qquad D_K=0.
\]
This is one common accounting law, not a candidate identification.

For \(U_{1,n}\), \(r(Q)=1\) for nonempty Q, so \(A=m,B=1\), giving
\[
(M,D,L)=(3m+2,0,4).
\]
For \(U_{n,n}\), \(r(Q)=|Q|\). At m=2,
\[
\mathbb E|Q_1|=1,\qquad \mathbb E|Q_2|=2-p_c,
\]
so \(A=3-p_c,B=2-p_c\), giving exactly
\[
(M,D,L)=\left(9-p_c,0,{11\over2}-{3p_c\over2}\right).
\]
For n >= 3, \(U_{n-1,n}\) has the same ranks for \(|Q|\le2\), hence the same two-demand ledger, but not the same handle leakage.

### 4.2 W7-SOL-MDC-UNIFORM-STRAT

Restrict to positive ranks \(1\le r\le n\). A subspace K realizes \(U_{r,n}\) exactly when a generator matrix G for K has every set of at most r columns independent, equivalently every r by r minor is nonzero.

For \(2\le r\le n-2\), put G in systematic form \([I_r\mid C]\). Any extra column must have every coordinate nonzero: if coordinate i were zero, that column together with all systematic columns except \(e_i\) would be dependent. Over \(\mathbb F_2\), the only all-nonzero column is \(\mathbf1^r\), so there can be at most one extra column. Hence \(n\le r+1\), contradicting \(r\le n-2\). Therefore no intermediate positive-rank stratum exists.

The survivors are constructive:

- r=1: \(K=\langle\mathbf1^n\rangle\), realizing \(U_{1,n}\).
- r=n-1: generator \([I_{n-1}\mid\mathbf1]\), the even-weight subspace, realizing \(U_{n-1,n}\).
- r=n: \(K=\mathbb F_2^n\), realizing \(U_{n,n}\).

The EC boundary search gives max n = 3,4,5 for fixed r = 2,3,4. The omitted rank-zero stratum is \(K=\{0\}\); it is outside the positive-residual MDC comparison.

### 4.3 W7-SOL-MDC-CRIT

Define \(n_{crit}\) as the least source dimension from which the named sequential candidate dominates the complete randomized variable-length no-recovery prefix hull for every \(\theta\in\Theta_n^\downarrow\) at (40,20).

#### Parity/Kimi stratum \(U_{1,n}\)

The candidate is \((8,0,4)\). For every baseline, joint error \(e_2\ge e_1\), so
\[
2L_T\ge2(1+\ell_T)+40e_1,
\]
\[
M_T\ge[2(1+\ell_T)+40e_1]+(1+\ell_T).
\]
The exact one-demand floors are \(F_{3,\downarrow}(40)=8\) and, by the lower-capped lift, \(F_{n,\downarrow}(40)\ge10\) for n >= 4. Hence:

- n=3: \(L_T\ge4\), \(M_T\ge9>8\). The exact two-demand DP strengthens the M floor to 12, giving margin \((4,0,0)\).
- n >= 4: \(L_T\ge5>4\), \(M_T\ge11>8\).
- n=2: the zero-error identity has \((M,D,L)=(9,0,3)\), so parity fails L-dominance.

Thus \(n_{crit}(U_{1,n})=3\).

#### Opaque/Fable stratum \(U_{n,n}\)

The zero-error identity has \(L=1+n\). Fable L-dominance is equivalent to
\[
{11\over2}-{3p_c\over2}\le1+n
\iff p_c\ge{9-2n\over3}.
\]
On \(\Theta_n^\downarrow\),
\[
\min p_c={1\over n},\qquad
\max p_c={ (n+4)^2+16(n-1)\over25n^2}.
\]
For n=2,3,4 the maxima \(13/25,9/25,7/25\) are below the thresholds \(5/3,1,1/3\), so identity kills the candidate throughout the class. For n >= 5 the threshold is negative.

For the full lossy hull, the Fable certificate proves the one-demand floor
\[
F_{n,\downarrow}(40)\ge11\qquad(n\ge5).
\]
Then for every two-demand baseline,
\[
2L_T\ge F_{n,\downarrow}(40)\ge11
>11-3p_c=2L_F,
\]
while
\[
M_T\ge F_{n,\downarrow}(40)+(1+\ell_T)\ge12>9-p_c=M_F.
\]
Thus Fable strictly dominates the entire lossy hull for n >= 5. Therefore \(n_{crit}(U_{n,n})=5\). Since \(U_{n-1,n}\) has the same two-demand rank-area ledger for n >= 3, its three-objective critical dimension is also 5.

The exact integer floor certificates are:

1. \(3^5=243<256=2^8\).
2. \(129^2 9^8=716340484161<824633720832=3\cdot128^2 8^8\).
3. \(63^3\cdot256=64012032>64000000=400^3\).
4. \(65^2 463^{10}=1912654926642196209234995037025\le3435973836800000000000000000000=8\cdot64^2 400^{10}\).
5. \(27^7=10460353203\ge8589934592=2^{33}\).
6. \(53^7=1174711139837\ge1099511627776=2^{40}\).
7. \(2075^2 309^{12}=3262405609632605408815639167825755625\le10633823966279326983230456482242756608=32\cdot2048^2 256^{12}\).
8. \(125\le128\).
9. \(17^{11}=34271896307633\le35184372088832=2^{45}\).

### 4.4 W7-SOL-MDC-SEP

For every full-support theta,
\[
F=(9-p_c,0,11/2-3p_c/2,0),
\]
\[
K=(8,0,4,n-1),
\]
in coordinates \((M,D,L,I_{pre})\), with information leakage minimized. Since \(p_c<1\),
\[
M_F-M_K=1-p_c>0,
\]
\[
L_F-L_K={3\over2}(1-p_c)>0,
\]
\[
I_F-I_K=-(n-1)<0.
\]
So K strictly dominates F after deleting leakage, while F is strictly better in leakage. They are Pareto-incomparable in four objectives.

At the Q4-down heavy vertex, \(p_c=7/25\):
\[
F=(218/25,0,127/25,0),\qquad K=(8,0,4,3),
\]
with M/L gaps \(18/25,27/25\). Fable fails against identity because \(127/25>5\). Kimi dominates the complete Q4 full-prefix hull with exact margins \((7,0,1)\), from \(F^{(2)}_3(40)=15\) and \(F^{(2)}_2(40)=10\). This is an exact same-lock separator, not a naming dispute.

For scalar weights \(w_M,w_L,w_I\ge0\),
\[
J(F)-J(K)=(1-p_c)(w_M+3w_L/2)-(n-1)w_I.
\]
The preference boundary is a genuine four-objective hyperplane.

### 4.5 W7-SOL-MDC-NONRED-FK and -KF

An **admissible handle morphism** may randomized-postprocess the existing visible handle using public randomness \(U\perp X\); may not introduce new source-dependent pre-demand information; and must preserve the registered exact residual service/rank budget.

**Fable to Kimi.** Fable opacity gives \(I(X;H_F)=0\). For every \(H'=\Phi(H_F,U)\), data processing gives \(I(X;H')=0\). Kimi's parity syndrome has \(I(X;H_K)=n-1\). No admissible postprocessing can create it. Hence no Fable-to-Kimi morphism.

**Kimi to Fable.** Suppose Kimi's handle is postprocessed to an opaque \(H'\), so \(I(X;H')=0\). For distinct i,j,
\[
H(X_i,X_j\mid H')=2.
\]
If one rank-one residual message R exactly recovered both bits, then
\[
2=I(X_i,X_j;R\mid H')\le H(R\mid H')\le1,
\]
a contradiction. Thus opacity and Kimi's one-bit “second demand free” service cannot both be preserved. No Kimi-to-Fable morphism exists.

The expand-process invariant gives the same conclusion on full support: Fable has \(\#exp=1\) with probability \(p_c\) and 2 with probability \(1-p_c>0\); Kimi is identically 1. Mean equality requires \(p_c=1\), a Dirac law excluded by \(\Theta_n^\downarrow\).

These are non-reductions under the explicit admissible category. No claim is made about arbitrary transformations allowed to add new source-dependent resources.

### 4.6 W7-SOL-MDC-LEAFCOIN

The two baseline leaf laws are different:

- Fable/adaptive:
  \[
  E_A(A)=d^2|A|-\sum_iw_i\max_a\sum_jw_j\max_b N_{ij}^{ab}(A).
  \]
- Kimi/prototype:
  \[
  E_B(A)=d^2|A|-\max_p\sum_{x\in A}\left(\sum_sw_s\mathbf1[p_s=x_s]\right)^2.
  \]

The exact prefix DP is
\[
\mathrm{Frontier}(A)=\mathrm{hull}\left(\{(0,E(A))\}\cup
\{(|A|+L_1+L_2,E_1+E_2):B\sqcup C=A\}\right),
\]
and
\[
F^{(2)}_\alpha(t)=\alpha+2^{-n}\min_{(L,E)}\left(\alpha L+tE/d^2\right),
\quad\alpha\in\{2,3\}.
\]

The root supported pairs coincide exactly under both laws:

| Vertex | Weights | Root pairs \((L,E)\) | Leaf disagreements | Intermediate-frontier disagreements |
|---|---|---|---:|---:|
| Q4 uniform | (1,1,1,1) | (0,176),(16,128),(33,80),(42,56),(64,0) | 35880/65536 | 37536/65536 |
| Q4 down | (2,1,1,1) | (0,272),(16,182),(32,108),(64,0) | 36120/65536 | 42152/65536 |
| Q4 cap | (3,3,2,2) | (0,1096),(16,776),(32,492),(64,0) | 36264/65536 | 38668/65536 |
| Q3 down | (7,4,4) | (0,1188),(8,738),(24,0) | 52/256 | 52/256 |
| Q3 uniform | (1,1,1) | (0,48),(8,30),(24,0) | 52/256 | 52/256 |
| Q2 down | (3,2) | (0,62),(8,0) | 0/16 | 0/16 |

Exact alpha=3/alpha=2 breakpoints:

| Vertex | alpha=3 | alpha=2 |
|---|---|---|
| Q4 uniform | 16,17,18,132/7 | 32/3,34/3,12,88/7 |
| Q4 down | 40/3,600/37,200/9 | 80/9,400/37,400/27 |
| Q4 cap | 15,1200/71,800/41 | 10,800/71,1600/123 |
| Q3 down | 12,600/41 | 8,400/41 |
| Q3 uniform | 12,72/5 | 8,48/5 |
| Q2 down | 300/31 | 200/31 |

They are nevertheless not the same leaf law. For
\[
A=\{000,001,010,101\},\quad n=3,\quad w=(1,1,1),
\]
the adaptive correctness score is 19 and prototype score 18, hence
\[
E_A(A)=17<18=E_B(A).
\]

**Why coincidence is not equivalence:** root lower-hull projection is many-to-one. It discards leaf identities, interior frontiers, handle leakage, residual rank, expansion count, and the candidate ledger. The equality is finite and scoped to the listed vertices and \(\alpha\in\{2,3\}\); the n=5 vertex and all-polytope equality were not computed or proved. Equality of these scalar envelopes cannot reverse either non-reduction proof.

## 5. Peer and substrate audit

| Label | Files read | Relevant survivor / verdict |
|---|---|---|
| Formal Core v1 | 01_RADC_FORMAL_CORE_V1_FREEZE.md | Permanent separate MDC IDs; no merge by label |
| W4 Sol Pro | 18_WAVE4_SOLPRO_PACKAGE_FULL.txt, especially W4-LINEAR-ALIAS-RANK and W4-BATCH-PARITY-KILL | Exact residual projection rank; parity kernel has rank one for every nonempty batch |
| SOLPRO_W5_CONT1 | 17_SOLPRO_W5_CONT1.md | Q4 no-message occupancy law and 18/19 face |
| SOLPRO_W5_CONT2 | 10_SOLPRO_W5_CONT2.md plus substrate/cont2 checkers | Q4 complete prefix hull; parity \((3m+2,0,4)\), mcrit=18 |
| SOLPRO_W6 | 21_SOLPRO_W6_THEORY.txt; peers/SOLPRO_W6/checkers/W6_THEORY_CHECKS.py | Four-objective separation and rigorous two-way admissible non-reduction |
| KIMI_W6 | 31_KIMI_W6_PACKAGE.md; 35_KIMI_W6_MDC_EC_LOG.md; peers/KIMI_W6/w6/w6_mdc_checks.py | Rank stratification, ncrit 3/5, leaf-envelope coincidence, MDS obstruction |
| KIMIK3_THINKING_W6 | 41_KIMIK3_THINKING_W6_PACKAGE.md | Shared accounting, candidate-dependent gap, no reduction |
| DEEPSEEK_W6 | 42_DEEPSEEK_W6_PACKAGE.md; swarm_lanes/M1_M10_MDC_RESOLUTION.md; tier3 checkers | Separating example, expand invariants, dual-track/triad warning |
| GROK_W6 | 54_GROK_W6_03_PROOFS.md; 56_GROK_W6_05_MDC_RESOLUTION.md; checker | Exact ledger/expand/phase separation |
| QWEN_W6 | 61_QWEN_W6_PACKAGE.md | Gap formulas and collision-vs-rank mechanism separation |
| FABLE_W5 / KIMI_W5 checker replicas | peers/DEEPSEEK_W6/ec-peer-reruns/fable and /kimi | Exact leaf DPs, big integers, parity/antipodal checks |

Key immutable hashes:

- 01 Core: 8a2df5541d66f97a584b16a9fe01a8846e79d2884d1a667049bd97d59cd4791f
- 17 Cont-1: c4b0b25470c1a73e22ae095ad8aa09841655d0661358512c4d661e95dc775d32
- 18 W4: bdca56260c513780ff9fa60c7e003044a6db3cd81704fa97d9f879d35717f09d
- 10 Cont-2: 1c3547cdea89823e95b3bb2d89c2c65496bc5d4e5930ffb1b384b50853a87f08
- Kimi full checker: ede6683facf12dfb9a6b9fab5aa7af6f7bcf72af18a91fe22e2a104ddb63b9ba
- Kimi 66/66 output: 6761e2871972a95d68f59e8a45c8097fc2a7893e5c6f96f2ee38abe3b040fb09

## 6. Exact W7 checker

Run from the source root with:

~~~sh
PYTHONDONTWRITEBYTECODE=1 python3 -B w7_mdc_check.py
~~~

The following is the exact checker body executed in this review. It computes the theorem arithmetic, residual ranks, binary-MDS boundary, both n <= 3 leaf DPs, the 17/18 witness, all nine integers, and attests the immutable full Q4 66/66 run.

~~~python
from fractions import Fraction as F
from itertools import combinations
from hashlib import sha256
from pathlib import Path

def pc(ws):
    d=sum(ws); return sum(F(w,d)**2 for w in ws)
def heavy(n): return (n+4,)+(4,)*(n-1)
for n in range(2,9):
    mx=pc(heavy(n)); mn=F(1,n); thr=F(9-2*n,3)
    assert mx==F((n+4)**2+16*(n-1),25*n*n)
    assert (n>=5)==(mn>=thr)
pc4=pc(heavy(4)); assert pc4==F(7,25)
MF,LF=9-pc4,F(11,2)-F(3,2)*pc4
assert (MF,LF)==(F(218,25),F(127,25)) and LF>5
assert (MF-8,LF-4)==(F(18,25),F(27,25))

def gf2rank(rows):
    rows=list(rows); rank=0
    while rows:
        p=max(rows); rows.remove(p)
        if not p: continue
        rank+=1; bit=1<<(p.bit_length()-1)
        rows=[x^p if x&bit else x for x in rows]
    return rank

def proj_rank(basis,Q):
    rows=[]
    for v in basis:
        z=0
        for k,i in enumerate(Q): z|=((v>>i)&1)<<k
        rows.append(z)
    return gf2rank(rows)

for n in range(2,9):
    U1=[(1<<n)-1]
    Un=[1<<i for i in range(n)]
    Un1=[(1<<i)|(1<<(n-1)) for i in range(n-1)]
    for q in range(1,n+1):
        for Q in combinations(range(n),q):
            assert proj_rank(U1,Q)==1
            assert proj_rank(Un,Q)==q
            assert proj_rank(Un1,Q)==min(q,n-1)

def mds_exists(r,n):
    for C in combinations(range(1,1<<r),n):
        if all(gf2rank(S)==r for S in combinations(C,r)): return True
    return False
for r in (2,3,4):
    assert mds_exists(r,r+1) and not mds_exists(r,r+2)

def leaf_errors(n,w,law):
    N=1<<n; d=sum(w); out=[0]*(1<<N)
    for A in range(1,1<<N):
        xs=[x for x in range(N) if A>>x&1]
        if law=='adaptive':
            C=0
            for i in range(n):
                best=0
                for a in (0,1):
                    s=0
                    for j in range(n):
                        s+=w[j]*max(sum(((x>>i)&1)==a and ((x>>j)&1)==b for x in xs)
                                      for b in (0,1))
                    best=max(best,s)
                C+=w[i]*best
        else:
            C=max(sum(sum(w[s] for s in range(n)
                              if ((p>>s)&1)==((x>>s)&1))**2 for x in xs)
                  for p in range(N))
        out[A]=d*d*len(xs)-C
    return out

def pair_at(n,w,law,alpha,t):
    E=leaf_errors(n,w,law); N=1<<n; d=sum(w); size=1<<N
    val=[F(0)]*size; split=[0]*size
    for A in range(1,size):
        best=t*E[A]; bbest=0; low=A&-A; rest=A^low; s=rest
        while True:
            B=s|low
            if B!=A:
                z=alpha*d*d*A.bit_count()+val[B]+val[A^B]
                if z<best: best,bbest=z,B
            if s==0: break
            s=(s-1)&rest
        val[A]=best; split[A]=bbest
    L=Et=0; st=[size-1]
    while st:
        A=st.pop(); B=split[A]
        if B: L+=A.bit_count(); st.extend((B,A^B))
        else: Et+=E[A]
    return (L,Et)

cases=[
 ('Q3down',3,(7,4,4),{2:[F(0),F(9),F(40)],3:[F(0),F(13),F(40)]},
  [(0,1188),(8,738),(24,0)]),
 ('Q3unif',3,(1,1,1),{2:[F(0),F(9),F(40)],3:[F(0),F(13),F(40)]},
  [(0,48),(8,30),(24,0)]),
 ('Q2down',2,(3,2),{2:[F(0),F(40)],3:[F(0),F(40)]},
  [(0,62),(8,0)])]
for name,n,w,samples,expect in cases:
    for a,ts in samples.items():
        pa=[pair_at(n,w,'adaptive',a,t) for t in ts]
        pp=[pair_at(n,w,'prototype',a,t) for t in ts]
        assert pa==pp==expect,(name,a,pa,pp)

A=sum(1<<x for x in (0,1,2,5))
Ea=leaf_errors(3,(1,1,1),'adaptive')[A]
Ep=leaf_errors(3,(1,1,1),'prototype')[A]
assert (Ea,Ep)==(17,18)

p=Path('peers/KIMI_W6/w6/w6_mdc_checks.out')
data=p.read_bytes()
assert sha256(data).hexdigest()==\
 '6761e2871972a95d68f59e8a45c8097fc2a7893e5c6f96f2ee38abe3b040fb09'
assert b'SUMMARY: 66 checks, 66 passed, 0 failed' in data
certs=[
 3**5<2**8,
 129**2*9**8<3*128**2*8**8,
 63**3*256>400**3,
 65**2*463**10<=8*64**2*400**10,
 27**7>=2**33,
 53**7>=2**40,
 2075**2*309**12<=32*2048**2*256**12,
 125<=128,
 17**11<=2**45]
assert all(certs)
print('W7-SOL-MDC-CHECK: PASS')
print('critical dimensions: parity U1=3; opaque U_n and U_{n-1}=5')
print('separator n=4 down:',pc4,MF,LF,'gaps',MF-8,LF-4)
print('n<=3 envelope replay: PASS; leaf witness:',Ea,Ep)
print('Q4 full-DP attestation sha256:',sha256(data).hexdigest(),'66/66 PASS')
print('MDS boundary r=2,3,4: max n=3,4,5; 9/9 integer certificates PASS')
~~~

Exact output:

~~~text
W7-SOL-MDC-CHECK: PASS
critical dimensions: parity U1=3; opaque U_n and U_{n-1}=5
separator n=4 down: 7/25 218/25 127/25 gaps 18/25 27/25
n<=3 envelope replay: PASS; leaf witness: 17 18
Q4 full-DP attestation sha256: 6761e2871972a95d68f59e8a45c8097fc2a7893e5c6f96f2ee38abe3b040fb09 66/66 PASS
MDS boundary r=2,3,4: max n=3,4,5; 9/9 integer certificates PASS
~~~

## 7. Validation log

1. Fresh W7 exact checker: PASS, output above.
2. peers/SOLPRO_W6/checkers/W6_THEORY_CHECKS.py: exit 0; “PASS W6-MDC-OPAQUE-RANK-SEPARATION” and “ALL W6 THEORY CHECKS PASS.”
3. Grok MDC checker + DeepSeek tier3 M3/M4/M5/M6/M7/M9/M10 + Fable w5f + Cont-2 Python checker: combined exit 0.
4. Full Kimi DP artifact: exact SHA-256 6761e2...fb09; 66 checks, 66 passed, 0 failed. Its checker source SHA-256 is ede668...b9ba.

## 8. Dependencies and nonclaims

### Dependencies

- W4 exact residual-rank theorem and one-demand floors.
- Fable one-demand floor certificate \(F_{n,\downarrow}(40)\ge11\) for n >= 5.
- Cont-2/Q4 complete prefix theorem for the explicit n=4 separator.
- Exact Kimi two-leaf-law DP only for the stated finite vertices.
- Standard data-processing and entropy lower bounds.

### Nonclaims

- No merge of MDC-FABLE and MDC-KIMI by “MDC,” “parity,” or shared rank-area syntax.
- No all-theta or all-n leaf-envelope identity; n=5 down was not computed.
- No claim that all linear residual matroids are uniform. Nonuniform linear handles remain outside this classification.
- No rank-zero classification inside MDC; \(U_{0,n}\) is real but is a fully determining visible handle.
- No uniqueness of parity among arbitrary nonlinear handles; only the one-dimensional full-support binary linear kernel is unique.
- No arbitrary-category non-reduction. The two non-reductions use the explicit admissible handle-morphism rules.
- No production TokenZero, tokenizer, security, or empirical dominance claim.
- No change to Cont-2 mcrit=18, BP1, or agency-RD claims.

## 9. Freeze recommendation

Freeze **W7-SOL-MDC-MASTER** with child IDs, not a merged MDC theorem:

1. Preserve MDC-FABLE and MDC-KIMI names on every candidate/phase claim.
2. Permit “shared rank-area engine” only at the ledger lemma level.
3. Record the positive-rank qualifier and n >= 3 qualifier on the uniform-MDS stratification.
4. Record envelope coincidence as finite EC, alongside the 17/18 leaf counterexample.
5. Correct the n=2 M-side sentence; the L-side alone proves the kill.