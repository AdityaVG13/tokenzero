# Wave 7 xhigh P5 master phase synthesis

**Outcome:** a certified master exists only as a union of exact slices. The arbitrary-\(n\), arbitrary-\((\rho,\lambda)\), arbitrary-cost full-prefix surface is not closed. The strongest surviving cells are the all-\(n\) registered-gauge sequential staircase, the all-\(n\) exact no-message face, the Q4 \(m\ge8\) exact unlinked rectangle, the registered-gauge batch theorem, the ISC formulas, the W4 finite-prefix inverses, and permanent MDC-FABLE/MDC-KIMI separation.

## 1. Lock and notation

- \(X\sim\mathrm{Unif}(\{0,1\}^n)\), \(S_1,\ldots,S_m\stackrel{\mathrm{iid}}\sim\theta\), independent of \(X\).
- \(\Theta_n^\downarrow=\{\theta:\theta_i\ge4/(5n)\}\), with heavy vertex \(v_n=((n+4),4,\ldots,4)/(5n)\). At \(n=4\), also \(\Theta_4^{\rm cap}=\{1/5\le\theta_i\le3/10\}\).
- Registered costs: \((h,q,c_0,c_1)=(1,0,1/2,1/2)\); \(s=h+q+c_0+c_1=2\). Registered gauge: \((\rho,\lambda)=(40,20)\); linked slice: \(\lambda=\rho/2\).
- Sequential parity: \((M,D,L)=(3m+2,0,4)\). Batch parity: \((5,0,4)\).
- No-recovery sequential prefix policy \(T\):
  \[
  M_T=(m+1)(1+\ell_T)+\rho e_T,\qquad
  L_T=1+\ell_T+c_{\rm comp}+\lambda e_T,\qquad D_T=e_T.
  \]
- Dominance is weak in all coordinates with at least one strict coordinate. Randomized prefix policies are included by conditioning and averaging.

Primary locks: <code>01_RADC_FORMAL_CORE_V1_FREEZE.md:71-100</code>, <code>10_SOLPRO_W5_CONT2.md:17-158</code>, <code>31_KIMI_W6_PACKAGE.md:77-114</code>.

## 2. Exact no-message face versus complete full-prefix hull

Define the worst-class no-message success on \(\Theta_n^\downarrow\):
\[
P_\downarrow(n,m)=2^{-n}\sum_{k=0}^{n-1}{n-1\choose k}
\left[\left(\frac{n+4+4k}{5n}\right)^m+
\left(\frac{4k}{5n}\right)^m\right].
\]
It is exact because \(P_{0,m}(\theta)=2^{-n}\sum_{B\subseteq[n]}\theta(B)^m\) is symmetric convex, hence Schur-convex, and is maximized at \(v_n\). For Q4 cap,
\[
P_{\rm cap}(m)=\frac1{16\,10^m}\sum_{a,b=0}^{2}{2\choose a}{2\choose b}(3a+2b)^m.
\]
For Q4 down, equivalently
\[
P_\downarrow(4,m)=\frac1{16\,5^m}\sum_{j=0}^{3}{3\choose j}\bigl(j^m+(j+2)^m\bigr).
\]

### Certified parallel table at \((40,20)\)

| \(n\), class | Exact no-message last winning \(m_{\rm NM}\) | Complete randomized full-prefix \(m_{\rm crit}\) | Universal prototype index \(m_{\rm obstr}\) | Latency / status |
|---|---:|---:|---:|---|
| \(2,\Theta_2^\downarrow\) | 14 | none | 14 | Full-prefix parity is killed for every \(m\) by identity \(L=3<4\). |
| \(3,\Theta_3^\downarrow\) | 16 | 16 | 17 | Weak \(L\) tie, strict \(M\); exact failure at \(m=17\). |
| \(4,\Theta_4^\downarrow\) | 18 | 18 | 18 | \(\gamma_L=1\); Cont-2. |
| \(4,\Theta_4^{\rm cap}\) | 18 | 18 | 18 | \(\gamma_L=1\); Cont-2. |
| \(5,\Theta_5^\downarrow\) | 18 | 18 | 18 | Strict \(L\); later Kimi/SolPro closure supersedes the earlier open \(m=4\ldots10\) strip. |
| \(n\ge6,\Theta_n^\downarrow\) | 19 | 19 | 19 | Strict \(L\); all-\(n\) proof is exact-tree/analytic union. |

Here
\[
m_{\rm obstr}(n,\rho)=\left\lfloor\frac{\rho(1-2^{-n})-1}{2}\right\rfloor
\]
is the largest index before the universal fixed-prototype obstruction guarantees failure; failure is guaranteed for \(m\ge m_{\rm obstr}+1\). It is not generally the exact class no-message threshold: at \(n=3\), \(m_{\rm NM}=16<17=m_{\rm obstr}\).

**Certificates:** <code>31_KIMI_W6_PACKAGE.md:132-162,195-264</code>; <code>33_KIMI_W6_VERIFICATION_LOG.md:176-260</code> (final verdict VERIFIED-WITH-NITS); <code>21_SOLPRO_W6_THEORY.txt:128-160,450-510</code>; <code>24_SOLPRO_W6_CHECKS.out</code>; independent finite corroboration in <code>41_KIMIK3_THINKING_W6_PACKAGE.md:239-258</code> and <code>61_QWEN_W6_PACKAGE.md:165-190,332-380</code>.

At \(n=4,m=18\), the exact \(M\)-margins are
\[
\gamma_M(\Theta_4^\downarrow)=\frac{277615146191}{762939453125}\approx0.363875724415,
\quad
\gamma_M(\Theta_4^{\rm cap})=
\frac{20074685943080277}{50000000000000000}\approx0.401493718862.
\]
At \(m=19\), a no-message baseline defeats parity. Cont-2 Python and independent C++ certificates both pass: <code>15_SOLPRO_W5_CONT2_CHECKS.out</code>, <code>27_SOLPRO_W6_CONT2_CPP_RERUN.out</code>.

## 3. Exact \((\rho,\lambda)\) cells

### 3.1 No-message rectangle, all \(n\)

Against the complete no-message face on \(\Theta_n^\downarrow\), sequential parity dominates iff
\[
\rho\ge\frac{2m+1}{1-P_\downarrow(n,m)},\qquad
\lambda\ge\frac{3}{1-P_\downarrow(n,m)}.
\]
On the linked slice, the exact no-message threshold is
\[
\rho^*_{\rm NM,linked}(n,m)=
\frac{\max\{2m+1,6\}}{1-P_\downarrow(n,m)}.
\]
This is a direct ledger derivation; equality is allowed because parity is still strictly better in distortion.

### 3.2 Q4 complete full-prefix tail rectangle

For either \(\Theta\in\{\Theta_4^\downarrow,\Theta_4^{\rm cap}\}\) and every integer \(m\ge8\), the no-message leaf is proved to be the exact active \(M/L\) constraint. Thus parity dominates the **complete randomized variable-length full-prefix hull iff**
\[
\boxed{\rho\ge\rho^*_{\Theta}(m)=\frac{2m+1}{1-P_\Theta(m)},\qquad
\lambda\ge\lambda^*_{\Theta}(m)=\frac{3}{1-P_\Theta(m)}.}
\]
On \(\lambda=\rho/2\), \(\rho^*=(2m+1)/(1-P_\Theta(m))\).

Selected exact cells:

| Class, \(m\) | \(\rho^*\) | \(\lambda^*\) |
|---|---:|---:|
| Q4 down, 18 | \(141143798828125/3563296863977\approx39.610451842\) | \(11444091796875/3563296863977\approx3.211658257\) |
| Q4 cap, 18 | \(74000000000000000000/1870074685943080277\approx39.570612102\) | \(6000000000000000000/1870074685943080277\approx3.208428008\) |
| Q4 down, 19 | \(595092773437500/14263650502901\approx41.720930649\) | \(45776367187500/14263650502901\approx3.209302358\) |
| Q4 cap, 19 | \(156000000000000000000/3742207147564718513\approx41.686628732\) | \(12000000000000000000/3742207147564718513\approx3.206663749\) |

Sources: <code>21_SOLPRO_W6_THEORY.txt:1183-1250</code>, <code>23_SOLPRO_W6_CHECKS.py</code>, <code>24_SOLPRO_W6_CHECKS.out</code>. This exact tail rectangle overrides any use of the one-demand value \(\lambda=\rho^*_{\rm class}/2\) as an exact \(m\ge8\) threshold.

### 3.3 General-\(n\) full-prefix arbitrary gauge

The only certified general form is a **certificate envelope**
\[
\widehat\rho(n,m)=\max\{\rho^*_{\rm NM}(n,m),\rho_{\rm tree}^{\rm cert}(n,m),\rho_L(n)\}.
\]
The no-message component is sharp; \(\rho_{\rm tree}^{\rm cert}\) is explicitly only sufficient. Therefore \(\widehat\rho\) must not be frozen as the exact least \(\rho^*\) away from the registered-gauge staircase or the Q4 \(m\ge8\) rectangle.

## 4. Batch versus sequential

| Timeline | Candidate | Baseline hull | Certified domain | Exact verdict |
|---|---|---|---|---|
| Batch | \((5,0,4)\) | Complete randomized batch no-recovery prefix hull | \(\Theta_n^\downarrow\), \((40,20)\), every \(m\ge1\) | Dominates for every \(n\ge3\); fails at \(n=2\). At \(n=3\), \(\gamma_M=3,\gamma_L=0\); at \(n=4\), margins \((5,0,1)\); \(n\ge5\): \(\gamma_M\ge5.4,\gamma_L\ge1.2\). |
| Sequential | \((3m+2,0,4)\) | Complete randomized variable-length no-recovery prefix hull | \(\Theta_n^\downarrow\), \((40,20)\) | Staircase: none, 16, 18, 18, 19 for \(n=2,3,4,5,\ge6\). |
| Sequential Q4 tail | \((3m+2,0,4)\) | Same | Q4 down/cap, \(m\ge8\), unlinked gauge | Exact rectangle in section 3.2. |

Batch proof: \(e_m\ge e_1\), so \(M_b\ge F_{n,\downarrow}(40)\) and \(2L_b\ge F_{n,\downarrow}(40)\); batch parity dominates iff \(F_{n,\downarrow}(40)\ge8\) with \(M\) strict. Source: <code>31_KIMI_W6_PACKAGE.md:264-271</code>; EC and adversarial closure: <code>33_KIMI_W6_VERIFICATION_LOG.md</code>.

## 5. ISC and W4 one-demand \(\rho^*(n,s,\Theta)\)

Let
\[
\psi(a)=2\left[1-\log_2(1+2^{-a/2})\right],\qquad T(s)=4+2s.
\]

### 5.1 ISC information-priced hull

| Demand class | Exact threshold | Quantifier/domain | Status |
|---|---|---|---|
| Uniform singleton | \(\rho^*_{\rm ISC}(n,s)=-2n\log_2\!\left(2^{1-(1+s)/n}-1\right)\) | \(n\ge2,\ 0\le s<n-1\); \(+\infty\) for \(s\ge n-1\) | DR; exact |
| \(\Theta_n^\downarrow\) ISC | Unique root of \(2+\psi(\rho(n+4)/(5n))+(n-1)\psi(4\rho/(5n))=4+2s\) | finite root for \(0\le s<n-1\); \(+\infty\) at/above the ceiling | DR; exact implicit |
| Uniform, \(n\to\infty\) | \(4(1+s)\) | fixed \(s\) | DR |
| \(\Theta_n^\downarrow, n\to\infty\) | \(10\log_2 x_s\), \(x_s^3=2^s(x_s+1)\) | fixed \(s\) | DR |

At \(s=2\), the last limit is \(10\log_2 x_2\approx12.527642810711\), \(x_2\approx2.382975767906\). Sources: <code>peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md:15-45</code>, corrected by <code>31_KIMI_W6_PACKAGE.md:380-404</code> and its verification log.

### 5.2 Exact finite-prefix one-demand inverses

These are for the full finite-prefix no-recovery hull, not ISC. Put \(T=4+2s\).

| Class | Exact \(\rho^*_{\rm FP}(T)\) | Ceiling |
|---|---|---:|
| Q4 cap | \(2(T-2)\) for \(T\le7\); \((10/3)(T-4)\) for \(7<T\le44/5\); \((40/7)(T-6)\) for \(44/5<T\le10\); \(+\infty\) for \(T>10\) | 10 |
| Q4 down | \(2(T-2)\) for \(T\le58/9\); \((40/11)(T-4)\) for \(58/9<T\le42/5\); \((20/3)(T-6)\) for \(42/5<T\le10\); \(+\infty\) for \(T>10\) | 10 |
| Q4 uniform | \(2(T-2)\) for \(T\le22/3\); \((16/5)(T-4)\) for \(22/3<T\le9\); \((16/3)(T-6)\) for \(9<T\le39/4\); \(8(T-29/4)\) for \(39/4<T\le10\); \(+\infty\) beyond | 10 |
| Q3 down | \(2(T-2)\) for \(T\le6\); \(4(T-4)\) for \(6<T\le31/4\); \((15/2)(T-23/4)\) for \(31/4<T\le8\); \(+\infty\) beyond | 8 |
| Q3 uniform | \(2(T-2)\) for \(T\le6\); \(4(T-4)\) for \(6<T\le8\); \(+\infty\) beyond | 8 |

At \(s=2,T=8\): Q4 cap \(40/3\), Q4 down \(160/11\), Q4 uniform \(64/5\), Q3 down \(135/8\), Q3 uniform \(16\). For \(n=2,T=8\), dominance is impossible because the floor ceiling is 6. Exact general \(n\ge5\) finite-prefix thresholds remain bracketed rather than solved.

Sources: <code>18_WAVE4_SOLPRO_PACKAGE_FULL.txt:4550-4850</code>; clean table and EC tags at <code>peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md:47-70</code>.

## 6. MDC-FABLE and MDC-KIMI: certified parallel rows

| Field | MDC-FABLE | MDC-KIMI / parity spine |
|---|---|---|
| Object | Sequential \(\pi_{EDC}^2\), dedup when \(S_2=S_1\) | Residual-rank-1 PARITY-DUAL |
| Two-demand ledger | \((9-p_c,0,11/2-(3/2)p_c)\), \(p_c=\sum_i\theta_i^2\) | Batch \((5,0,4)\); sequential \((8,0,4)\) |
| Mechanism | Law-dependent collision saving; expansions in \(\{1,2\}\), \(\Pr[2]=1-p_c\) | Algebraic fiber collapse; exactly one expansion |
| Certified phase | ZE identity dominance iff \(p_c\ge(9-2n)/3\); on \(\Theta_n^\downarrow\), scoped critical dimension 5; full lossy-hull claim at \((40,20)\) for \(n\ge5\) rests on peer floors/checker reruns | Full-prefix two-demand critical dimension 3; \(n=2\) latency kill, \(n\ge3\) registered-gauge dominance. Q4 margins: batch \((5,0,1)\), sequential \((7,0,1)\). |
| Scope | EDC/opaque multi-expand class. The \(n\le4\) kill is not a theorem about every exact-ref policy. | Rank-1 parity class; Cont-2 and the sequential staircase attach here. |

**Permanent separation theorem survives.** On every full-support law, Fable's expansion distribution is non-degenerate while Kimi's is a point mass at one; the ledgers coincide only at \(p_c=1\), a Dirac law excluded by full support. At the Q4 down heavy vertex, Fable has \((218/25,0,127/25)\) and fails \(L\)-dominance; Kimi sequential has \((8,0,4)\) and dominates. No merged MDC label is certified.

Sources and EC: <code>56_GROK_W6_05_MDC_RESOLUTION.md:15-125</code>; <code>41_KIMIK3_THINKING_W6_PACKAGE.md:264-288,438-459</code>; <code>31_KIMI_W6_PACKAGE.md:282-330</code>; <code>peers/DEEPSEEK_W6/checkers/tier3/m10_certificates.py</code> (9/9 certificates pass).

## 7. Open cells and explicit nonclaims

| Cell | Certified state |
|---|---|
| General \(n\), arbitrary \((\rho,\lambda)\), sequential full-prefix | **OPEN as an exact least-threshold surface.** Exact no-message component and sufficient tree certificate exist; exact all-tree threshold is known only on the registered staircase and Q4 \(m\ge8\) tail rectangle. |
| General \(n>4\), cap/band classes, full-prefix staircase | **OPEN.** The all-\(n\) closure is for \(\Theta_n^\downarrow\); Q4 cap is separately closed. |
| Q4 full-prefix unlinked \(m<8\) | One-demand W4 cells and selected two-demand/MDC cells are known; no single exact all-\(m<8\) rectangle was certified. |
| Finite-prefix one-demand exact \(\rho^*\), \(n\ge5\) | **OPEN corridor/brackets**, not the ISC threshold. |
| MDC merged track | Forbidden. Separation is proved; no reduction in either direction. |
| Production/tokenizer/real-agent interpretation | Not claimed. All results are locked formal models. |

Also not claimed: BP1 all \(n\), arbitrary real-agent agency RD, a production corridor map, or a complete surface in \((n,m,\rho,\lambda,h,q,c)\). See <code>01_RADC_FORMAL_CORE_V1_FREEZE.md:86-100</code>, <code>10_SOLPRO_W5_CONT2.md:931-960</code>, <code>41_KIMIK3_THINKING_W6_PACKAGE.md:492-506</code>.

## 8. Proposed theorem

### W7-SOL-P5-MASTER-SURVIVOR [PI|DR|EC] [M]

**Statement.** Under the lock in section 1, clauses 2--6 are simultaneously valid with exactly their displayed quantifiers: (i) the ISC thresholds and W4 finite-prefix inverses; (ii) the all-\(n\) exact no-message face; (iii) the registered-gauge batch theorem and sequential staircase; (iv) the Q4 \(m\ge8\) exact unlinked full-prefix rectangle; and (v) permanent MDC-FABLE/MDC-KIMI separation. No value is assigned to an open cell in section 7.

**Derivation.**

1. ISC and W4 rows are direct inversions of exact scalar floors at target \(T=4+2s\).
2. Schur convexity puts the no-message supremum at the heavy vertex, giving \(P_\downarrow\); comparing the exact ledgers gives the two no-message inequalities.
3. Leaf-occupancy transversality plus exact prefix spectra closes nontrivial trees; the exact no-message signs then give the registered staircase. The batch row follows from \(e_m\ge e_1\) and the W4 floor.
4. The SolPro active-leaf certificate proves that Q4, \(m\ge8\), has the no-message leaf as the exact active \(M/L\) constraint, yielding the unlinked rectangle.
5. Expansion-distribution invariance and the \(p_c\)-representation obstruction prove MDC separation.

**EC.** Fresh runs in this review: Cont-2 exact checker PASS; SolPro W6 checker PASS; DeepSeek/KimiK3 \(n=3\), Q4 \(\rho\)-surface, \(\lambda\), and MDC-separation checkers all exit 0. Stored independent evidence: SolPro C++ PASS; Kimi general-\(n\) checker 193 PASS/0 FAIL with final adversarial verdict VERIFIED-WITH-NITS; MDC 66/66; Grok 22/22.

**Nonclaims.** W7-SOL-P5-MASTER-SURVIVOR is a conjunction/synthesis theorem, not an assertion that the open cells form one exact master formula. In particular it does not promote \(\widehat\rho\) to the least general full-prefix threshold, merge MDC tracks, or map to production.

## 9. Review findings

1. **HIGH -- misleading exactness:** <code>31_KIMI_W6_PACKAGE.md:271-281,395-404</code> writes \(\rho^*=\max(\rho^*_{NM},\rho_{tree},\rho_L)\) while explicitly describing \(\rho_{tree}\) as sufficient-certified. This is not an exact least threshold outside the registered staircase. Freeze it as a certificate envelope only.
2. **HIGH -- \(\lambda\) scope collision:** <code>41_KIMIK3_THINKING_W6_PACKAGE.md:254-266</code> gives the one-demand \(\lambda^*=\rho^*_{class}/2\) floor threshold and phrases it as exact decoupling. For Q4 \(m\ge8\), the later exact full-prefix value is instead \(3/(1-P_\Theta(m))\), proved in <code>21_SOLPRO_W6_THEORY.txt:1214-1250</code>. Do not propagate the one-demand value into tail rows.
3. **MEDIUM -- Qwen attestation gap:** <code>61_QWEN_W6_PACKAGE.md:13-70,165-190</code> says arbitrary \(n\), but its displayed EC theorem is only \(n=3,4,5,6\), and <code>peers/QWEN_W6/</code> contains only four documentation/hash files, no claimed checker scripts. Its overlapping values survive through SolPro/Kimi/KimiK3 certificates, not through runnable Qwen artifacts in this bundle.
4. **LOW -- Fable uniqueness is false but phase values survive:** <code>peers/DEEPSEEK_W6/ec-peer-reruns/REPORT.md:56-63</code> records ties at \(n=2,8,16,\ldots\); antipodal value remains optimal, uniqueness does not. Do not promote uniqueness.
5. **LOW -- stale master prose:** <code>peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md:29-45</code> prints an approximate lower-capped ISC domain and the stale \(x_1\) label for the \(s=2\) limit. The remediated Kimi package uses \(x_2^3=4(x_2+1)\), verified in <code>33_KIMI_W6_VERIFICATION_LOG.md:200-235</code>.

## 10. Validation evidence

- <code>python3 12_SOLPRO_W5_CONT2_CHECKS.py</code>: all exact spectra, coverage, margins, monotonicity, tree barrier, and \(m=19\) obstruction PASS.
- <code>python3 23_SOLPRO_W6_CHECKS.py</code>: all W6 checks PASS, including staircase signs, Q4 exact unlinked rectangle, block-Fano barrier, and MDC separation.
- DeepSeek/KimiK3 tier checkers: \(n=3\) exact phase; Q4 \(\rho\)-surface; Q3 \(\lambda^*=135/16\); 9 MDC separation certificates -- all exit 0.
- Source tree was not edited. Only this requested artifact was written.