# Wave 7 xhigh affirmative-proof edition: P1 / P2 / P3

**Disposition.** P1 is publishable after two missing uniformity arguments are inserted. P2's numerical conclusions are publishable only in the corrected dual-track form below. P3 is publishable as a finite uniform theorem through n=12 plus the stated weighted finite/root fragments, not as an all-n or full-Q5 theorem. Source bundles were read-only.

**Status tags.** PI = inherited published input; DR = deduction shown here; EC = finite exact computation; BE = bounded experiment; SB = speculative bridge. No BE or SB supports any theorem below.

## 1. Editorial findings and repairs

1. **MAJOR -- P1 Q3 uniform-class implication omitted.** analysis/xhigh/70_p1_general_n.md:100-112 evaluates the exact leaf projection only at the heavy vertex v_3, then uses it for every theta in Theta_3^downarrow. The leaf-occupancy theorem alone does not identify that extremizer. Repair: the explicit formulas for u_r(theta) in P1 Step 2 below are separable convex on theta_i in [4/15,7/15], hence Schur-convex and maximized at v_3. This closes the class quantifier.
2. **MAJOR -- P1 block-Fano class quantifier omitted.** analysis/xhigh/70_p1_general_n.md:116-139 substitutes kappa(v_n) without proving it is the worst theta. Repair: kappa(theta)=sum_i[1-(1-theta_i)^m] is symmetric concave, hence Schur-concave; v_n majorizes every theta in Theta_n^downarrow, so kappa(theta)>=kappa(v_n). The source contains the needed fact at 21_SOLPRO_W6_THEORY.txt:608-615,1557-1568.
3. **MAJOR -- P2 had a hidden PI dependency.** analysis/xhigh/71_p2_mdc.md:157-168 imported F_{n,downarrow}(40)>=11, while peers/KIMI_W6/w6/w6_mdc_checks.py:578-608 only asserts disconnected integers. Repair: P2 Step 4 proves the stronger latency bound directly from coordinate Fano and exact rational/log inequalities.
4. **MEDIUM -- finite-to-infinite and randomized-hull prose was too loose.** The N<=64 DP in analysis/xhigh/70_p1_general_n.md:83-95 is regression EC, not the proof of the all-N spectrum. The analytic complete-level/discrete-convexity proof is supplied below. For random seeds, the proof now cites realization-independent positive gaps, rather than merely saying strictness averages. See also analysis/xhigh/80_critical_review.md:204-208.
5. **MEDIUM -- P2 rank/object and theorem-ID scopes were conflated.** U_{n,n}'s residual law is rank-area-equivalent to Fable after accounting for its independent alias; it is not Fable's opaque alias/private-store object. Uniform binary realizability is stated only for positive residual rank, n>=3; U_{0,n} is excluded explicitly. “Permanent” is a namespace rule; the non-reduction theorem is only for the declared postprocessing/rank-service category. Sources: 18_WAVE4_SOLPRO_PACKAGE_FULL.txt:5482-5564; 31_KIMI_W6_PACKAGE.md:318-325; analysis/xhigh/71_p2_mdc.md:71-129,219-247.
6. **HIGH -- inherited P3 implications cannot be published.** peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md:383-390 prints 4/e_anti kill values as n=5,n=7 BP1 candidates; lines 409-411 wrongly infer that the one-bit line intersects first from concavity. 41_KIMIK3_THINKING_W6_PACKAGE.md:335-337 uses G/(dL)<=1/4 in the wrong direction when s_1<1/4. None is used below. The corrected candidates are 800/79 and 896/79, and the valid minimum-depth condition is s_1>=1/(2r).
7. **MEDIUM -- P3 finite EC scope must remain visible.** Uniform BP1 is proved only for 1<=n<=12. Q5-down is proved only for 37 first-split bipartitions with arbitrary descendants. peers/DEEPSEEK_W6/checkers/tier5/b4_n5_cells.c:104-112 mislabels radius-2 balls as optimal; n5_all16.c hard-codes the 16 true optimizers and ships no K=157 output. The corrected theorem separates the families and does not promote them to all roots.

8. **LOW -- P1 threshold notation included an undefined endpoint.** analysis/xhigh/70_p1_general_n.md:103-109 maximizes a ratio through r=8, but b_8=0 and the positive-part numerator is also zero. The checker silently assigns this case value zero. The publication formula below uses 1<=r<=7 and handles r=8 directly.

No blocker remains in the theorem statements below.

# 2. P1 -- exact sequential full-prefix staircase

## Theorem P1: W7-SOL-SEQ-DOWN-STAIRCASE [PI|DR|EC] [M]

Let n>=2 and m in N_{>=1}. Put
\[
\Theta_n^\downarrow=\{\theta\in\Delta_{n-1}:\theta_i\ge 4/(5n)\},
\qquad v_n=(n+4,4,\ldots,4)/(5n).
\]
Let X be uniform on \(\{0,1\}^n\), let \(S_1,\ldots,S_m\) be iid from theta and independent of X, and compare against every randomized variable-length binary no-recovery prefix policy under the locked sequential ledger
\[
M_T=(m+1)(1+\ell_T)+40e_T,\qquad
L_T=1+\ell_T+c_T+20e_T,\qquad D_T=e_T,
\]
where \(c_T\ge0\), \(e_T\) is joint failure, and dominance is coordinatewise weak with at least one strict coordinate, pointwise for every theta. The parity/complement policy has
\[
(M_{par},D_{par},L_{par})=(3m+2,0,4).
\]
Then parity dominates the complete randomized hull exactly for
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
\boxed{m_{crit}(2)=0,  m_{crit}(3)=16,  m_{crit}(4)=m_{crit}(5)=18,  m_{crit}(n)=19\ (n\ge6).}
\]
Child IDs and tags are preserved: W7-SOL-OCCUPANCY-PROJECTION [PI|DR|EC] [F], W7-SOL-PREFIX-SPECTRUM-N [DR|EC] [S], W7-SOL-BLOCK-FANO-BARRIER [DR|EC] [F], and W7-SOL-Q3-FULLPREFIX [PI|DR|EC] [F].

### Proof

**Step 1 -- deterministic leaf projection.** Condition on the entire policy seed. Its pre-demand transcript partitions the N=2^n equiprobable words into r nonempty leaves A_j, at depths d_j, with \(\ell=N^{-1}\sum_j|A_j|d_j\). If \(K_m=|\{S_1,\ldots,S_m\}|=k\), one fixed answer pattern is compatible with at most \(2^{n-k}\) words in each leaf. Therefore
\[
P_T\le \mathbb E_\theta\min\{1,r2^{-K_m}\}. \tag{P1.1}
\]
For r=1 a prototype attains
\[
P_{0,m}(\theta)=\mathbb E_\theta2^{-K_m}
=2^{-n}\sum_{B\subseteq[n]}\theta(B)^m. \tag{P1.2}
\]
Each summand is convex for integer m>=1, so this symmetric function is Schur-convex. The lower-capped simplex has heavy vertices given by permutations of v_n, whence its maximum is
\[
p_{n,m}=\frac1{2^n(5n)^m}\sum_{k=0}^{n-1}{n-1\choose k}
\big[(n+4+4k)^m+(4k)^m\big]. \tag{P1.3}
\]
This rederives, rather than merely imports, W6-LEAF-OCC (31_KIMI_W6_PACKAGE.md:175-194).

**Step 2 -- the missing Q3 projection extremizer.** For n=3 write
\(a=\sum_i\theta_i^m\), \(b=\sum_i(1-\theta_i)^m\), and
\(u_r(\theta)=\mathbb E_\theta\min(1,r2^{-K_m})\). Inclusion-exclusion gives
\[
\begin{aligned}
u_1&=(1+a+b)/8, &u_2&=(1+a+b)/4,\\
u_3&=3/8+3b/8-a/8,\\
u_r&=r/8+(1-r/8)(b-a),\quad 4\le r\le7, &u_8&=1.
\end{aligned} \tag{P1.4}
\]
On Theta_3^downarrow, every coordinate lies in [4/15,7/15], strictly below 1/2. The one-variable second derivatives behind (P1.4) are positive multiples of
\(x^{m-2}+(1-x)^{m-2}\),
\(3(1-x)^{m-2}-x^{m-2}\), or
\((1-x)^{m-2}-x^{m-2}\). Thus every u_r is symmetric convex for m>=2 and is maximized at v_3. This supplies the uniform-theta implication absent from the draft.

**Step 3 -- all-N prefix spectrum.** Let C_N(r) be the minimum of \(\sum_j|A_j|d_j\) over r-leaf prefix partitions of N words. Put
\[
U(s)=s(a+2)-2^{a+1},\qquad a=\lfloor\log_2s\rfloor,  U(1)=0.
\]
For \(1\le d\le\lfloor\log_2r\rfloor\), let
\[
k=2^d,  q=\lfloor(r-1)/(k-1)\rfloor,  b=(r-1)-(k-1)q.
\]
Then
\[
\boxed{C_N(1)=0,  C_N(r)=\min_d\{Nd+(k-1-b)U(q)+bU(q+1)\}.} \tag{P1.5}
\]
Proof: for a fixed tree put one word in every leaf and all N-r extra words in a shallowest leaf of depth d. The tree is complete through level d. One level-d node is that leaf and the other k-1 nodes root full subtrees with positive leaf counts summing to r-1. The minimum unit-mass external path sum of an s-leaf full binary tree is U(s), attained by the two adjacent depths. Since the forward differences of U are nondecreasing, the k-1 counts must differ by at most one, giving q and q+1. Conversely those complete subtrees attain the displayed value. This is an analytic proof for every N; DP agreement through N=64 is EC regression only. In particular
\[
C_{16}=(0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64).
\]
Source lineage: 21_SOLPRO_W6_THEORY.txt:833-898; analysis/xhigh/00_substrate_methods.md:174-190.

**Step 4 -- no-message signs, including the finite strip.** Define
\[
G_0(n,m)=40(1-p_{n,m})-(2m+1).
\]
For m>=10,
\[
p_{n,m}-p_{n,m+1}=\mathbb E[Z^m(1-Z)]
\le \frac{m^m}{(m+1)^{m+1}}<\frac1{20},
\]
so G_0(n,m)>G_0(n,m+1). The unmentioned m<10 strip and every value through each cutoff are evaluated exactly, not inferred from this monotonicity. Endpoint certificates are

| (n,m) | exact G_0(n,m) |
|---|---:|
| (3,16) | 845049722020265693/437893890380859375 |
| (3,17) | -22519522704133297/437893890380859375 |
| (4,18) | 277615146191/762939453125 |
| (4,19) | -1227337666073/762939453125 |
| (5,18) | 887975035189461090631639/582076609134674072265625 |
| (5,19) | -254541365995396231447867/582076609134674072265625 |
| (6,19) | 2975301311635846283/19705225067138671875 |
| (6,20) | -2684852348710641308821/1477891880035400390625 |

Padding gives \((v_n,0)\succ v_{n+1}\), because every first-k partial sum differs by \(4k/[5n]-4k/[5(n+1)]>0\). Hence p_{n,m}>=p_{n+1,m} and G_0(n,m)<=G_0(n+1,m). The n=6 signs therefore close every n>=6. Exact finite-strip assertions are in 23_SOLPRO_W6_CHECKS.py:140-145 and the embedded P1 checker.

**Step 5 -- every nontrivial Q3 tree.** Let \(c_r=C_8(r)/8\), \(u_r=u_r(v_3)\), and \(b_r=1-u_r\). By Steps 1--3 and the Q3 extremizer repair,
\[
M_T-(3m+2)\ge(m+1)c_r-(2m+1)+40b_r. \tag{P1.6}
\]
For 3<=m<=16 exact rational evaluation over all 1<=r<=8 gives
\[
\rho_{PL}(3,m)=\max_{1\le r\le7}
\frac{[2m+1-(m+1)c_r]_+}{b_r}<40,
\]
with the terminal value
\[
\rho_{PL}(3,16)=
\frac{144504983825683593750}{3823887026147156267}
\approx37.790076651737. \tag{P1.7}
\]
The omitted r=8 endpoint has b_8=0 and c_8=3, so its direct memory gap is m+2>0; no 0/0 convention is needed. For m=1,2 the exact Q3 scalar DP has supported pairs
\((0,60),(8,30),(15,16),(24,0)\), seams \(8,15,135/8\), and gives the two required floors 8. Consequently \(M_T\ge8>5\) at m=1 and \(M_T\ge12>8\) at m=2.

**Step 6 -- every nontrivial tree for n>=4.** For arbitrary theta,
\[
\kappa_m(\theta)=\mathbb E_\theta K_m
=\sum_i[1-(1-\theta_i)^m].
\]
This is symmetric concave, hence \(\kappa_m(\theta)\ge\kappa_m(v_n)=:\kappa_{n,m}\), where
\[
\kappa_{n,m}=1-[4(n-1)/(5n)]^m
 +(n-1)[1-((5n-4)/(5n))^m]. \tag{P1.8}
\]
Conditional Fano applied to \(X_{Q_m}\), followed by the prefix entropy bound, gives uniformly
\[
\ell_T\ge\kappa_{n,m}-H_2(e_T)-k e_T,
\qquad k=\min(n,m). \tag{P1.9}
\]
A nontrivial prefix has ell>=1. With a=m+1,
\[
\Gamma:=M_T-(3m+2)=a(\ell_T-1)+40e_T-m. \tag{P1.10}
\]
For m=2,3,4, minimize \((40-ak)e-aH_2(e)\) by entropy conjugacy and use
\(\log_2(1+x)<3x/2\). At n=4 the exact positive lower bounds are
\[
16159/102400,  15561/8000,  14957/4000. \tag{P1.11}
\]
For 5<=m<=19, e>m/40 makes (P1.10) positive. If e<=m/40, then H_2(e)<1 and
\[
\Gamma>
\begin{cases}
a(\kappa_{n,m}-2)-m,&40-ak\ge0,\\
a(\kappa_{n,m}-2-km/40),&40-ak<0.
\end{cases} \tag{P1.12}
\]
For n>=m, majorization \((v_n,0)\succ v_{n+1}\) and Schur-concavity make kappa increase with n; therefore only the finite strip 4<=n<=m<=19 is required. Exact rational enumeration has global minimum
\[
\frac{331725854346589385191559240189443183}
{794428636916437084448554992675781250}>0
\]
at n=m=19. Thus every nontrivial tree has a strict memory gap for n>=4, 2<=m<=19.

**Step 7 -- latency, m=1, and strictness.** Joint failure contains first-answer failure, so
\[
2L_T\ge2+2\ell_T+40e_1. \tag{P1.13}
\]
At n=3 the exact floor is 8, hence L_T>=4. For n>=4, coordinate Fano yields
\[
2+2\ell_T+40e_1\ge
\Phi_n:=2+2n[1-\log_2(1+2^{-16/n})]. \tag{P1.14}
\]
The right side is nondecreasing and
\(\Phi_4=10-8\log_2(17/16)>8\), since \(17^4<2\,16^4\). Thus L_T>4; the same floor gives M_T>8>5 at m=1. Always D_T=e_T>=0.

The exact no-message gaps, the finite Q3 threshold, and (P1.11)--(P1.12) provide a positive bound for each fixed (n,m) in the claimed phase. Conditioning on a random seed and averaging the affine ledger therefore preserves the strict M gap and the weak L,D inequalities over the randomized hull.

**Step 8 -- sharp failures.** For n=2 the zero-error identity has \((M,D,L)=(3m+3,0,3)\), so parity never dominates. A legal no-message prototype succeeds with probability at least 2^{-n}, whence
\[
M_0-M_{par}\le39-2m-40/2^n. \tag{P1.15}
\]
At n=3,m=17 this upper bound ties, but the exact heavy-vertex gap in the table is negative. It is negative from m=18 at n=3, from m=19 at n=4,5, and from m=20 at n>=6. Monotonicity then excludes every later m. This proves both directions. QED.

# 3. P2 -- MDC dual-track master theorem

All critical-dimension claims in this section concern **two iid sequential demands** at (rho,lambda)=(40,20), \((h,q,c_0,c_1)=(1,0,1/2,1/2)\), the lower-capped class Theta_n^downarrow, joint failure distortion, and the complete randomized variable-length no-recovery prefix hull. They are not batch claims and not arbitrary-m claims.

## Theorem P2A: W7-SOL-MDC-RANK-AREA [DR]

For a binary linear visible handle Z=AX with K=ker A and demanded coordinate set Q,
\[
H(X_Q\mid Z,Q)=r_K(Q):=\dim\pi_Q(K).
\]
The exact minimum binary prefix-free residual payload is r_K(Q). With \(Q_k=\{S_1,\ldots,S_k\}\),
\[
A_K^{(m)}=\mathbb E\sum_{k=1}^m r_K(Q_k),
\qquad B_K^{(m)}=\mathbb E r_K(Q_m),
\]
\[
M_K^{seq}=(m+1)(1+h)+(1+q)A_K^{(m)},
\qquad
L_K^{seq}=1+h+c_0+(1+q+c_1)B_K^{(m)},
\qquad D_K=0. \tag{P2.1}
\]
For U_{1,n}, this is \((3m+2,0,4)\). At m=2, U_{n,n} gives
\[
(9-p_c,0,11/2-3p_c/2),
\qquad p_c=\sum_i\theta_i^2,
\]
and U_{n-1,n} has the same two-demand ledger for n>=3. Fable's opaque alias/private-store candidate is rank-area-equivalent to the U_{n,n} residual law after its independent alias cost is included; it is not literally the deterministic A=0 handle.

**Proof.** Given Z=z, X is uniform on an affine coset x_0+K; its Q-projection is uniform on a coset of \(\pi_Q(K)\), containing \(2^{r_K(Q)}\) equiprobable values. Kraft/entropy gives expected exact residual length at least r_K(Q), and a fixed-length basis-coordinate encoding attains it. Summing the carried-token payloads gives (P2.1). For U_1 every nonempty Q has rank one. For U_n, ranks are |Q|, and at m=2 the successive expectations are 1 and 2-p_c. For U_{n-1}, rank equals |Q| whenever |Q|<=2<n. QED. Source: 18_WAVE4_SOLPRO_PACKAGE_FULL.txt:5482-5564; analysis/xhigh/71_p2_mdc.md:71-106.

## Theorem P2B: W7-SOL-MDC-UNIFORM-STRAT [DR|EC]

For positive residual rank 1<=r<=n and n>=3, a binary linear handle realizes the uniform residual matroid U_{r,n} iff
\[
\boxed{r\in\{1,n-1,n\}.}
\]
The rank-zero U_{0,n}, realized by K={0}, exists but is outside this classification.

**Proof.** A generator matrix G of K realizes U_{r,n} iff every r columns are independent. For 2<=r<=n-2, put G=[I_r|C]. Every extra column must have every coordinate nonzero; otherwise it and the r-1 systematic columns avoiding that coordinate are dependent. Over F_2 the only such column is the all-ones vector, and it can occur at most once. Hence n<=r+1, contradicting r<=n-2. The three survivors are realized by \(\langle\mathbf1^n\rangle\), \([I_{n-1}|\mathbf1]\), and \(\mathbb F_2^n\). Exact boundary enumeration for r=2,3,4 returns maximal n=3,4,5 as regression EC. QED. Source correction: 31_KIMI_W6_PACKAGE.md:318-325.

## Theorem P2C: W7-SOL-MDC-CRIT [DR|EC]

Let n_crit be the least dimension from which the named two-demand sequential ledger dominates the complete randomized prefix hull uniformly over Theta_n^downarrow. Then
\[
\boxed{n_{crit}(U_1)=3,
\qquad n_{crit}(U_{n-1})=n_{crit}(U_n)=5.}
\]
Here U_1 is the MDC-KIMI parity/complement stratum. U_n and, at two demands, U_{n-1}, are the MDC-FABLE ledger stratum; the candidate IDs remain distinct.

### Proof

**Step 1 -- a dependency-free first-answer bound.** If e_i is the error in estimating bit X_i from the pre-demand transcript, prefix entropy and coordinate Fano give
\[
\ell_T\ge\sum_i[1-H_2(e_i)].
\]
Since theta_i>=4/(5n), \(40e_1\ge(32/n)\sum_i e_i\). Separable entropy conjugacy therefore gives
\[
2+2\ell_T+40e_1\ge
\Phi_n:=2+2n[1-\log_2(1+2^{-16/n})]. \tag{P2.2}
\]
For a two-demand baseline, joint error e_2>=e_1, so
\[
2L_T\ge\Phi_n,
\qquad M_T\ge\Phi_n+(1+\ell_T). \tag{P2.3}
\]
The elementary derivative of \(n[1-\log_2(1+2^{-16/n})]\) is positive, hence Phi_n is nondecreasing.

**Step 2 -- Kimi.** At n=3, the exact first-answer floor is 8, so L_T>=4 and M_T>=9>8. For n>=4, \(\Phi_4=10-8\log_2(17/16)>8\), so both M and L are strict against (8,0,4). At n=2, identity has (9,0,3), killing latency dominance. These statements are uniform in theta.

**Step 3 -- Fable for n>=5.** Cauchy gives p_c>=1/n, hence \(2L_F=11-3p_c\le11-3/n\). At n=5 put x=2^{-16/5}. The exact inequalities
\[
7^5>2^{14},
\qquad
\ln2>2\left(\frac13+\frac1{3^3\,3}+\frac1{3^5\,5}\right)
=\frac{842}{1215}>\frac9{13}
\]
give x<7/64 and
\[
\log_2(1+x)<\frac{13}{9}\frac7{64}=\frac{91}{576}<\frac4{25}.
\]
Thus
\[
\Phi_5>3001/288>52/5\ge2L_F.
\]
At n=6, \(2^8>6^3\) gives \(2^{-8/3}<1/6\), and \(\ln2>2/3\) gives \(\log_2(1+2^{-8/3})<1/4\); hence \(\Phi_6>11\). Monotonicity closes every n>=6. Equations (P2.3) now make both latency and memory strict against Fable for all n>=5, with no Fable-floor PI.

**Step 4 -- sharp lower dimensions.** Identity has L=1+n. Fable could dominate it only if
\[
p_c\ge(9-2n)/3.
\]
The maximum of p_c over Theta_n^downarrow occurs at v_n and equals
\[
\frac{(n+4)^2+16(n-1)}{25n^2}.
\]
For n=2,3,4 these maxima are 13/25, 9/25, 7/25, all below the corresponding threshold. Thus Fable fails through n=4. U_{n-1,n} has the same two-demand ledger for n>=3. Finally, (P2.2)--(P2.3) hold for every deterministic seed with explicit positive margins, so seed conditioning extends the result to the randomized hull. QED.

## Theorem P2D: W7-SOL-MDC-SEP [DR|EC], W7-SOL-MDC-NONRED-FK [DR], and -KF [DR]

For full-support theta and n>=2, in objectives \((M,D,L,I_{pre})\),
\[
F=(9-p_c,0,(11-3p_c)/2,0),
\qquad K=(8,0,4,n-1).
\]
Since p_c<1, Kimi is strictly cheaper in M,L while Fable has strictly less pre-demand leakage. They are Pareto-incomparable in four objectives.

An admissible handle morphism may use public randomness independent of X to postprocess the visible handle, may not introduce new source-dependent pre-demand information, and must preserve the registered exact residual rank/service. Under this category there is no morphism either way.

**Proof.** Fable-to-Kimi is impossible by data processing: I(X;H_F)=0 implies I(X;Phi(H_F,U))=0, whereas Kimi has I(X;H_K)=n-1. Conversely, if a postprocessed Kimi handle H' is opaque, then for distinct i,j, \(H(X_i,X_j|H')=2\). One residual bit R cannot recover both exactly, because
\[
2=I(X_i,X_j;R\mid H')\le H(R\mid H')\le1.
\]
This is category-relative non-reduction only. Distinct IDs are a publication rule, not a universal impossibility theorem. Source: 21_SOLPRO_W6_THEORY.txt:1950-2092; analysis/xhigh/30_kimik3_w6.md:21-28.

## Theorem P2E: W7-SOL-MDC-LEAFCOIN [EC]

Let E_A be the adaptive/Fable leaf law and E_B the prototype/Kimi leaf law:
\[
E_A(A)=d^2|A|-\sum_iw_i\max_a\sum_jw_j\max_bN_{ij}^{ab}(A),
\]
\[
E_B(A)=d^2|A|-\max_p\sum_{x\in A}
\left(\sum_sw_s\mathbf1[p_s=x_s]\right)^2.
\]
Under the exact prefix recurrence
\[
\mathrm{Frontier}(A)=\mathrm{hull}\left(\{(0,E(A))\}\cup
\{(|A|+L_1+L_2,E_1+E_2):B\sqcup C=A\}\right),
\]
the root supported pairs have been computed and found equal under both laws at the following six vertices:

| vertex | root pairs (L,E) |
|---|---|
| Q4 uniform | (0,176),(16,128),(33,80),(42,56),(64,0) |
| Q4 down | (0,272),(16,182),(32,108),(64,0) |
| Q4 cap | (0,1096),(16,776),(32,492),(64,0) |
| Q3 down | (0,1188),(8,738),(24,0) |
| Q3 uniform | (0,48),(8,30),(24,0) |
| Q2 down | (0,62),(8,0) |

The quantifier is only these six rational vertices and \(\alpha\in\{2,3\}\) in
\[
F^{(2)}_\alpha(t)=\alpha+2^{-n}\min_{(L,E)}(\alpha L+tE/d^2).
\]
It is not an all-theta or all-n identity. Indeed for
\(A=\{000,001,010,101\}\), uniform n=3,
\[
E_A(A)=17\ne18=E_B(A).
\]
Thus root projection equality does not imply leaf-law, policy, handle, or phase equivalence. Exact source/checker: peers/KIMI_W6/w6/w6_mdc_checks.py and its 66/66 artifact; scope warning at analysis/xhigh/20_kimi_w6.md:116-127.

# 4. P3 -- BP1 publication theorem suite

## Locked definitions and tangent equivalence [PI rederived|DR]

Let \(\Omega=\{0,1\}^n\), N=2^n, X uniform, and integer demand weights \(w_i\ge0\), with d=sum_i w_i>0. For A subset Omega define
\[
E_w(A)=\sum_iw_i\min(N_i^0(A),N_i^1(A)).
\]
For a recursive binary prefix partition T,
\[
E(T)=\sum_{A\in leaves(T)}E_w(A),
\quad e(T)=E(T)/(dN),
\quad L(T)=\sum_A|A|\,depth(A)=\sum_{internal\ A}|A|,
\quad \ell=L/N,
\]
\[
J_t(T)=2+2\ell(T)+t e(T),
\qquad F(t)=\min_TJ_t(T).
\]
The root has e=1/2 and line \(J_0(t)=2+t/2\). Let e_1 be the least one-bit-tree error, \(s_1=1/2-e_1\), and \(\tau_1=2/s_1\). Then
\[
\boxed{BP1\iff e(T)\ge1/2-s_1\ell(T)\ \forall T
\iff E_w(\Omega)-E(T)\le s_1dL(T)\ \forall T
\iff \text{the root is DP-optimal at }t=\tau_1.} \tag{P3.1}
\]
The one-bit optimizer has ell=1 and ties the root at tau_1, so the equivalence is exact. Source lineage: 41_KIMIK3_THINKING_W6_PACKAGE.md:333-337.

## Theorem P3A: W7-SOL-BP1-BASE4 [DR|EC]

BP1 holds for every prefix tree in the five frozen weighted classes:

| class | primitive w; d | E_0 | E_1 | e_1 | s_1 | tau_1 |
|---|---:|---:|---:|---:|---:|---:|
| Q3-down | (7,4,4);15 | 60 | 30 | 1/4 | 1/4 | 8 |
| Q3-uniform | (1,1,1);3 | 12 | 6 | 1/4 | 1/4 | 8 |
| Q4-down | (2,1,1,1);5 | 40 | 22 | 11/40 | 9/40 | 80/9 |
| Q4-cap | (3,3,2,2);10 | 80 | 48 | 3/10 | 1/5 | 10 |
| Q4-uniform | (1,1,1,1);4 | 32 | 20 | 5/16 | 3/16 | 32/3 |

**Proof certificate.** For c=s_1d define exactly over every nonempty source subset
\[
U_c(A)=\min\left(E_w(A),  c|A|+
\min_{\varnothing\ne B\subsetneq A}[U_c(B)+U_c(A\setminus B)]\right). \tag{P3.2}
\]
Induction on |A| proves that U_c(A) is the minimum of \(E(T_A)+cL(T_A)\) over all recursive prefix subtrees on A. Thus U_c(Omega)=E_w(Omega) is equivalent to (P3.1). Exhaustive exact-integer evaluation returns equality in all five rows; lowering the normalized slope by 1/(dN) returns a strict smaller value, proving tightness. The rerun values are 480=8E_0 for Q3 and 2560=16E_0 for Q4. Checker recurrence: peers/DEEPSEEK_W6/checkers/tier5/b4_frontier.c:1-119. QED.

## Theorem P3B: W7-SOL-BP1-UNIFORM-SIZE-DP [DR]

At uniform demand put
\[
\Phi(A)=\sum_i\left|\sum_{x\in A}(-1)^{x_i}\right|,
\qquad E(A)=(n|A|-\Phi(A))/2.
\]
Let beta_n(k)=max_{|A|=k}Phi(A). If q_n is the multiset containing \(n-2j\) with multiplicity \({n\choose j}\), then beta_n(k) is the sum of the k largest members of q_n. Put \(M_n=\max_k\beta_n(k)\). If
\[
D_n(a,b):=2M_n(a+b)+N\beta_n(a+b)-N\beta_n(a)-N\beta_n(b)\ge0 \tag{P3.3}
\]
for every positive a,b with a+b<=N, then uniform BP1 holds at dimension n, with
\[
s_1=M_n/(nN),  e_1=1/2-s_1,  \tau_1=2/s_1.
\]

**Proof.** Since
\[
\Phi(A)=\max_{\sigma\in\{\pm1\}^n}\sum_{x\in A}
\langle\sigma,(-1)^x\rangle,
\]
choosing the k largest scores proves the beta formula; all sigma have the same binomial score multiset. For a subtree on A define
\[
H(T_A)=\sum_{C\in leaves(T_A)}\Phi(C)-2(M_n/N)L(T_A).
\]
At a leaf H<=beta_n(|A|). If A splits into sizes a,b, induction and (P3.3) give
\(H(T_A)\le\beta_n(a+b)\). At Omega, beta_n(N)=0, so
\[
\sum_C\Phi(C)\le2(M_n/N)L(T).
\]
Substitution into \(e(T)=1/2-\sum_C\Phi(C)/(2nN)\) gives (P3.1). This condition is stronger than BP1 because it permits the two children to attain beta independently. QED. The beta identity refines 31_KIMI_W6_PACKAGE.md:327-350.

## Theorem P3C: W7-SOL-BP1-UNIFORM-N12 [DR|EC]

Condition (P3.3), hence arbitrary-prefix uniform BP1, holds exactly as certified for every integer 1<=n<=12:

| n | N | M_n | s_1 | e_1 | tau_1 | zero slacks | min positive D_n |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 2 | 1 | 1/2 | 0 | 4 | 1 | none |
| 2 | 4 | 2 | 1/4 | 1/4 | 8 | 4 | 4 |
| 3 | 8 | 6 | 1/4 | 1/4 | 8 | 1 | 8 |
| 4 | 16 | 12 | 3/16 | 5/16 | 32/3 | 7 | 16 |
| 5 | 32 | 30 | 3/16 | 5/16 | 32/3 | 1 | 56 |
| 6 | 64 | 60 | 5/32 | 11/32 | 64/5 | 21 | 112 |
| 7 | 128 | 140 | 5/32 | 11/32 | 64/5 | 1 | 256 |
| 8 | 256 | 280 | 35/256 | 93/256 | 512/35 | 71 | 608 |
| 9 | 512 | 630 | 35/256 | 93/256 | 512/35 | 1 | 1024 |
| 10 | 1024 | 1260 | 63/512 | 193/512 | 1024/63 | 253 | 2992 |
| 11 | 2048 | 2772 | 63/512 | 193/512 | 1024/63 | 1 | 4096 |
| 12 | 4096 | 5544 | 231/2048 | 793/2048 | 4096/231 | 925 | 13984 |

The EC proof enumerates every ordered positive size pair (a,b) with a+b<=2^n using exact integers and asserts D_n(a,b)>=0. This is exhaustive for the sufficient theorem (P3.3), not sampled-tree BE. At n=5 the complete beta table is
\[
(0,5,8,11,14,17,20,21,22,23,24,25,26,27,28,29,30,
29,28,27,26,25,24,23,22,21,20,17,14,11,8,5,0).
\]
QED.

## Theorem P3D: W7-SOL-BP1-Q5DOWN-ROOT37 [DR|EC]

At Q5-down, w=(9,4,4,4,4), d=25, N=32. Exact enumeration gives
\[
E_0=400,  E_1=242,  e_1=121/400,  s_1=79/400,  \tau_1=800/79,
\]
with exactly 16 canonical optimal one-bit bipartitions. Every tree whose first split is in the following union satisfies the BP1 tangent, with arbitrary recursive subtrees below both children:

| first-split family | canonical roots | side size | E per side | root gain gamma | slack s_1dN-gamma |
|---|---:|---:|---:|---:|---:|
| exact one-bit optimizers | 16 | 16 | 121 | 158 | 0 |
| radius-2 balls/coballs | 16 | 16 | 125 | 150 | 8 |
| heavy-coordinate halfcube | 1 | 16 | 128 | 144 | 14 |
| light-coordinate halfcube | 4 | 16 | 168 | 64 | 94 |

**Proof.** Use the exact cell DP (P3.2) with integer scaling S=32, K=158. Equality \(U(C)=32E(C)\) means the maximum child excess \(R(C)=\max_T[E(C)-E(T)-(79/16)L(T)]\) is zero. The exact DPs give R=0 on all 32 sides of the 16 optimal roots, all ball/coball sides up to XOR translation, both heavy halfcubes, and all eight light halfcubes up to light-coordinate permutation. The packaged b4_n5_cells.c directly checks a light-coordinate halfcube; an exact temporary review variant changing only that coordinate predicate checks the heavy halfcube and returns U=4096=32E. For a root B,C,
\[
E_0-E(T)=\gamma+[E(B)-E(T_B)]+[E(C)-E(T_C)],
\]
while the root cost is \((79/16)N=158\). Thus R(B)+R(C)<=158-gamma proves the tangent. The table has nonnegative slack in every row. XOR translation preserves E and the DP, and the 16 center/complement pairs are distinct; together with five coordinate cuts and 16 optimal cuts this gives 37 distinct bipartitions. The 16 optimal masks are
00017fff, 0002bfff, 0004dfff, 0008efff, 0010f7ff, 0020fbff, 0040fdff, 0080feff, 0100ff7f, 0200ffbf, 0400ffdf, 0800ffef, 1000fff7, 2000fffb, 4000fffd, 7fff0001.
Checker sources: peers/DEEPSEEK_W6/checkers/tier5/n5_all16.c and b4_n5_cells.c:1-148; the heavy-coordinate rerun is documented in this review validation. QED.

## Theorem P3E: all-n lower face and proof-route obstructions [DR]

1. **W7-SOL-BP1-ALLN-BASE4.** For arbitrary nonnegative weights and every n,
\[
\sum_{leaves}\Phi_w(A)\le dL(T),
\qquad e(T)\ge1/2-\ell(T)/2.
\]
Hence \(F(t)=2+t/2\) for 0<=t<=4. Proof: at each split, coordinatewise triangle inequality bounds the increase in weighted absolute bias by d|A|; sum over internal cells. This is only a universal lower bound on the first breakpoint.
2. **W7-SOL-BP1-ANTI-LOCAL-OBSTRUCTION.** For every full-support class and n>=2, each antipodal pair has split-gain density 1/2. Since e_1>0 gives s_1<1/2, any proof requiring every local split gain to be <=s_1 fails on all 2^{n-1} antipodal cells. This does not disprove BP1.
3. **W7-SOL-BP1-LEAF-INFO-OBSTRUCTION.** At the heavy vertex v_n and A=Omega minus one point, posterior advantage divided by self-information is
\[
R_n=\frac1{2(N-1)\log_2(N/(N-1))}>\ln2/2>1/3.
\]
Fourier Cauchy--Parseval gives
\[
s_1\le\|v_n\|_2/2,
\qquad \|v_n\|_2^2=(n+24)/(25n),
\]
so s_1<=3/10 for n>=3. Therefore pointwise leaf self-information charging at slope s_1 fails for every n>=3. It does not exclude sibling-coupled or global potentials.
4. If every leaf has depth at least r, then \(G/(dL)\le1/(2r)\); such trees are certified only when \(s_1\ge1/(2r)\). At Q5-down, r=3 works and r=2 does not follow.

# 5. Concise non-solutions log

- No arbitrary-n prefix spectrum is inferred from finite DP; (P1.5) is the analytic proof.
- No Kimi adaptive-DTV theorem or exact general-rho surface is imported. Those overclaims are rejected at analysis/xhigh/20_kimi_w6.md:3-10.
- No all-theta/all-n adaptive-prototype leaf-law identity, Fable/Kimi policy identity, or unrestricted-category non-reduction is claimed.
- No full Q5-down BP1 theorem and no uniform n>=13 theorem is claimed; ROOT37 and N12 are exact finite statements.
- No counterexample to BP1, production TokenZero claim, tokenizer/security claim, or empirical real-agent dominance claim is made.

# 6. Exact citation ledger

- Locked Core scopes/gauges: 01_RADC_FORMAL_CORE_V1_FREEZE.md:73-96; analysis/xhigh/00_substrate_methods.md:12-139. The Core itself is mixed-status, as recorded at analysis/xhigh/00_substrate_methods.md:7-10.
- Q4 finite substrate and its scope: 10_SOLPRO_W5_CONT2.md; analysis/xhigh/00_substrate_methods.md:7-10,174-190. It is not an arbitrary-n proof.
- P1 analytic/finite lineage: 17_SOLPRO_W5_CONT1.md:164-173; 21_SOLPRO_W6_THEORY.txt:608-615,833-898,1516-1888; 23_SOLPRO_W6_CHECKS.py:59-145,221-275; 31_KIMI_W6_PACKAGE.md:175-263.
- P2 rank and separation: 18_WAVE4_SOLPRO_PACKAGE_FULL.txt:5482-5564; 21_SOLPRO_W6_THEORY.txt:1950-2092; 31_KIMI_W6_PACKAGE.md:282-325; peers/KIMI_W6/w6/w6_mdc_checks.py:578-608.
- P3 tangent/beta/DP: 31_KIMI_W6_PACKAGE.md:327-362; 41_KIMIK3_THINKING_W6_PACKAGE.md:333-359; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md:365-420; peers/DEEPSEEK_W6/checkers/tier5/b4_frontier.c:1-119; b4_n5_cells.c:1-148; n5_all16.c.
- Foundational audit cautions: analysis/xhigh/10_solpro_w6.md:7-19 (missing W6 C++ source and narrower EC); analysis/xhigh/20_kimi_w6.md:3-10 (false adaptive-DTV and sufficient-only rho); analysis/xhigh/30_kimik3_w6.md:19-34 (undefined universal reduction, finite-to-all-n and PI promotions).

# 7. Validation

- Embedded P1 exact checker: PASS; 0/16/18/18/19 staircase, terminal Q3 rho, endpoint fractions, all-N formula regression through N=64, and positive block-Fano minimum matched.
- Embedded P2 checker: PASS; critical dimensions 3/5, MDS boundaries, n<=3 dual leaf DP, exact 17/18 witness, and 66/66 artifact digest matched. The full Q4 source checker exceeded this review runtime's 24-second call limit; artifact identity is not represented as an independent recomputation.
- Uniform size checker: PASS for every pair (a,b), n=1..12; the table above matched exactly.
- Recompiled BP1 C checkers: b4_frontier returned HOLDS/TIGHT for all five base classes and E1=242 with 16 Q5 optimizers; n5_all16 returned R=0 on 32/32 optimal sides; b4_n5_cells returned ball/coball and light-halfcube R=0; the heavy-coordinate review variant returned U=4096=32E and R=0.

## Residual risks

1. P3's N12 and ROOT37 theorems are exact finite-program certificates, not proof-assistant objects. Full Q5-down and uniform n>=13 remain open.
2. The packaged n5_all16 source hard-codes masks and does not ship the K=157 variant; this does not affect the K=158 ROOT37 theorem.
3. The independent W6 C++ source advertised by 28_SOLPRO_W6_SHA256.txt:2 is absent. Do not cite its output as reproducible evidence.
4. P2 Q4 leaf coincidence was not independently recomputed in this runtime; the supplied source, prior 66/66 output, hash, and smaller independent DP checks agree.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Sections 1 and 6 give concrete severity-ranked findings with exact analysis/source paths; sections 2-4 give corrected theorem statements and complete proof sequences; sections 5 and Residual risks record non-solutions and remaining scope."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/06964ddf-9ef8-4cc1-88c9-c08685140e50/analysis-xhigh/83_proof_editor.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "sed -n '173,371p' analysis/xhigh/70_p1_general_n.md > /tmp/w7_proofedit_p1.py && python3 -B /tmp/w7_proofedit_p1.py",
      "result": "passed",
      "summary": "Exact P1 staircase, endpoints, spectrum regression, Q3 projection threshold, and block-Fano minimum passed."
    },
    {
      "command": "sed -n '330,462p' analysis/xhigh/71_p2_mdc.md > /tmp/w7_proofedit_p2.py && python3 -B /tmp/w7_proofedit_p2.py",
      "result": "passed",
      "summary": "P2 3/5 dimensions, MDS boundary, small leaf DP, witness, integers, and artifact digest passed."
    },
    {
      "command": "python3 -B peers/KIMI_W6/w6/w6_mdc_checks.py",
      "result": "failed",
      "summary": "Full Q4 source recomputation exceeded the 24-second tool-call limit; no mathematical failure was emitted."
    },
    {
      "command": "sed -n '115,145p' analysis/xhigh/72_p3_bp1.md | sed 's/^    //' > /tmp/w7_proofedit_p3_uniform.py && python3 -B /tmp/w7_proofedit_p3_uniform.py",
      "result": "passed",
      "summary": "Every exact size-pair slack for uniform n=1..12 passed and reproduced the table."
    },
    {
      "command": "cc -O3 b4_frontier.c; cc -O3 n5_all16.c; cc -O3 b4_n5_cells.c; compile heavy-coordinate /tmp variant; run all",
      "result": "passed",
      "summary": "Five base BP1 classes HOLDS/TIGHT, Q5 E1=242/16 optimizers, 32/32 optimal sides R=0, and ball/light/heavy coordinate certificates matched."
    }
  ],
  "validationOutput": [
    "PASS W7-SOL-P1 checker; mcrit 0/16/18/18/19.",
    "W7-SOL-MDC-CHECK: PASS; critical dimensions 3/5; leaf witness 17/18.",
    "Uniform n=1..12: every D_n(a,b)>=0; n=12 minimum positive slack 13984.",
    "b4_frontier: HOLDS/TIGHT five classes; n5_all16: R=0 on 32/32 cells."
  ],
  "residualRisks": [
    "Full Q5-down BP1 and uniform n>=13 remain open.",
    "The packaged Q5 optimal-side checker hard-codes masks and omits its K=157 variant.",
    "The advertised independent W6 C++ source is absent.",
    "Full Q4 leaf coincidence source recomputation timed out in this runtime; supplied 66/66 artifact and smaller checks agree."
  ],
  "noStagedFiles": true,
  "diffSummary": "Created only the required proof-editor artifact; both source trees remained unchanged.",
  "reviewFindings": [
    "major: analysis/xhigh/70_p1_general_n.md:100-112 - Q3 heavy-vertex projection was used without proving the uniform-theta extremizer; repaired by explicit separable convexity.",
    "major: analysis/xhigh/70_p1_general_n.md:116-139 - block Fano omitted Schur-concavity needed for every theta; repaired.",
    "low: analysis/xhigh/70_p1_general_n.md:103-109 - r=8 made the displayed projection-threshold ratio 0/0; publication formula restricts the ratio to r<=7 and handles r=8 directly.",
    "major: analysis/xhigh/71_p2_mdc.md:157-168 - hidden Fable-floor PI dependency; replaced by direct Phi_n proof.",
    "medium: analysis/xhigh/71_p2_mdc.md:71-129,219-247 - rank-area equivalence, object identity, positive-rank scope, and category-relative non-reduction conflated; repaired.",
    "high: peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md:383-411 and 41_KIMIK3_THINKING_W6_PACKAGE.md:335-337 - wrong BP1 candidate/concavity and inequality-direction implications; excluded and corrected.",
    "no blockers in the corrected scoped P1/P2/P3 theorem suite"
  ],
  "manualNotes": "Read-only review. Only the authoritative external artifact was written."
}
```
