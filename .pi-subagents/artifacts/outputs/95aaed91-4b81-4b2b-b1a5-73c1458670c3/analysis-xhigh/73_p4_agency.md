# W7 P4 agency RD / decision-TV - proof and correction package

**Status:** REVIEWED, source-read-only.  
**Source root:** /Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT  
**Scope:** finite formal models only. No production-agent or arbitrary-policy claim.

## 0. Executive verdict

1. **The locked singleton binary agency theorem is exact:**
   \[
   R_{\mathrm{ag},\theta}(D)=1-H_2(D),\qquad 0\le D\le \tfrac12.
   \]
   It remains exact for a binary soft decision measured by TV to the correct point mass. No exact-expand hybrid can improve it when rate is the locked mutual information.
2. **The strongest true information-rate result is therefore a tight barrier, not an open improvement interval.** Exact expansion of one unbiased demanded bit costs one bit. Time-sharing with the endpoint \((0,1)\) produces chords above the convex soft curve.
3. **A genuine lossy+expand open interval exists only after locking the separate Model-H memory/latency ledger.** For uniform \(n=4\), \((h,q,c)=(1,0,1)\), the exact onset for Pareto dominance over pure recovery-aware soft is
   \[
   D_H=0.03979682699574900289\ldots,
   \]
   improving the coin-flip-only crossover \(D_{\rm CF}=0.04158686495638442061\ldots\). On
   \[
   (D_H,D^*),\qquad D^*=0.06149047007872417922\ldots,
   \]
   a genuinely mixed lossy-sketch+expand policy strictly beats both pure recovery-aware soft and pure no-recovery in both registered ledgers.
4. Peer claims that obtain agency information rate below \(1-H_2(D)\) charge exact expansion at \(\rho<1\), charge it only to latency, or use an invalid \(D/D_0\) entropy expression. Those are not the locked agency RD problem.

## 1. Evidence inventory and survivor decisions

| Artifact | Agency content reviewed | Decision |
|---|---|---|
| 01_RADC_FORMAL_CORE_V1_FREEZE.md:73-91 | Cont-1 agency curve is a freeze candidate; arbitrary real policies explicitly not frozen | Keep finite-only lock |
| 17_SOLPRO_W5_CONT1.md:27-40,53-196 | Binary theorem, no-recovery water filling, corridor | Reproved below; keep |
| 21_SOLPRO_W6_THEORY.txt:2166-2257 | Finite correct-action/soft-TV reduction to conditional Hamming RD | Keep with distribution-sensitive statement |
| 23_SOLPRO_W6_CHECKS.py:378-394 | q-ary normalization/KKT arithmetic | Pass, but weak EC only |
| 31_KIMI_W6_PACKAGE.md:363-386 | Joint multi-demand parity-noise upper bound and coverage converse | Keep only as a separate joint-error fragment |
| 37_KIMI_W6_PROOF_DEVELOPMENT.md:184-200 | Development version of the joint envelope | Reject stale minimum/error; main package corrected it |
| 41_KIMIK3_THINKING_W6_PACKAGE.md:292-308 | Subsidized-expand envelope, Model H, binary soft-TV | Relabel \(\rho<1\) as a cost envelope; strengthen Model H below |
| 42_DEEPSEEK_W6_PACKAGE.md:293-310 | W6-DS-HYBRID-LOSSY formula | Reject as malformed/uncertified |
| 54_GROK_W6_03_PROOFS.md:307-330 | Exact-expand chord barrier | Keep; strengthened from class fragment to consequence of the full converse |
| 61_QWEN_W6_PACKAGE.md:701 | Records no new hybrid theorem | Consistent nonclaim |
| peers/DEEPSEEK_W6/checkers/tier4/a4_hybrid_ec.py | \(\rho\)-envelope, coin-flip Model H, coarse frontier grid | Pass; exact threshold strengthened below |
| peers/DEEPSEEK_W6/checkers/tier4/a5_decision_tv_ec.py | Binary latent answer with expanded action/soft grids | Pass under its surjective binary-latent lock |
| peers/GROK_W6/checkers/w6_bp1_agency_phase.py | Soft-vs-expand grid/chord | Pass |

Duplicate checker identity was verified: 23_SOLPRO_W6_CHECKS.py equals peers/SOLPRO_W6/checkers/W6_THEORY_CHECKS.py (SHA-256 dc3a329e...c22), and the Grok checker equals its DeepSeek peer-rerun copy (SHA-256 ad05f71e...a53).

## 2. Locked models, distortions, gauges, and endpoints

### 2.1 Model B: binary singleton agency

- \(X=(X_1,\ldots,X_n)\), independent unbiased bits.
- \(S\sim\theta\), independent of \(X\), with full support unless stated otherwise.
- Pre-demand \(Z\) is generated from \(X\). Post-demand \(R\) is generated after observing \(S\).
- Hard answer \(\widehat A\in\{0,1\}\); distortion \(D=\Pr[\widehat A\ne X_S]\).
- Rate
  \[
  I(X;Z)+I(X;R\mid Z,S).
  \]
- Endpoints: \(D=0\) has rate 1; \(D=1/2\) has rate 0. For a constraint \(\mathbb E d\le D\), the curve stays 0 for \(D\ge1/2\).

### 2.2 Model F: finite correct action and soft decision-TV

Let finite \(A^*=a(X,S)\). A transcript emits a distribution \(Q(\cdot\mid Z,R,S)\). Distortion is
\[
\mathbb E\,d_{\rm TV}(\delta_{A^*},Q)=\mathbb E[1-Q(A^*)].
\]
This is not universally \(1-H_2(D)\). Its exact value is the conditional Hamming RD function of \(A^*\mid S\). The binary unbiased singleton is the specialization yielding \(1-H_2(D)\). For uniform \(q\)-ary \(A^*\), the endpoint is \(D=1-1/q\), not \(1/2\).

If the allowed soft grid omits point masses, there is an irreducible distortion floor. The A5 checker example \(\{1/4,1/2,3/4\}\) has \(D_{\min}=1/4\) and shifted curve \(1-H_2(2(D-1/4))\) on \([1/4,1/2]\).

### 2.3 Model J: joint/adaptive multi-demand DTV

31_KIMI_W6_PACKAGE.md uses joint error: failure of at least one demanded answer. Its parity-noise construction has rate \(n-H_2(D)\), retains the \(n-1\)-bit complement handle at \(D=1/2\), and is not the singleton theorem. Its coverage converse leaves an \(nD\) gap. Adaptive demands require a directed-information or explicit conditioning audit because \(S_{1:m}\) may become correlated with \(X\); no exact adaptive RD theorem is frozen here.

### 2.4 Model H: carried-token memory/latency ledger

This is a cost model, not Shannon agency rate. For one demand, no-recovery curve \(r(d)=R_{\rm NR,\theta}(d)\), \(s=h+q+c\), and
\[
\begin{aligned}
M_{\rm NR}(d)&=2+2r(d),&L_{\rm NR}(d)&=1+r(d),\\
M_{\rm RA}(D)&=2+f(D)+2h+q,&L_{\rm RA}(D)&=1+f(D)+s,
\end{aligned}
\]
where \(f(D)=1-H_2(D)\).

A Model-H lossy-sketch+expand member chooses \(d\in[D,1/2]\) and
\[
\alpha=1-D/d,
\]
then has
\[
M_H=2+2r(d)+\alpha(1+2h+q),\qquad
L_H=1+r(d)+\alpha(1+s).
\]
The distortion identity is \(D=(1-\alpha)d\). Genuine mixing means \(D<d<1/2\).

## 3. W7-SOL-AG-BINARY-RD [S] [DR]

### Theorem

Under Model B,
\[
\boxed{R_{\mathrm{ag},\theta}(D)=1-H_2(D),\quad 0\le D\le1/2.}
\]
The value is independent of the full-support demand law \(\theta\).

### Proof

Because \(Z\) is pre-demand and \(S\perp(X,Z)\),
\[
I(X;Z)+I(X;R\mid Z,S)=I(X;Z,R\mid S).
\]
Conditioned on \(S=s\), \(X_s\) is an unbiased bit and
\[
X_s\longrightarrow X\longrightarrow(Z,R)\longrightarrow\widehat A
\]
is a valid data-processing chain. Hence
\[
I(X;Z,R\mid S)\ge I(X_S;\widehat A\mid S).
\]
Let \(E=1\{X_S\ne\widehat A\}\). Given the binary \(\widehat A\) and \(E\), \(X_S\) is determined, so
\[
H(X_S\mid\widehat A,S)\le H(E\mid S)\le H_2(D).
\]
Since \(H(X_S\mid S)=1\),
\[
I(X_S;\widehat A\mid S)\ge1-H_2(D).
\]

For achievability, take \(Z\) constant, independent \(N\sim\mathrm{Bernoulli}(D)\), and send
\[
R=X_S\oplus N,\qquad \widehat A=R.
\]
Then the error is \(D\) and
\[
I(X;R\mid S)=I(X_S;X_S\oplus N\mid S)=1-H_2(D).
\]
Converse and achievability coincide. \(\square\)

**Correction to the peer proof:** 17_SOLPRO_W5_CONT1.md:68 uses the information identity without spelling out the needed pre-demand independence \(S\perp(X,Z)\). The theorem is correct once that lock is explicit.

## 4. W7-SOL-AG-FINITE-TV [S] [DR]

### Theorem

Under Model F,
\[
\boxed{
R_{\rm ag}^{\rm TV}(D)=
\inf_{P_{A\mid A^*,S}:\Pr[A\ne A^*]\le D} I(A^*;A\mid S).
}
\]
For an unbiased binary \(A^*\), this is \(1-H_2(D)\).

### Proof

Given soft \(Q\), sample \(A\sim Q\) with fresh decoder randomness. Then
\[
\Pr[A\ne A^*]=\mathbb E[1-Q(A^*)]\le D.
\]
Conditioned on \(S\),
\[
A^*\longleftarrow X\longrightarrow(Z,R)\longrightarrow A,
\]
so data processing gives
\[
I(X;Z,R\mid S)\ge I(A^*;A\mid S).
\]
Taking the infimum gives the converse.

For any admissible test channel \(P_{A\mid A^*,S}\), take \(Z\) constant, generate \(A\) from \((A^*,S)\), send \(R=A\), and emit \(\delta_A\). Since \(A^*\) is a function of \((X,S)\) and \(X\to A^*\to A\) given \(S\),
\[
I(X;R\mid S)=I(A^*;A\mid S).
\]
This attains the infimum. \(\square\)

### Consequences and correction

- Binary soft randomization buys nothing over a hard BSC endpoint.
- For uniform \(q\)-ary \(A^*\),
  \[
  R(D)=\log_2q-H_2(D)-D\log_2(q-1),\quad 0\le D\le1-1/q.
  \]
- 41_KIMIK3_THINKING_W6_PACKAGE.md:308 is valid only because its checker locks a binary latent answer and a surjective map from a larger action alphabet. Read literally for arbitrary \(k\)-ary correct actions, the claim \(1-H_2(D)\) is false.

## 5. W7-SOL-AG-EXPAND-BARRIER [S] [DR+EC]

### Theorem

In Model B, exact expansion cannot produce any lossy open interval below \(f(D)=1-H_2(D)\). This holds for every protocol, hence for every lossy+expand hybrid.

### Proof 1: full converse

W7-SOL-AG-BINARY-RD lower-bounds every valid transcript by \(f(D)\). Exact expansion is only a subclass. Equality is already attained by the BSC reply, so the barrier is tight. \(\square\)

### Proof 2: explicit time-sharing geometry

Suppose a hybrid uses a soft point \((d,f(d))\) with fraction \(\beta=D/d\) and exact expansion with fraction \(1-\beta\). Exact recovery of an unbiased demanded bit has information cost at least 1, so its endpoint is \((0,1)\). The hybrid rate is
\[
R_H=\beta f(d)+(1-\beta)f(0).
\]
Because \(f=1-H_2\) is strictly convex,
\[
R_H\ge f(\beta d+(1-\beta)0)=f(D),
\]
with equality only at degenerate endpoints. \(\square\)

### Exact correction of the \(\rho\)-charged peer envelope

The A4a calculation is mathematically correct only as a separate subsidized cost envelope
\[
C_\rho(D)=\min_{d\in[D,1/2]}
\left[\frac Dd f(d)+\left(1-\frac Dd\right)\rho\right].
\]
Define
\[
\rho^*(D)=1+\log_2(1-D).
\]
Then
\[
C_\rho(D)=
\begin{cases}
f(D),&\rho\ge\rho^*(D),\\
\rho-D\log_2\dfrac{1-d_\rho}{d_\rho},&0\le\rho<\rho^*(D),
\end{cases}
\qquad d_\rho=1-2^{\rho-1}.
\]
Indeed, the chord comparison threshold is
\[
\Phi(D,d)=\frac{d f(D)-D f(d)}{d-D},
\]
and strict convexity makes \(\Phi\) decrease in \(d\), with
\[
\sup_{d>D}\Phi(D,d)=f(D)-Df'(D)=1+\log_2(1-D).
\]
The tangent equation gives \(d_\rho\) and the displayed envelope. For genuine information rate, \(\rho\ge1\); since \(\rho^*(D)\le1\), only the barrier branch survives. A4b's \(\rho=0\) optimum is therefore a latency/subsidy objective, not \(R_{\rm ag}\).

## 6. W7-SOL-MODELH-CF-CROSSOVER [S] [DR]

Let
\[
a_M=1+2h+q,\qquad a_L=1+s.
\]
The coin-flip member takes \(d=1/2\), \(r(1/2)=0\), \(\alpha=1-2D\). Relative to pure recovery-aware soft,
\[
\Delta_M=M_{\rm CF}-M_{\rm RA}=H_2(D)-2a_MD,
\]
\[
\Delta_L=L_{\rm CF}-L_{\rm RA}=H_2(D)-2a_LD.
\]
Because \(H_2(D)/D\) strictly decreases from \(+\infty\) to 2 on \((0,1/2]\):

- CF jointly dominates RA iff \(H_2(D)<2\min(a_M,a_L)D\).
- RA jointly dominates CF iff \(H_2(D)>2\max(a_M,a_L)D\).
- If \(a_M\ne a_L\), the interval between those roots is ledger-incomparable.
- A strict CF interval exists iff \(\min(a_M,a_L)>1\).

At \((h,q,c)=(1,0,1)\), \(a_M=a_L=3\), so the unique crossover solves
\[
H_2(D_{\rm CF})=6D_{\rm CF},
\]
with
\[
D_{\rm CF}=0.0415868649563844206070\ldots.
\]

## 7. W7-SOL-MODELH-4U-FRONTIER [F] [DR+EC]

### Complete family specification

For fixed \(D\), every policy in the locked one-parameter Model-H family is exactly the image
\[
d\in[D,1/2]\mapsto(M_H(D,d),L_H(D,d)).
\]
For a scalar ledger with coefficient \(k\in\{1,2\}\) on \(r\) and expansion coefficient \(a\in\{a_L,a_M\}\), every interior stationary frontier candidate satisfies
\[
kr'(d)+\frac{aD}{d^2}=0;
\]
endpoints \(d=D,1/2\) must also be tested. The complete nondominated curve is obtained by evaluating these candidates and deleting pairwise dominated images. This is a frontier specification for Model H only, not for all interactive protocols.

### Exact registered-gauge threshold

Lock uniform \(n=4\), so \(r(d)=4f(d)\), and \((h,q,c)=(1,0,1)\). Then
\[
\begin{aligned}
M_H&=2+8f(d)+3(1-D/d),\\
L_H&=1+4f(d)+3(1-D/d),\\
M_{\rm RA}&=4+f(D),\\
L_{\rm RA}&=3+f(D).
\end{aligned}
\]
Thus
\[
\Delta_M(D,d)=H_2(D)+8f(d)-\frac{3D}{d}=:F(D,d),
\]
\[
\Delta_L(D,d)=F(D,d)-4f(d)\le F(D,d).
\]
Memory is the binding ledger.

Let \((D_H,d_H)\) be the unique solution in
\((0.039,0.040)\times(0.48,0.50)\) of
\[
D_H=\frac83d_H^2\log_2\frac{1-d_H}{d_H},
\]
\[
H_2(D_H)+8f(d_H)-\frac{3D_H}{d_H}=0.
\]
Then
\[
\boxed{D_H=0.0397968269957490028937902026779\ldots}
\]
\[
\boxed{d_H=0.489195292037718884930182531448\ldots}
\]
and \(\alpha_H=1-D_H/d_H=0.91864838512656718149\ldots\).

### Theorem

1. For \(0<D<D_H\), no Model-H family member Pareto-dominates pure RA.
2. At \(D=D_H\), \(d=d_H\) ties memory and strictly improves latency.
3. For every \(D\in(D_H,1/2)\), a genuinely mixed member \(D<d<1/2\) strictly improves both memory and latency over pure RA.

### Proof

Write \(\ell(d)=\log_2((1-d)/d)\) and
\[
g(d)=\frac83d^2\ell(d).
\]
Then
\[
\partial_dF(D,d)=\frac3{d^2}[D-g(d)].
\]
Moreover, the sign of \(g'(d)\) is the sign of
\[
2\ln\frac{1-d}{d}-\frac1{1-d},
\]
which is strictly decreasing from \(+\infty\) to a negative value. Hence \(g\) is unimodal.

For \(D\le0.04\), \(g(D)<D\), and \(g\) exceeds \(D\) in the interior. Therefore the global minimum of \(F(D,\cdot)\) is either the left endpoint or the upper stationary root \(d_+(D)\). The left value is
\[
F(D,D)=5-7H_2(D)>0
\]
on this range. Along the upper stationary branch, the envelope derivative is
\[
\frac d{dD}F(D,d_+(D))=\ell(D)-\frac3{d_+(D)}.
\]
Here \(d_+(D)\) decreases with \(D\), so this derivative strictly decreases. The envelope starts at 0 with positive right derivative, reaches one maximum, and then decreases. The certified sign change at the displayed system therefore gives its unique positive zero \(D_H\). This proves items 1 and 2.

For \(D_H<D<d_H\), hold \(d=d_H\). Since \(D_H>1/65\),
\[
\partial_DF(D,d_H)=\ell(D)-3/d_H<6-3/d_H<0,
\]
so \(F(D,d_H)<0\). For \(D\ge d_H\), the boundary value \(F(D,D)=5-7H_2(D)<0\); continuity permits a choice \(d>D\) arbitrarily close to \(D\), preserving strict negativity and making \(\alpha>0\). Since \(\Delta_L\le\Delta_M\), both ledgers strictly improve. \(\square\)

### Strong open interval against both pure endpoints

The Cont-1 uniform corridor endpoint at \(n=4,s=2\) satisfies
\[
3f(D^*)=2,
\qquad
D^*=H_2^{-1}(1/3)=0.06149047007872417922\ldots.
\]
For \(D<D^*\), pure RA strictly dominates pure NR in both ledgers. Combining with the theorem gives
\[
\boxed{D\in(D_H,D^*)\implies (M_H,L_H)<(M_{\rm RA},L_{\rm RA})<(M_{\rm NR},L_{\rm NR}).}
\]
This is the requested finite, genuinely lossy+expand, open-interval certificate. At the simple rational-decimal witness \(D=0.04,d=0.49\),
\[
\Delta_M=-0.0002973041237380654\ldots,
\qquad
\Delta_L=-0.0014515371124983866\ldots.
\]

## 8. EC code/spec and transcript

The independent checker used Decimal at 70 digits and the following obligations:

    1. Bisect the Model-H tangency equation K(d)=0 on [0.48,0.499].
    2. Recover D=(8/3)d^2 log2((1-d)/d), and assert
       0.039796826995748 < D < 0.039796826995750.
    3. Bisect H2(D)=6D and H2(D)=1/3; assert D_H < D_CF < D*.
    4. Evaluate the explicit D=0.04,d=0.49 memory and latency margins.
    5. Exhaust a 99x100 binary soft/expand grid and assert every information-rate chord is at least f(D).

Transcript:

    PASS W7-SOL-AG-BARRIER dense grid
    PASS W7-SOL-MODELH Dc= 0.03979682699574900289379020267792612045623194971603383242120671399283551
    dc= 0.4891952920377188849301825314475563898140645691256315591311175249838644
    alpha_c= 0.9186483851265671814897706242552528347298222648002626004701533866936209
    Dcf= 0.04158686495638442060703280906923941897539008671196342761072203665844320
    Dstar= 0.0614904700787241792218932428890720151047304520969757739721612214902482
    D=.04 explicit margins dM= -0.0002973041237380654...
    dL= -0.0014515371124983866...

Peer checker reruns:

    PASS W6-AGTV-CONDITIONAL-RD: q-ary symmetric-channel and water-filling arithmetic, 2<=q<=16
    ALL W6 THEORY CHECKS PASS

    min [R_hybrid - f(D)] = 0.001779 >= 0 [W6-GROK-AG-HYBRID-TV confirmed]
    Ddagger solving H2(D)=6D: 0.041587
    D=0.02: exists hybrid dominating RA: False
    D=0.05,0.1,0.2,0.3,0.45: exists hybrid dominating RA: True
    PASS a4: hybrid audit + threshold theorem

    A5 k=2,3,4 binary-latent action tests: max BA errors 2.78e-16, 2.71e-06, 2.22e-16
    grid no-endpoints full-info distortion d_min = 1/4 exactly
    PASS a5: decision-TV audit

    PASS agency hybrid: pure soft 1-H2(D) dominates expand-time-sharing on grid
    PASS agency: R_soft(1/4) < chord-to-expand(1/4)=1/2
    PASS all BP1/agency/phase EC checks

## 9. Review findings

1. **BLOCKER - invalid construction:** 42_DEEPSEEK_W6_PACKAGE.md:293-304 says \(D_0<D\) but evaluates \(H_2(D/D_0)\), placing its argument above 1; it provides no valid gate/test channel. W6-DS-HYBRID-LOSSY must not be promoted.
2. **HIGH - objective conflation:** 41_KIMIK3_THINKING_W6_PACKAGE.md:292-300 calls \(\rho<1\), especially \(\rho=0\), an agency rate. Exact recovery of an unbiased demanded bit costs one mutual-information bit. Preserve A4a only as \(C_\rho\), and A4b only as a latency/subsidy ledger.
3. **HIGH - stale arithmetic contradiction:** 37_KIMI_W6_PROOF_DEVELOPMENT.md:184-190 states \(\min(n-2D,n-H_2(D))=n-2D\). Since \(H_2(D)\ge2D\), the minimum is \(n-H_2(D)\). 31_KIMI_W6_PACKAGE.md corrected this; the development note and its downstream gap text are non-authoritative.
4. **HIGH - adaptive joint-model gap:** 31_KIMI_W6_PACKAGE.md:363-386 asserts adaptivity invariance while conditioning rate on adaptive \(S_{1:m}\). Once demands depend on prior \(X\)-dependent transcript, \(S_{1:m}\not\perp X\); the singleton identity and an \(n\)-bit entropy start do not follow. Require directed information or an exogenous-demand lock.
5. **MEDIUM - missing action-source qualifier:** 41_KIMIK3_THINKING_W6_PACKAGE.md:308 says \(k\)-action RD is \(1-H_2(D)\). The checker actually uses a binary latent source with a surjective action map. General finite correct actions use conditional Hamming RD.
6. **MEDIUM - EC scope:** 23_SOLPRO_W6_CHECKS.py:378-394 checks q-ary channel normalization and parameter ranges, not the finite conditional-RD converse. Treat theorem confidence as DR, with EC arithmetic support only.
7. **LOW - source corruption:** 17_SOLPRO_W5_CONT1.md:155 contains a form-feed-corrupted \(\frac\). It is typographical, not mathematical.

## 10. Dependency graph, ambition/status tags, and obstructions

| ID | Tags | Depends on | Status |
|---|---|---|---|
| W7-SOL-AG-BINARY-RD | [S] [DR] | Model B, binary Fano/data processing | PROVED |
| W7-SOL-AG-FINITE-TV | [S] [DR] | finite correct action, decoder sampling | PROVED |
| W7-SOL-AG-EXPAND-BARRIER | [S] [DR+EC] | binary RD theorem, convexity | PROVED, tight |
| W7-SOL-AG-RHO-ENVELOPE | [S] [DR+EC] | artificial \(\rho\)-charged endpoint | PROVED as cost envelope, not agency RD |
| W7-SOL-MODELH-CF-CROSSOVER | [S] [DR+EC] | Model H ledger, entropy-ratio monotonicity | PROVED |
| W7-SOL-MODELH-4U-FRONTIER | [F] [DR+EC] | uniform \(n=4\), \((1,0,1)\), Cont-1 \(r=4f\) | PROVED for locked family |

Obstruction map:

- **Information-rate hybrid:** closed by the exact converse. Any claimed improvement must change the rate accounting or the model.
- **Arbitrary finite actions:** distribution-sensitive conditional Hamming RD; no universal binary formula.
- **Joint adaptive demands:** directed-information/exogenous-demand issue remains open.
- **Model H general \((n,\theta,h,q,c)\):** exact one-parameter image is known, but a closed-form global Pareto threshold requires solving the law-dependent \(r'(d)\) stationarity and ledger incomparability.
- **Real agents/production traces:** no finite source/action law, tokenizer, latency distribution, or empirical policy map is supplied.

## 11. Explicit nonclaims

- No theorem for arbitrary real agent policies, production traces, TokenZero cost, or tokenizer bytes.
- No identification of Model H with mutual-information \(R_{\rm ag}\).
- No exact joint/adaptive multi-demand RD curve.
- No claim that the Model-H one-parameter family exhausts all interactive protocols.
- No use of the rejected 42_DEEPSEEK W6-DS-HYBRID-LOSSY formula.
- No source-tree edits; only this analysis artifact is written.

## 12. Review gate

**REVIEWED-PASS for P4 finite scope.** Proof assumptions are explicit; binary, finite-action soft-TV, joint multi-demand, subsidized endpoint, and Model-H ledgers are separated. Every promoted numeric threshold has an independent rerun transcript. The blocker peer construction and objective conflations are excluded from survivor dependencies.


```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Created only the requested 73_p4_agency.md artifact; source bundle remained read-only. Published finite P4 proofs, corrections, EC, tags, obstructions, and nonclaims."
    },
    {
      "id": "criterion-2",
      "status": "satisfied",
      "evidence": "Independent review evidence includes exact file/line findings, complete converse/achievability proofs, Decimal threshold brackets, checker reruns, duplicate hashes, and residual-risk boundaries."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/95aaed91-4b81-4b2b-b1a5-73c1458670c3/analysis-xhigh/73_p4_agency.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "python3 23_SOLPRO_W6_CHECKS.py | rg 'AGTV|ALL|FAILED|Traceback'",
      "result": "passed",
      "summary": "W6 AGTV arithmetic passed; all W6 theory checks passed."
    },
    {
      "command": "python3 peers/DEEPSEEK_W6/checkers/tier4/a4_hybrid_ec.py",
      "result": "passed",
      "summary": "Expand chord, rho threshold, coin-flip crossover, and coarse Model-H frontier passed."
    },
    {
      "command": "python3 peers/DEEPSEEK_W6/checkers/tier4/a5_decision_tv_ec.py",
      "result": "passed",
      "summary": "Binary-latent finite-action and soft-TV BA checks passed."
    },
    {
      "command": "python3 peers/GROK_W6/checkers/w6_bp1_agency_phase.py | rg -i 'agency|hybrid|pass|fail'",
      "result": "passed",
      "summary": "Pure-soft expand barrier and point certificate passed."
    },
    {
      "command": "inline Python Decimal W7 Model-H tangency/crossover/corridor checker plus 99x100 expand grid",
      "result": "passed",
      "summary": "Certified D_H, d_H, D_CF, D*, explicit negative margins, and the information-rate barrier."
    },
    {
      "command": "sha256sum duplicate SolPro and Grok agency checker copies",
      "result": "passed",
      "summary": "SolPro pair and Grok pair are byte-identical."
    }
  ],
  "validationOutput": [
    "PASS W7-SOL-AG-BARRIER dense grid",
    "PASS W7-SOL-MODELH D_H=0.0397968269957490028937902026779...",
    "D_CF=0.0415868649563844206070...; D*=0.0614904700787241792219...",
    "At D=0.04,d=0.49: Delta_M=-0.000297304123738... and Delta_L=-0.001451537112498...",
    "PASS a4, PASS a5, PASS Grok agency, ALL W6 THEORY CHECKS PASS"
  ],
  "residualRisks": [
    "General adaptive joint-demand RD remains unproved because adaptive demands can correlate with X.",
    "The exact Model-H threshold is proved only for uniform n=4 and registered gauge (h,q,c)=(1,0,1).",
    "A5 numerical BA evidence uses a binary latent source; arbitrary finite correct-action laws remain distribution-sensitive.",
    "The KIMI all-purpose checker was inspected but not rerun because its final statement writes an absolute /mnt/agents output, violating source-read-only execution."
  ],
  "noStagedFiles": true,
  "diffSummary": "Added one requested analysis artifact; no source, checker, or bead state changes.",
  "reviewFindings": [
    "blocker: 42_DEEPSEEK_W6_PACKAGE.md:293-304 - D0<D makes H2(D/D0) invalid and no test channel is specified.",
    "high: 41_KIMIK3_THINKING_W6_PACKAGE.md:292-300 - rho<1 exact-expand accounting is not mutual-information agency rate.",
    "high: 37_KIMI_W6_PROOF_DEVELOPMENT.md:184-190 - stale envelope picks n-2D instead of n-H2(D).",
    "high: 31_KIMI_W6_PACKAGE.md:363-386 - adaptive-demand invariance lacks the independence/directed-information lock.",
    "medium: 41_KIMIK3_THINKING_W6_PACKAGE.md:308 - k-action formula omits the binary-latent qualifier."
  ],
  "manualNotes": "Reviewer gate passed for the finite P4 scope. The strongest Shannon result is the tight expand barrier; the open interval belongs only to the separately locked Model-H ledger."
}
```
