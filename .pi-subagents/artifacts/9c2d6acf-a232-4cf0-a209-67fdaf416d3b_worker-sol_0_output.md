# Wave 7 substrate + methods formal audit

**Scope:** read-only audit of the mandatory 26 files under `wave7-attach-FLAT`, in the required order. Source claims were treated as untrusted. Only the required artifact below was written. All citations use `[LABEL | theorem/family ID | relative path]`.

## 1. Audit verdict

1. **Attested finite result:** the Continuation-2 Q4 theorem is internally coherent at the locked gauge and its Python, independent C++ certificate, 45-run grid output, and nine-file SHA manifest reproduced exactly. The attested scope is Q4, the two registered demand polytopes, the complete randomized variable-length no-recovery prefix class, and integer demand counts. It does not establish arbitrary (n). [SOLPRO_W5_CONT2 | W5-SOL-MDC-Q4-FULL-18/19 | `10_SOLPRO_W5_CONT2.md`]
2. **Not an attested Core as a whole:** `01_RADC_FORMAL_CORE_V1_FREEZE.md` calls its entries working/freeze candidates and says peer/Sol Pro labels remain claims until independent EC. The Wave4 text supplies final DR/EC labels but no runnable Wave4 checker file exists under the audited root. [CORE_FREEZE | W4-DP / W4-FLOOR | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]; [WAVE4_SOLPRO | W4-DP-Q4 | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
3. **Cont-1 is partly superseded:** its no-message (mle18) conclusion contains an insufficient monotonicity sentence. Cont-2 explicitly identifies and repairs that gap. The Cont-1 result should not be cited standalone without the Cont-2 repair. [SOLPRO_W5_CONT1 | W5-SOL-MDC-NOMSG-18/19 | `17_SOLPRO_W5_CONT1.md`]; [SOLPRO_W5_CONT2 | W5-SOL-Q4-NOMSG-REPAIR | `10_SOLPRO_W5_CONT2.md`]
4. **Methods are not evidence:** files 70 and 72 expressly describe method transfer, post-cutoff exemplars, speculative bridges, and prove targets. They add no frozen theorem. [METHOD_AI_MATH | METHOD-ONLY | `70_ADJACENT_MATH_AI_PROOF_METHODS.md`]; [METHOD_OMEGA_FRANKENSIM | METHOD-ONLY | `72_OMEGA_FRANKENSIM_MATH_TRANSFER.md`]

## 2. Locked definitions and gauges

### 2.1 Proof-status lock

- `PI`: published input with classical scope; `DR`: deduction from stated hypotheses, not peer-reviewed novelty; `EC`: finite exact computation with checker; `BE`: bounded search only; `SB`: speculative bridge. [WAVE4_SOLPRO | STATUS-LOCK | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
- Files 70/72 use the equivalent long-form hierarchy and prohibit promotion of estimated/bounded evidence to a theorem or production claim. [METHOD_AI_MATH | STATUS-LOCK | `70_ADJACENT_MATH_AI_PROOF_METHODS.md`]; [METHOD_OMEGA_FRANKENSIM | EVIDENCE-COLORS | `72_OMEGA_FRANKENSIM_MATH_TRANSFER.md`]

### 2.2 Q4 demand and policy lock

[
Theta_4^{downarrow}={	hetainDelta_3:	heta_ige1/5},qquad
Theta_4^{mathrm{cap}}={	hetainDelta_3:1/5le	heta_ile3/10}.
]
Here (Xsimmathrm{Unif}({0,1}^4)), (S_1,ldots,S_mstackrel{iid}{sim}	heta), and (S_{1:m}perp X). A deterministic no-recovery encoder emits a source-dependent prefix-free transcript before demands, inducing nonempty leaves (A_j) with depths (d_j) and
[
ell=rac1{16}sum_j |A_j|d_j.
]
The decoder receives no correctness oracle or other source-dependent feedback between demands. With joint success (P_T) and (e_T=1-P_T),
[
M_T=(m+1)(1+ell)+40e_T,quad
L_T=1+ell+c_{m comp}+20e_T, c_{m comp}ge0,quad D_T=e_T.
]
The sequential parity/complement policy has
[
(M_{m par},D_{m par},L_{m par})=(3m+2,0,4).
]
Conditioning and averaging extends deterministic inequalities to randomized policies because (M,L,P) are affine under mixtures. [SOLPRO_W5_CONT2 | STATEMENT-LOCK | `10_SOLPRO_W5_CONT2.md`]

The registered sequential gauge is
[
(ho_{m fail},lambda_{m fail})=(40,20).
]
[SOLPRO_W5_CONT2 | W5-SOL-MDC-Q4-FULL-18/19 | `10_SOLPRO_W5_CONT2.md`]

### 2.3 Agency RD lock

Let (f(d)=1-H_2(d)), (0le dle1/2). No-recovery pre-demand rate is (I(X;Z)); recovery-aware agency rate is
[
I(X;Z)+I(X;Rmid Z,S).
]
For the Q4 multi-demand statement, distortion is failure of at least one of the (m) answers. [SOLPRO_W5_CONT1 | STATEMENT-LOCK | `17_SOLPRO_W5_CONT1.md`]

For corridor overhead (s=h+q+c), the locked endpoint uses
[
G_	heta(D)=R_{m NR,	heta}(D)-[1-H_2(D)],qquad G_	heta(D_	heta^star)=s.
]
[SOLPRO_W5_CONT1 | W5-SOL-AGRD-THETA-CORRIDOR | `17_SOLPRO_W5_CONT1.md`]

### 2.4 Wave4 ledger, exact reference, and dominance lock

For (Xsimmathrm{Unif}(mathbb F_2^n)), (Sperp X), a no-recovery baseline is (C=f(X,U)), (Uperp(X,S)), with per-seed binary prefix-free serialization. Its linked-gauge ledger is
[
M=2(1+ell)+ho_{m fail}e_	heta,qquad
L=1+ell+c_{m comp}+lambda_{m fail}e_	heta,qquad
lambda_{m fail}=ho_{m fail}/2.
]
[WAVE4_SOLPRO | STATEMENT-LOCK | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

For an opaque exact reference expanded on every singleton demand,
[
M_{m opaque}=3+2h+q,qquad
L_{m opaque}=2+h+q+c_0+c_1.
]
The registered instance is
[
(h,q,c_0,c_1)=(1,0,1/2,1/2),qquad (M,D,L)=(5,0,4).
]
[WAVE4_SOLPRO | W4-AFF-Q4-40 | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

For visible state (H), (F(h)={x:Pr[H=hmid X=x]>0}), (delta_i(H)) is the probability that coordinate (i) is constant on (F(H)), (Delta_	heta(H)=sum_i	heta_idelta_i(H)), and (Delta_Theta(H)=inf_{	hetainTheta}Delta_	heta(H)). A linear alias exposes (Z=AX), (Ainmathbb F_2^{r	imes n}), and for batch (Q),
[
r_A(Q)=dimpi_Q(ker A).
]
[WAVE4_SOLPRO | W4-DETERMINATION-FLOOR / W4-LINEAR-ALIAS-RANK | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

Locked dominance quantifies over every (	hetainmathrm{relint}(Theta)) and every baseline-hull point, requiring candidate margins (M(a)le M(b)-gamma_M), (D(a)le D(b)-gamma_D), (L(a)le L(b)-gamma_L), with at least one positive margin. Formal exact-reference claims are not production claims without tokenizer, handle, store-survival, and latency mapping. [WAVE4_SOLPRO | AFFIRMATIVE-LOCK | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

### 2.5 Exact Wave4 floors and phase gauge

Define
[
F_{n,Theta}(t)=inf_{	hetainTheta}inf_{binmathcal B_{NR}(n)}[2+2ell_b+t e_	heta(b)].
]
On (lambda=ho/2), every baseline has (M_bge F_{n,Theta}(ho)) and (2L_bge F_{n,Theta}(ho)). For a zero-error candidate, (T(a)=max{M(a),2L(a)}). If (F(ho)ge T(a)), floor-derived margins are (gamma_M=F(ho)-M(a)) and (gamma_L=F(ho)/2-L(a)). [WAVE4_SOLPRO | W4-PHASE-MASTER | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

The exact supported pairs and resulting floors are:

- Cap Q4, pairs ((0,80),(16,48),(32,28),(64,0)):
  [
  F_{4,cap}(t)=min{2+t/2, 4+3t/10, 6+7t/40, 10},
  ]
  with breakpoints (10,16,160/7). [WAVE4_SOLPRO | W4-FLOOR-Q4-CAP | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
- Lower-capped Q4, pairs ((0,40),(16,22),(32,12),(64,0)):
  [
  F_{4,downarrow}(t)=min{2+t/2, 4+11t/40, 6+3t/20, 10},
  ]
  with breakpoints (80/9,16,80/3). [WAVE4_SOLPRO | W4-FLOOR-Q4-DOWN | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
- Uniform Q4, pairs ((0,32),(16,20),(32,12),(42,8),(64,0)):
  [
  F_{4,unif}(t)=min{2+t/2, 4+5t/16, 6+3t/16, 29/4+t/8, 10},
  ]
  with breakpoints (32/3,16,20,22). [WAVE4_SOLPRO | W4-FLOOR-Q4-UNIFORM | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
- Lower-capped Q3, pairs ((0,60),(8,30),(15,16),(24,0)):
  [
  F_{3,downarrow}(t)=min{2+t/2, 4+t/4, 23/4+2t/15, 8},
  ]
  with breakpoints (8,15,135/8). [WAVE4_SOLPRO | W4-FLOOR-Q3-DOWN | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

For the standard opaque candidate (q=0,c_0+c_1=1), (T(h)=6+2h). The exact Q4 threshold inverses are
[
ho^star_{cap}(h)=
egin{cases}
8+4h&0le hle1/2,\
rac{20}{3}(1+h)&1/2le hle7/5,\
rac{80h}{7}&7/5le hle2,\
+infty&h>2,
end{cases}
]
[
ho^star_{downarrow}(h)=
egin{cases}
8+4h&0le hle2/9,\
rac{80}{11}(1+h)&2/9le hle6/5,\
rac{40h}{3}&6/5le hle2,\
+infty&h>2.
end{cases}
]
At (h=1), these are (40/3) and (160/11); at (ho=40), both floors equal 10 and the candidate margins are ((gamma_M,gamma_D,gamma_L)=(5,0,1)). [WAVE4_SOLPRO | W4-PHASE-Q4-H / W4-AFF-Q4-40 / W4-AFF-Q4-EXPANDED | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

## 3. Theorem inventory

### 3.1 Formal Core freeze inventory

The Core file is an inventory, not a proof document.

| Family/ID | Locked claim shape | Audit status / dependency | Citation |
|---|---|---|---|
| RACC-PUBLIC | Visible capsule, exact refs, recovery-adjusted objective, never-wrong-bytes | Normative product reference; source doc is not in audited root | [CORE_FREEZE | RACC-PUBLIC | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| RACC-DISTILL | Research distill | Non-shipping notes; source doc absent | [CORE_FREEZE | RACC-DISTILL | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-DP / W4-FLOOR | Exact subset-tree DP and piecewise (F_4) | Working base; depends on final Wave4 DR+EC package; no runnable Wave4 checker in root | [CORE_FREEZE | W4-DP / W4-FLOOR | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-PHASE | Linked-slice phase thresholds | Working base; depends on exact floors and declared gauge | [CORE_FREEZE | W4-PHASE | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-AFF-Q4-40 | Candidate beats no-recovery hull at ((40,20)) with margins | Working base; final Wave4 table says DR+EC | [CORE_FREEZE | W4-AFF-Q4-40 | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-Qn | Lower-capped (nge3) extensions | Working base, declared-gauge only; exact general-(n) no-recovery phase remains unclaimed | [CORE_FREEZE | W4-Qn | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-DA-RATE | Opaque exact-ref rate vs full (n)-bit no-recovery | Working base; singleton and unrestricted-retrieval scopes differ | [CORE_FREEZE | W4-DA-RATE | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-OPAQUE-CAS-ALIAS / DIRECT-HASH-KILL | Visible hash is not opaque; two-level alias to private CAS | Working base; depends on opacity and store hypotheses | [CORE_FREEZE | W4-OPAQUE-CAS-ALIAS / DIRECT-HASH-KILL | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-CORRIDOR | Handle/tokenizer/selector/latency parameters ((h,q,c)) | Formal corridor only, not production measurement | [CORE_FREEZE | W4-CORRIDOR | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W4-NEG-NR / NO-PENALTY-ROBUST | Negative constraints against overclaim | Working base; zero-error and full lossy-hull scopes must remain separate | [CORE_FREEZE | W4-NEG-NR / NO-PENALTY-ROBUST | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| MDC-FABLE vs MDC-KIMI | Distinct MDC objects | Explicitly not merged until a PROVED+EC reduction | [CORE_FREEZE | MDC-FABLE / MDC-KIMI | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |
| W5-SOL-MDC-Q4-FULL-18-19 | Full Q4 sequential prefix phase, (m_{crit}=18) | Highest-confidence freeze only after checker rerun; rerun passed in this audit | [CORE_FREEZE | W5-SOL-MDC-Q4-FULL-18-19 | `01_RADC_FORMAL_CORE_V1_FREEZE.md`] |

Explicitly **not frozen**: production global Pareto dominance, real-tokenizer (h_	au) without measurement, “99.9% always,” Fable/Kimi MDC identification, arbitrary real-agent (R_{ag}(D)), BP1 general (n), and arbitrary-(n) Cont-2. [CORE_FREEZE | NOT-FROZEN | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]

### 3.2 Continuation 1

| Theorem ID | Exact statement | Source status | Dependencies and audit verdict |
|---|---|---|---|
| W5-SOL-AGRD-THETA | For full-support (	heta) and (0le Dle1/2), (R_{ag,	heta}(D)=1-H_2(D)) | DR [M] | Conditional binary Fano + data processing; recovery sent after observing (S). Coherent within ISC/binary lock. [SOLPRO_W5_CONT1 | W5-SOL-AGRD-THETA | `17_SOLPRO_W5_CONT1.md`] |
| W5-SOL-AGRD-WATERFILL | (R_{NR,	heta}(D)=minsum_i f(d_i)), (sum_i	heta_i d_ile D); unique (mu>0), (d_i(mu)=1/(1+2^{mu	heta_i})), (D(mu)=sum_i	heta_i d_i(mu)) | DR+EC [M] | KKT/strict convexity and full support. No separate Cont-1 checker is included in the mandatory set. [SOLPRO_W5_CONT1 | W5-SOL-AGRD-WATERFILL | `17_SOLPRO_W5_CONT1.md`] |
| W5-SOL-AGRD-THETA-CORRIDOR | (G_	heta) strictly decreases from (n-1) to 0; if (s=h+q+c<n-1), unique (D_	heta^starin(0,1/2)) with (G_	heta(D_	heta^star)=s); strict same-distortion memory/latency gain for (D<D^star) | DR [M] | Depends on AGRD-THETA + WATERFILL derivative. Source line 156 has a malformed form-feed in the displayed (rac), but the intended inequality and earlier formula are recoverable. [SOLPRO_W5_CONT1 | W5-SOL-AGRD-THETA-CORRIDOR | `17_SOLPRO_W5_CONT1.md`] |
| W5-SOL-OCCUPANCY-SCHUR | (P_{0,m}(	heta)=2^{-n}sum_{Bsubseteq[n]}	heta(B)^m) is symmetric convex, hence Schur-convex, for integer (mge1) | DR [F] | Uniform (X), fixed no-message prototype, iid demand; finite sum of convex powers. [SOLPRO_W5_CONT1 | W5-SOL-OCCUPANCY-SCHUR | `17_SOLPRO_W5_CONT1.md`] |
| W5-SOL-MDC-NOMSG-18/19 | Over full Q4 cap/down classes, parity beats all no-message baselines for (mle18); a no-message baseline beats parity for (mge19) | DR+EC [F] | Depends on occupancy Schur extrema and parity ledger. The written implication “(P_{0,m}) decreases, hence margin positive for all (mle18)” is insufficient because (39-2m) also changes. Superseded/repaired by Cont-2 NOMSG-REPAIR. It never covers nontrivial prefixes. [SOLPRO_W5_CONT1 | W5-SOL-MDC-NOMSG-18/19 | `17_SOLPRO_W5_CONT1.md`] |

### 3.3 Continuation 2

| Theorem ID | Exact statement | Source status | Dependencies and audit verdict |
|---|---|---|---|
| W5-SOL-COVERAGE-LEAF | With (r) nonempty prefix leaves, (P_Tle1-p_{cov}(	heta,m)(1-r/16)) | DR [F] | Full-coordinate coverage lets each transcript leaf contain at most one successful source word. Analytic lemma, not directly proved by checker. [SOLPRO_W5_CONT2 | W5-SOL-COVERAGE-LEAF | `10_SOLPRO_W5_CONT2.md`] |
| W5-SOL-Q4-LENGTH-SPECTRUM | Minimum external path spectrum (C_{16}(r)=(0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64)) for (r=1,ldots,16) | DR+EC [S] | Prefix/Kraft subset-split DP; Python and independent C++ reproduce exactly. [SOLPRO_W5_CONT2 | W5-SOL-Q4-LENGTH-SPECTRUM | `10_SOLPRO_W5_CONT2.md`] |
| W5-SOL-Q4-NONTRIVIAL-BARRIER | Every deterministic or randomized nontrivial Q4 prefix policy satisfies (M_T-M_{par}ge1) for (10le mle18) over both polytopes | DR+EC [F] | COVERAGE-LEAF + LENGTH-SPECTRUM + (p_{10}=6560848/9765625); all ((m,r)in[10,18]	imes[2,6]) checked and (rge7) follows from (ellge2). [SOLPRO_W5_CONT2 | W5-SOL-Q4-NONTRIVIAL-BARRIER | `10_SOLPRO_W5_CONT2.md`] |
| W5-SOL-Q4-NOMSG-REPAIR | (P_{0,m}=2^{-4}sum_B	heta(B)^m); exact worst-case endpoints and (gamma_{0,m}>gamma_{0,m+1}) for (10le mle17); minimum strip margin is at (m=18) | DR+EC [S] | Repairs Cont-1 via eight exact monotonicity certificates and Schur-convex extrema ((2,1,1,1)/5), ((3,3,2,2)/10). [SOLPRO_W5_CONT2 | W5-SOL-Q4-NOMSG-REPAIR | `10_SOLPRO_W5_CONT2.md`] |
| W5-SOL-MDC-Q4-FULL-18/19 | At ((40,20)), parity ((3m+2,0,4)) strictly dominates the complete randomized variable-length no-recovery prefix hull uniformly over either registered Q4 polytope iff (1le mle18) | DR+EC [M] | (mle9): prior exact one-demand floor; (10le mle18): nontrivial barrier + repaired no-message face; latency uses (F_Theta(40)=10); (mge19): no-message obstruction. Arithmetic and grid EC reproduced. [SOLPRO_W5_CONT2 | W5-SOL-MDC-Q4-FULL-18/19 | `10_SOLPRO_W5_CONT2.md`] |

The exact demand phase is (mathcal D^{seq}_{parity}={minmathbb N:mle18}). At (m=18), sharp (M)-margins are
[
gamma_M(Theta_4^downarrow)=277615146191/762939453125,
quad
gamma_M(Theta_4^{cap})=20074685943080277/50000000000000000,
]
with (gamma_L=1), (gamma_D=0). At (m=19), a valid no-message baseline has (M_0-M_{par}le-3/2). [SOLPRO_W5_CONT2 | W5-SOL-MDC-Q4-FULL-18/19 | `10_SOLPRO_W5_CONT2.md`]

### 3.4 Wave4 final merged-survivor inventory

The table below uses the **later final index** in the concatenated AI-export file, not the earlier embedded index that labels rows simply `PROVED`.

| Theorem ID | Final status | Final-index statement | Main dependency/scope |
|---|---|---|---|
| W4-DP-Q4 | DR+EC | Exact subset-prefix-tree recurrence for Q4 | Integer subset recurrence; source supplies replication brief, not runnable checker. [WAVE4_SOLPRO | W4-DP-Q4 | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-FLOOR-Q4-CAP | DR+EC | Exact four-piece floor on (Theta_4^{cap}) | DP-Q4 + cap supported pairs/vertex. [WAVE4_SOLPRO | W4-FLOOR-Q4-CAP | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-FLOOR-Q4-DOWN | DR+EC | Exact four-piece floor on (Theta_4^downarrow) | DP-Q4 + lower-capped supported pairs/vertex. [WAVE4_SOLPRO | W4-FLOOR-Q4-DOWN | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-FLOOR-Q4-UNIFORM | DR+EC | Exact five-piece uniform-Q4 floor | DP-Q4 + uniform supported pairs. [WAVE4_SOLPRO | W4-FLOOR-Q4-UNIFORM | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-FLOOR-Q3-DOWN | DR+EC | Exact four-piece floor on (Theta_3^downarrow) | Exact Q3 subset recurrence and vertex. [WAVE4_SOLPRO | W4-FLOOR-Q3-DOWN | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-PHASE-MASTER | DR | Linked-gauge dominance reduces to one scalar floor | Exact floor, (lambda=ho/2), candidate target (T). [WAVE4_SOLPRO | W4-PHASE-MASTER | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-PHASE-Q4-H | DR+EC | Exact ((ho,h,	ext{demand class})) Q4 phase | PHASE-MASTER + cap/down floors + opaque ledger. [WAVE4_SOLPRO | W4-PHASE-Q4-H | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-AFF-Q4-40 | DR+EC | Sharp cap-polytope margins ((5,0,1)) | Registered ((40,20)), ((h,q,c_0,c_1)=(1,0,1/2,1/2)), cap floor. [WAVE4_SOLPRO | W4-AFF-Q4-40 | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-AFF-Q4-EXPANDED | DR+EC | Declared-gauge result on larger (Theta_4^downarrow) | Same gauge/candidate + down floor. [WAVE4_SOLPRO | W4-AFF-Q4-EXPANDED | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-ZE-GORDIAN | DR | Penalty-robust all-(n) theorem for zero-error no-recovery policies | Zero-error-only class; does not imply full lossy-hull robustness. [WAVE4_SOLPRO | W4-ZE-GORDIAN | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-Qn-FANO | PI+DR | Kimi/Jensen lower-bound family, correctly scoped | Published Fano/Jensen input; relaxation, not exact attainable floor. [WAVE4_SOLPRO | W4-Qn-FANO | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-Qn-SEPARABLE | PI+DR | Stronger coordinate-separable lower-bound family | Binary Fano + conditional subadditivity; (ellge n-sum_i h_2(e_i)). [WAVE4_SOLPRO | W4-Qn-SEPARABLE | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-Qn-3PLUS | DR+EC | Declared-gauge affirmative for every (nge3) | Correctly scoped Fano/separable bounds + finite certificates; not an exact general-(n) phase. [WAVE4_SOLPRO | W4-Qn-3PLUS | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-Q5-ANTIPODAL | DR+EC | Exact (n=5) one-bit counterpolicy and phase kill | Explicit antipodal policy; optional 496-codebook enumeration is bounded only. [WAVE4_SOLPRO | W4-Q5-ANTIPODAL | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-DETERMINATION-FLOOR | DR | Exact singleton recovery floor (1-Delta_	heta(H)) | Visible-state support fibers and coordinate determination. [WAVE4_SOLPRO | W4-DETERMINATION-FLOOR | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-EXTREMAL-NONUNIQUE | DR | Infinite parity-alias family ties opaque EDC | Exact-reference mapping + alias family; kills path uniqueness, not ledger bound. [WAVE4_SOLPRO | W4-EXTREMAL-NONUNIQUE | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-EXTREMAL-KILL | DR | Coordinate-determining alias family strictly improves EDC | DETERMINATION-FLOOR + valid exact-reference alias. [WAVE4_SOLPRO | W4-EXTREMAL-KILL | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-LINEAR-ALIAS-RANK | DR | Exact batch recovery rate (dimpi_Q(ker A)) | Binary linear syndrome model. [WAVE4_SOLPRO | W4-LINEAR-ALIAS-RANK | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-BATCH-PARITY-KILL | DR | Rank-((n-1)) alias ties singleton and beats every nontrivial batch | LINEAR-ALIAS-RANK; valid only in the linear/private-reference class. [WAVE4_SOLPRO | W4-BATCH-PARITY-KILL | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-ALIAS-CAPACITY | DR | (K2^rle N_	au(h)) necessary for rank-(r) multiplexed aliases | Finite visible-tokenizer cardinality. [WAVE4_SOLPRO | W4-ALIAS-CAPACITY | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-PROD-CORRIDOR-DELTA | DR | Leakage-aware exact Q4 production corridor under locked store hypotheses | (Delta)-profile + alias capacity + tokenizer/latency/store assumptions; still not measured production. [WAVE4_SOLPRO | W4-PROD-CORRIDOR-DELTA | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-NO-PENALTY-ROBUST | DR | Full lossy-hull dominance impossible for all nonnegative gauges | Explicit lossy/no-message obstructions; no conflict with zero-error-only ZE-GORDIAN. [WAVE4_SOLPRO | W4-NO-PENALTY-ROBUST | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-NEG-NR-n | DR | No no-reference policy strictly dominates identity | Correct direction is nonexistence of a strict dominator, not identity dominating every lossy policy. [WAVE4_SOLPRO | W4-NEG-NR-n | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-DA-RATE | DR | Opaque singleton rate (n) versus 2; unrestricted retrieval rate 1 | Singleton and batch/retrieval scopes must not be conflated. [WAVE4_SOLPRO | W4-DA-RATE | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-DIRECT-HASH-KILL | DR | Direct content hash invalidates opacity certificate | Hash reveals source-dependent visible information. [WAVE4_SOLPRO | W4-DIRECT-HASH-KILL | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-OPAQUE-CAS-ALIAS | DR | Random visible alias can coexist with private CAS hash | Two-level alias/private-store construction; pinning/survival assumptions remain load-bearing. [WAVE4_SOLPRO | W4-OPAQUE-CAS-ALIAS | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-GEO-Q4 | DR+EC | Complete hull, Tchebycheff, and hypervolume witnesses | At ((40,20)), baseline front ({(10,0,5)}); candidate ((5,0,4)); (T=5.10<10.16), (HV=12>1), (Delta HV=11). [WAVE4_SOLPRO | W4-GEO-Q4 | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |
| W4-PEER-REPAIRS | DR+EC | Fable/Gemini counterexample repairs remain frozen | Correct unsupported-point, FIFO/LRU, adaptive-INDEX, and hypervolume scopes. [WAVE4_SOLPRO | W4-PEER-REPAIRS | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`] |

## 4. Checker commands and key integers

### 4.1 Canonical commands

The in-pack commands are:

```bash
cd substrate/cont2
python3 W5_FULL_PREFIX_CHECKS.py

g++ -std=c++20 -O2 -Wall -Wextra -pedantic \
  w5_full_prefix_check.cpp -o w5_full_prefix_check
./w5_full_prefix_check

g++ -std=c++20 -O2 -Wall -Wextra -pedantic \
  sol_m_demand_grid.cpp -o sol_m_demand_grid
for w in "4 4 4 8" "4 4 5 7" "4 4 6 6" "4 5 5 6" "5 5 5 5"; do
  for m in $(seq 10 18); do
    ./sol_m_demand_grid 4 "$m" "$((m+1))" 40 1 $w
  done
done

shasum -a 256 -c SHA256SUMS.txt
```

To preserve source read-only state, this audit copied the three checker sources to `/tmp`, compiled there, regenerated outputs, and byte-compared them to the packaged outputs. All comparisons passed. [SOLPRO_W5_CONT2 | EC-COMMANDS | `11_SOLPRO_W5_CONT2_README.md`]; [SOLPRO_W5_CONT2 | RUN-ALL | `substrate/cont2/RUN_ALL.sh`]

### 4.2 Reproduced integers

- (C_{16}(r)): `0 16 18 21 24 28 32 36 40 45 50 53 56 60 62 64`.
- (p_{10}=6560848/9765625approx0.6718308352).
- (B_2=10769686/1953125), (B_3=97023471/15625000), (B_4=252888283/31250000), (B_5=38966203/3906250), (B_6=20384017/1562500); all exceed 1.
- No-message endpoint margins: down17 (71088276063/30517578125); down18 (277615146191/762939453125); cap17 (475055717444931/200000000000000); cap18 (20074685943080277/50000000000000000).
- Universal no-message obstruction starts at (m=19), with gap (-3/2).
- Grid: 5 denominator-20 orbit representatives times 9 demand counts = 45 exact runs. Every recorded optimum has `Ltot 0`, `leaves 1`, and `splits 21457825`. Regenerated `Q4_GRID20_FULL_DP.out` matched byte-for-byte. [SOLPRO_W5_CONT2 | W5-SOL-Q4-LENGTH-SPECTRUM / W5-SOL-MDC-Q4-FULL-18/19 | `15_SOLPRO_W5_CONT2_CHECKS.out`; `substrate/cont2/w5_full_prefix_check.out`; `substrate/cont2/Q4_GRID20_FULL_DP.out`]

Wave4's replication brief supplies supported-pair integers and thresholds (135/8,64/5,40/3,160/11), but the audited root contains no Wave4 `.py`, `.cpp`, or other executable checker. Those EC labels therefore remain source claims in this audit. [WAVE4_SOLPRO | W4-DP-Q4 / W4-PHASE-Q4-H | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

## 5. Conflicts, gaps, and severity

1. **HIGH -- Wave4 EC not independently reproducible.** The mandatory Wave4 artifact is a 7,184-line AI Exporter transcript containing prompts, duplicated package material, PDF page breaks, and final claims. No Wave4 checker source exists anywhere under the audited root. Do not promote the entire Core to independently attested status from this bundle. [WAVE4_SOLPRO | W4-DP-Q4 and all Wave4 EC rows | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
2. **HIGH -- arbitrary-(n) Cont-2 and BP1 remain open.** Core explicitly excludes arbitrary-(n) Cont-2 and BP1 general (n). The Q4 EC cannot be extrapolated. [CORE_FREEZE | NOT-FROZEN | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]; [METHOD_GROK_MATRIX | BP1 | `71_W5_GROK_CONFLICT_MATRIX.md`]
3. **HIGH if merged -- MDC-FABLE and MDC-KIMI are different objects.** Fable uses sequential (M=9-p_c), (p_c=sum_i	heta_i^2), and (n_{crit}=5); Kimi distinguishes batch ((5,0,4)) from sequential ((8,0,4)). Keep dual-track IDs until a reduction is proved and checked. [METHOD_GROK_MATRIX | MDC-FABLE-* / MDC-KIMI-* | `71_W5_GROK_CONFLICT_MATRIX.md`]
4. **MEDIUM -- Cont-1 standalone proof gap.** The monotonicity of (P_{0,m}) does not itself establish monotonic positivity of (39-2m-40P_{0,m}). Cont-2 repairs only after exact strip inequalities. [SOLPRO_W5_CONT1 | W5-SOL-MDC-NOMSG-18/19 | `17_SOLPRO_W5_CONT1.md`]; [SOLPRO_W5_CONT2 | W5-SOL-Q4-NOMSG-REPAIR | `10_SOLPRO_W5_CONT2.md`]
5. **MEDIUM -- theorem ID is unstable.** The Cont-2 theorem/index uses `W5-SOL-MDC-Q4-FULL-18/19`; Core and bead material use `W5-SOL-MDC-Q4-FULL-18-19`. These should be treated as aliases, not two theorems. [SOLPRO_W5_CONT2 | W5-SOL-MDC-Q4-FULL-18/19 | `10_SOLPRO_W5_CONT2.md`]; [CORE_FREEZE | W5-SOL-MDC-Q4-FULL-18-19 | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]
6. **MEDIUM -- stale Qwen provenance.** Mandatory files declare QWEN_W6 `NOT_IN_ZIP`/`NOT_IN_TREE`, but `60_QWEN_W6_PROVENANCE.txt`, `61_QWEN_W6_PACKAGE.md`, and four files under `peers/QWEN_W6/` are present. [WAVE7_CAMPAIGN | QWEN_W6 | `02_WAVE7_THEORY_CAMPAIGN.md`]; [WAVE7_MANIFEST | QWEN_W6 | `00_WAVE7_OPERATOR_MANIFEST.md`]
7. **MEDIUM -- duplicate Wave4 status layers.** An earlier embedded theorem index uses `PROVED`; the later final merged-survivor index uses PI/DR/EC. This audit uses the later, more precise index. Consumers searching the first occurrence can overstate status. [WAVE4_SOLPRO | THEOREM-INDEX | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]
8. **LOW -- source corruption.** Cont-1 line 156 contains a form-feed where `\frac` was intended: `>\f rac1/(...)`. The earlier exact water-fill formula is intact, but the proof display is not clean source text. [SOLPRO_W5_CONT1 | W5-SOL-AGRD-THETA-CORRIDOR | `17_SOLPRO_W5_CONT1.md`]
9. **LOW -- flat checksum UX.** `16_SOLPRO_W5_CONT2_SHA256.txt` names the nested canonical files, not flat names 10--15, so `shasum -c` must run in `substrate/cont2`. Hashes show the flat mirrors are byte-identical where applicable. [SOLPRO_W5_CONT2 | SHA256 | `16_SOLPRO_W5_CONT2_SHA256.txt`; `substrate/cont2/SHA256SUMS.txt`]
10. **INFORMATIONAL -- method claims remain unverified.** File 70's July-2026/X episodes and file 72's Omega/FrankenSim transfers are explicitly workflow examples and speculative targets. They cannot upgrade theorem status. [METHOD_AI_MATH | METHOD-ONLY | `70_ADJACENT_MATH_AI_PROOF_METHODS.md`]; [METHOD_OMEGA_FRANKENSIM | W5+ Ω1--Ω5 | `72_OMEGA_FRANKENSIM_MATH_TRANSFER.md`]

Additional frozen negative boundaries from Wave4: literal EDC path uniqueness is dead; EDC optimality outside opacity is dead; global opacity is not the exact singleton condition; direct-content-hash opacity is dead; no penalty-independent theorem covers the full lossy hull; no exact general-(n) no-recovery phase is claimed; (n=5) bounded enumeration is not a complete prefix theorem; singleton rate does not transfer unchanged to batches; typed failure is not (D=0); pinning remains load-bearing; no production TokenZero or Lean artifact is certified. [WAVE4_SOLPRO | W4-EXTREMAL-NONUNIQUE / W4-DIRECT-HASH-KILL / W4-NO-PENALTY-ROBUST | `18_WAVE4_SOLPRO_PACKAGE_FULL.txt`]

## 6. Methods audit

- **Required workflow:** bound -> claimed extremal -> kill uniqueness/existence -> infinite family or phase -> obstruction map -> exact/optional formal certificate -> honest scope. [METHOD_AI_MATH | DESIGN-LOOP | `70_ADJACENT_MATH_AI_PROOF_METHODS.md`]
- **Forbidden inference patterns:** finite search failure is not nonexistence; Q4 exact work is not all (n); local conditions do not imply global nonvanishing; renaming a hard step does not remove it; an unchecked announcement is not a theorem. [METHOD_AI_MATH | ANTI-PATTERNS | `70_ADJACENT_MATH_AI_PROOF_METHODS.md`]
- **Conflict merge rule:** freeze only high-agreement islands after EC; never single-label MDC; recompute kill thresholds from exact floors; keep Grok residue as optional strengthener. [METHOD_GROK_MATRIX | MERGE-POLICY | `71_W5_GROK_CONFLICT_MATRIX.md`]
- **Exact core vs metric layer:** Kraft/DP/MI statements remain separate from tokenizer (h_	au,q_	au), measured latency, and store semantics. [METHOD_OMEGA_FRANKENSIM | PATTERN-1 | `72_OMEGA_FRANKENSIM_MATH_TRANSFER.md`]
- **Prove targets, not results:** agency RD, conservation ledger, tropical expand geometry, dual-weighted adaptivity, and certify-or-escalate are proposed W5+ Ω1--Ω5 targets. [METHOD_OMEGA_FRANKENSIM | W5+ Ω1--Ω5 | `72_OMEGA_FRANKENSIM_MATH_TRANSFER.md`]

## 7. NOT_IN_ZIP / presence audit

### Explicit marker

- `QWEN_W6 | NOT_IN_ZIP unless present` is **stale/false for this extracted root**. Present: `60_QWEN_W6_PROVENANCE.txt`, `61_QWEN_W6_PACKAGE.md`, `peers/QWEN_W6/00_PROVENANCE.txt`, `RADC_WAVE6_PACKAGE.md`, `SHA256SUMS.txt`, and `radc-wave6-qwen.md`. [WAVE7_CAMPAIGN | QWEN_W6 | `02_WAVE7_THEORY_CAMPAIGN.md`]

### Referenced original-layout material not found under the audited flat root

- `docs/racc-public.md` and `docs/RACC_RESEARCH_DISTILL.md`: **NOT_IN_ZIP/root**; no flat equivalents identified. [CORE_FREEZE | RACC-PUBLIC / RACC-DISTILL | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]
- `wave5-returns/{FABLE,KIMI,GROK_DEEP_RESEARCH}/`: **NOT_IN_ZIP/root**; therefore the Core's W5 peer-island inventory cannot be independently checked from mandatory substrate. [CORE_FREEZE | FABLE_W5 / KIMI_W5 / GROK residue | `01_RADC_FORMAL_CORE_V1_FREEZE.md`]
- `freeze/RADC_FORMAL_CORE_V1_FREEZE.md`: original path absent, but byte-content is represented by flat `01_RADC_FORMAL_CORE_V1_FREEZE.md`.
- `wave5-returns/SOLPRO/cont2/`: original path absent, but fully mirrored by `substrate/cont2/` and flat 10--16.
- `wave5-returns/SOLPRO/cont1/` and bare `RADC_W5_SOLPRO_CONTINUATION_1.md`: original paths absent; flat `17_SOLPRO_W5_CONT1.md` is present; `substrate/cont1/` is empty.
- `sources/wave4/WAVE4_SOLPRO_PACKAGE_FULL.txt`: original path absent; flat `18_WAVE4_SOLPRO_PACKAGE_FULL.txt` is present.

## 8. Complete read ledger

All entries below were read completely, in this exact order. Columns are `path | lines | bytes | SHA-256 | read`.

```text
00_WAVE7_OPERATOR_MANIFEST.md | 140 | 4771 | 139b78a450f7cad37ef4cdf57306601196a3e86fb33a89f7ba8669bdfd8017a6 | COMPLETE
01_RADC_FORMAL_CORE_V1_FREEZE.md | 123 | 4774 | 8a2df5541d66f97a584b16a9fe01a8846e79d2884d1a667049bd97d59cd4791f | COMPLETE
02_WAVE7_THEORY_CAMPAIGN.md | 125 | 4349 | e90f01170c89ea1ae5e1d92088a1f782ddaf1ac7322f7db3d10087e0cedf3644 | COMPLETE
03_README_OPEN_FIRST.txt | 25 | 822 | 863f154f054a5a45d06af69dd35d75e343b781b67b7deb8061bd669807d70b4a | COMPLETE
10_SOLPRO_W5_CONT2.md | 962 | 22399 | 1c3547cdea89823e95b3bb2d89c2c65496bc5d4e5930ffb1b384b50853a87f08 | COMPLETE
11_SOLPRO_W5_CONT2_README.md | 24 | 752 | f66b087f6c8fa47c8b739507ea785faf17dd0e23eb74992db835ff93e4d34716 | COMPLETE
12_SOLPRO_W5_CONT2_CHECKS.py | 125 | 4409 | d3b0c08ee339aa85eee646a81bb49b1741429cc89add2db2fd70638a13e3f337 | COMPLETE
13_SOLPRO_W5_CONT2_CHECKS.cpp | 153 | 5288 | 4db033816fa6d5fb49e7e1376296681f29290192e9d9150457661a8e3ab2e14c | COMPLETE
14_SOLPRO_W5_CONT2_GRID.cpp | 45 | 2625 | 1ffc1243fe353adbcd1e3e421bd6725c221c81204cf17234e8167949fdbd88e6 | COMPLETE
15_SOLPRO_W5_CONT2_CHECKS.out | 16 | 810 | 0ad17b44095afbb814d1195bc4e943ed7f0b16af417607bc5ed41f96572c6463 | COMPLETE
16_SOLPRO_W5_CONT2_SHA256.txt | 9 | 808 | a100266a9ee1a2dd23016a5118c9d18741fe938ff57e412be65e50e05aa5800a | COMPLETE
17_SOLPRO_W5_CONT1.md | 364 | 10022 | c4b0b25470c1a73e22ae095ad8aa09841655d0661358512c4d661e95dc775d32 | COMPLETE
18_WAVE4_SOLPRO_PACKAGE_FULL.txt | 7184 | 259978 | bdca56260c513780ff9fa60c7e003044a6db3cd81704fa97d9f879d35717f09d | COMPLETE
substrate/cont2/Q4_GRID20_FULL_DP.out | 50 | 5100 | d65f7e0a58b7c96899b3525573368f0903d5784b900a394ae96bc4cefecacdaf | COMPLETE
substrate/cont2/RADC_W5_SOLPRO_CONTINUATION_2.md | 962 | 22399 | 1c3547cdea89823e95b3bb2d89c2c65496bc5d4e5930ffb1b384b50853a87f08 | COMPLETE
substrate/cont2/README_CONTINUATION_2.md | 24 | 752 | f66b087f6c8fa47c8b739507ea785faf17dd0e23eb74992db835ff93e4d34716 | COMPLETE
substrate/cont2/RUN_ALL.sh | 15 | 785 | fd1f244917d1374555614cc0629a32a367f2ddee230b65f3446ae1c06573c31b | COMPLETE
substrate/cont2/SHA256SUMS.txt | 9 | 808 | a100266a9ee1a2dd23016a5118c9d18741fe938ff57e412be65e50e05aa5800a | COMPLETE
substrate/cont2/W5_FULL_PREFIX_CHECKS.out | 16 | 810 | 0ad17b44095afbb814d1195bc4e943ed7f0b16af417607bc5ed41f96572c6463 | COMPLETE
substrate/cont2/W5_FULL_PREFIX_CHECKS.py | 125 | 4409 | d3b0c08ee339aa85eee646a81bb49b1741429cc89add2db2fd70638a13e3f337 | COMPLETE
substrate/cont2/sol_m_demand_grid.cpp | 45 | 2625 | 1ffc1243fe353adbcd1e3e421bd6725c221c81204cf17234e8167949fdbd88e6 | COMPLETE
substrate/cont2/w5_full_prefix_check.cpp | 153 | 5288 | 4db033816fa6d5fb49e7e1376296681f29290192e9d9150457661a8e3ab2e14c | COMPLETE
substrate/cont2/w5_full_prefix_check.out | 13 | 399 | 28699fe84c87348cdcb60209a0f5d0ab2aee97b4a9de3bfae9b235ea68d6222a | COMPLETE
70_ADJACENT_MATH_AI_PROOF_METHODS.md | 296 | 22490 | ec23c862e5feebccab94e5c8c456450b71c51ea5f0043400bbb1f87e600b3924 | COMPLETE
71_W5_GROK_CONFLICT_MATRIX.md | 25 | 3379 | 912a55d76e578953e57264f9055d493fe85202cc796a8e9cd912c6384ecbc6cc | COMPLETE
72_OMEGA_FRANKENSIM_MATH_TRANSFER.md | 210 | 11366 | efee103ed7546f89cb78fe660263febcd583153fc1b750c75aafe3cbcaaf744b | COMPLETE
```

## 9. Residual risk

- The Cont-2 **arithmetic certificates** are attested, but the continuum theorem also relies on human-readable analytic lemmas (coverage-leaf, convex/Schur extrema, affine randomization). This audit checked their statement/dependency consistency, not a proof assistant formalization.
- Wave4, Cont-1 WATERFILL EC, Core peer islands, and referenced product docs are not independently attested by runnable artifacts in the mandatory/root material.
- No Wave-6 peer theorem was audited beyond mandatory metadata; Qwen presence was checked only to resolve the contradictory `NOT_IN_ZIP` marker.
- Production TokenZero claims remain outside the frozen theorem scope.