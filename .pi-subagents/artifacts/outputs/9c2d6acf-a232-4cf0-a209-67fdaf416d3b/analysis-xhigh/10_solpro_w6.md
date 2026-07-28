# Wave 7 xhigh read-only audit: SOLPRO_W6

Source audited: /Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT

Audit scope: flat files 20 through 28 and every regular file under peers/SOLPRO_W6. No source file was changed. The only written file is this required audit artifact.

## Executive verdict

**Qualified pass on the mathematics; package attestation is incomplete.** I found no mathematical blocker in the exact m_crit staircase, Q4 full-prefix tail completeness, occupancy/prefix lemmas, block-Fano argument, agency reduction, phase-polar algebra, or BP1 route obstruction. The supplied Python certificate executes and reproduces its recorded output byte-for-byte. The Cont-2 Python certificate also reproduces its recorded output byte-for-byte.

The strongest adverse result is packaging, not a counterexample: the theory and SHA manifest claim an independent W6 C++20 checker, but W6_THEORY_CHECKS.cpp is absent from both the audited peer return and the entire flat root. Its recorded output cannot be inspected or reproduced. Several [EC] tags are also broader than the executable assertions actually supplied.

## Concrete findings

1. **HIGH -- independent W6 C++ certificate is not reproducible.** 21_SOLPRO_W6_THEORY.txt:2937-2969 claims an independent C++20 checker and artifact; 28_SOLPRO_W6_SHA256.txt:2 gives SHA-256 b3fb52... for /mnt/data/radc_wave6/W6_THEORY_CHECKS.cpp; 25_SOLPRO_W6_CHECKS_CPP.out and peers/SOLPRO_W6/reruns/W6_THEORY_CHECKS_CPP.out contain only claimed output. A complete-root find returned no W6_THEORY_CHECKS.cpp. Consequence: the advertised independent implementation is output-only evidence.
2. **MEDIUM -- [EC] coverage is materially narrower than the theorem labels.** 23_SOLPRO_W6_CHECKS.py:377-394 does not compute mutual information, R_q(D), an RD optimum, or a heterogeneous weighted objective; it only checks channel normalization, ranges, and the D_q(z) parametrization. No assertion in the checker names or evaluates W6-PHASE-POLAR-MASTER. The MDC section (lines 355-374) checks ledger inequalities and kernel enumeration, not either information-theoretic no-morphism proof. The BP1 section (397-404) checks only the norm bounds, not the almost-full-leaf ratio. The DR arguments remain independently plausible/sound, but the package should not treat EC as full theorem verification.
3. **MEDIUM -- the audited peer return is not dependency-self-contained.** The theorem index relies on frozen W4/W5/Core IDs and Cont-2. peers/SOLPRO_W6 contains none of those source statements. The two Cont-2 checker sources named in the SHA manifest are absent under peers/SOLPRO_W6; hash-identical renamed copies exist only outside the peer return as 12_SOLPRO_W5_CONT2_CHECKS.py and 13_SOLPRO_W5_CONT2_CHECKS.cpp. Therefore imported gauges/ledgers cannot be reconstructed from this peer directory alone.
4. **MEDIUM -- the SHA manifest does not attest the primary theorem artifacts.** 28_SOLPRO_W6_SHA256.txt covers the Python checker, missing W6 C++ source, outputs, and Cont-2 sources/outputs, but omits the primary PDF, TXT extract, provenance, and the manifest itself. Paths are stale absolute /mnt/data paths. Local hashes do match every supplied mapped entry.
5. **LOW -- two scope hypotheses should be explicit.** W6-MDC-OPAQUE-RANK-SEPARATION uses full support to infer p_c<1 and n-1>0, requiring n>=2; n=1 is not excluded in its statement. W6-AGTV-CONDITIONAL-RD's q-ary and heterogeneous formulas use log(q-1), requiring q and every q_s to be at least 2; singleton action alphabets are not split out. The main finite conditional-RD reduction itself remains valid.

## Theorem inventory

Canonical path below is 21_SOLPRO_W6_THEORY.txt; peers/SOLPRO_W6/RADC-Wave-6-Theory.txt is byte-identical. All listed authored statuses are those printed in the theory, not upgraded audit attestations.

### 1. W6-OCCUPANCY-TRANSVERSAL
- **Claim shape:** An r-leaf transcript has joint success at most E[min(1,r 2^-K_m)]; r=1 is exact; every nonincreasing occupancy functional is Schur-convex, with heavy-vertex and dimension extremizers.
- **Path:** 21_SOLPRO_W6_THEORY.txt:527-785.
- **Gauge:** X uniform on the binary n-cube; iid demands from theta; deterministic r-leaf realization, then conditional extension to randomized policies; lower-capped theta or Q4 cap for exact extremizers.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-SOL-MDC-NOMSG-18/19, W5-SOL-COVERAGE-LEAF.
- **Key integers:** heavy weights H=n+4, L=4, W=5n; Q4 cap weights (3,3,2,2); recurrence checked for 3<=n<=10 and 1<=m<=20.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py, occupancy and majorization sections.
- **Own verdict:** **ACCEPT.** The projection count, r=1 subset identity, pairwise T-transform proof, and padded-v_n majorization are coherent. EC checks recurrence, not Schur theory.

### 2. W6-PREFIX-SPECTRUM-N
- **Claim shape:** Gives the exact closed form C_N(r)=min_d[N d+V_d(r)] for the minimum weighted external path length of an r-leaf binary prefix partition of N equiprobable states.
- **Path:** 21_SOLPRO_W6_THEORY.txt:786-915.
- **Gauge:** Binary complete prefix trees; 1<=r<=N; positive integer leaf masses summing to N.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-SOL-Q4-LENGTH-SPECTRUM.
- **Key integers:** C_16=(0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64); independent root-split DP agreement for every 1<=r<=N<=64.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py.
- **Own verdict:** **ACCEPT.** The minimum-depth decomposition and discrete convex balancing cover every complete binary prefix-tree shape; the independent DP is meaningful.

### 3. W6-FULLPREFIX-CERT-SURFACE
- **Claim shape:** Combines exact prefix length, occupancy success, and block Fano into computable sufficient dominance surfaces in arbitrary (n,m,Theta,rho,lambda), with a pure rational projection-length surface.
- **Path:** 21_SOLPRO_W6_THEORY.txt:916-1182.
- **Gauge:** Complete randomized variable-length no-recovery prefix hull; ledgers M=(m+1)(1+ell)+rho e_m, L=1+ell+c_comp+lambda e_m, D=e_m; 1<=r<=2^n.
- **Authored status:** [DR|EC].
- **Dependencies:** W4-Qn-SEPARABLE, W5-SOL-COVERAGE-LEAF, plus W6-OCCUPANCY-TRANSVERSAL and W6-PREFIX-SPECTRUM-N.
- **Key integers/formulas:** c_r=C_(2^n)(r)/2^n; b_r=1-u_r; k=min(n,m); O(nm) heavy-coordinate occupancy recurrence; thresholds (5.21)-(5.23).
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py, PL_threshold and block-Fano sections.
- **Own verdict:** **ACCEPT AS A CERTIFICATE, NOT AN EXACT GENERAL PHASE.** The theory correctly admits that the lower envelope may be non-tight away from closed regimes.

### 4. W6-Q4-UNLINKED-TAIL-RECTANGLE
- **Claim shape:** For Q4 lower-capped or Q4 capped demands and every m>=8, parity dominates the entire randomized prefix hull iff rho>=(2m+1)/(1-p_Theta(m)) and lambda>=3/(1-p_Theta(m)); r=1 is the active obstruction.
- **Path:** 21_SOLPRO_W6_THEORY.txt:1183-1515.
- **Gauge:** n=4; Theta in {Theta_down_4, Theta_cap_4}; arbitrary unlinked nonnegative rho,lambda; m>=8; complete variable-length no-recovery prefix hull.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-SOL-MDC-Q4-FULL-18/19 and W6-FULLPREFIX-CERT-SURFACE.
- **Key integers:** down m=18 rho=141143798828125/3563296863977, lambda=11444091796875/3563296863977; down m=19 rho=595092773437500/14263650502901; cap m=18 rho=74000000000000000000/1870074685943080277; cap m=19 rho=156000000000000000000/3742207147564718513. Tail coverage constants 15/28 and 75/104; finite exceptions down m=8,9,10 and cap m=8,9.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py; for inherited 18/19 regression, python3 12_SOLPRO_W5_CONT2_CHECKS.py.
- **Own verdict:** **ACCEPT.** All r=1,...,16 are covered: symbolic tail bounds plus exact finite exceptions; mixtures follow by seed conditioning and affine ledgers. Necessity is attained by the no-message leaf. No prefix-tree completeness gap found.

### 5. W6-BLOCK-FANO-BARRIER
- **Claim shape:** At rho=40, every nontrivial prefix realization has M>3m+2 for n>=4 and 2<=m<=19.
- **Path:** 21_SOLPRO_W6_THEORY.txt:1516-1672.
- **Gauge:** Lower-capped class via its heavy vertex; n>=4; r>=2; 2<=m<=19; memory branch only; rho=40.
- **Authored status:** [DR|EC].
- **Dependencies:** internal occupancy/block-Fano machinery; no external dependency printed in the index row.
- **Key integers:** small-m lower certificates 16159/102400, 15561/8000, 14957/4000; finite-sweep minimum at n=m=19 is 331725854346589385191559240189443183/794428636916437084448554992675781250, about 0.417565328.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py.
- **Own verdict:** **ACCEPT.** The m=2,3,4 entropy-conjugate bounds and m=5,...,19 rational split are algebraically consistent; n>m is reduced by dimension monotonicity.

### 6. W6-SEQ-DOWN-STAIRCASE
- **Claim shape:** At (rho,lambda)=(40,20), the exact largest dominated demand count is m_crit(2)=0, m_crit(3)=16, m_crit(4)=18, m_crit(5)=18, and m_crit(n)=19 for n>=6.
- **Path:** 21_SOLPRO_W6_THEORY.txt:1673-1949.
- **Gauge:** Lower-capped Theta_down_n; complete randomized variable-length no-recovery prefix hull; registered three-objective ledger; n>=2.
- **Authored status:** [DR|EC].
- **Dependencies:** W4-Qn-3PLUS, W4-FLOOR-Q3-DOWN, Cont-2, W6-BLOCK-FANO-BARRIER.
- **Key integers:** first failures 1,17,19,19,20. Exact endpoint gaps: n3 m16 =845049722020265693/437893890380859375 and m17 =-22519522704133297/437893890380859375; n4 m18 =277615146191/762939453125 and m19 =-1227337666073/762939453125; n5 m18 =887975035189461090631639/582076609134674072265625 and m19 negative; n6 m19 =2975301311635846283/19705225067138671875 and m20 negative.
- **EC commands:** python3 23_SOLPRO_W6_CHECKS.py; python3 12_SOLPRO_W5_CONT2_CHECKS.py.
- **Own verdict:** **ACCEPT.** Nontrivial trees, r=1, n=3 finite cases, n=2 identity, latency, dimension tail, and failures beyond the endpoint are all separately routed. No missing r class or monotonicity leap found.

### 7. W6-MDC-OPAQUE-RANK-SEPARATION
- **Claim shape:** Fable and Kimi are Pareto-incomparable in (M,D,L,I_pre), and neither can be obtained from the other by the explicitly defined opacity/rank-preserving postprocessing morphisms.
- **Path:** 21_SOLPRO_W6_THEORY.txt:1950-2164.
- **Gauge:** Two demands; full-support theta; binary uniform source; objectives (M,D,L,I_pre); morphisms may only postprocess an existing visible handle with public source-independent randomness and may not increase residual rank.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-MDC-FABLE, W5-MDC-SEQ, W4-LINEAR-ALIAS-RANK.
- **Key integers:** F=(9-p_c,0,11/2-(3/2)p_c,0); K=(8,0,4,n-1); unique one-dimensional full-support binary kernel span(1^n); enumeration n=2,...,20.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py, lines 355-374.
- **Own verdict:** **ACCEPT CONDITIONALLY.** Pareto algebra and the two information/rank contradictions are sound for n>=2. This is not an unrestricted permanent no-reduction theorem; it is exactly as strong as the newly locked morphism definition. Imported Fable/Kimi policy ledgers are NOT_IN_ZIP and were not independently reconstructed here.

### 8. W6-AGTV-CONDITIONAL-RD
- **Claim shape:** Finite decision-TV agency RD equals conditional Hamming RD of the correct action; uniform q-ary actions have the standard q-ary formula and heterogeneous uniform action sets admit one-parameter water filling.
- **Path:** 21_SOLPRO_W6_THEORY.txt:2165-2399.
- **Gauge:** Finite X,S; S independent of X; pre-demand Z generated without S; post-demand R; deterministic correct action A*=a(X,S); information-only rate; decision-TV distortion. The displayed q formulas require q,q_s>=2.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-SOL-ISC-HULL, W5-SOL-AGENCY-RD.
- **Key integers/formulas:** R_q(D)=log2(q)-H_2(D)-D log2(q-1) to D=1-1/q; q=2 gives 1-H_2(D); supplied loop spans q=2,...,16.
- **EC commands:** python3 23_SOLPRO_W6_CHECKS.py; independent in-memory microcheck described below.
- **Own verdict:** **ACCEPT DR, DOWNGRADE SUPPLIED EC TO PARTIAL.** Sampling Q proves the converse and setting R=A proves achievability. Direct independent mutual-information and equal-slope checks passed. Singleton q scope needs a trivial separate clause.

### 9. W6-PHASE-POLAR-MASTER
- **Claim shape:** Defines a substrate-indexed polar of scalar floors and maps finite-prefix, ISC, sequential M, sequential L, and linked phases into that common algebra.
- **Path:** 21_SOLPRO_W6_THEORY.txt:2400-2684.
- **Gauge:** Nondecreasing scalar floors; finite affine lower envelopes where (5.56) is used; separate pre, ISC, and sequential substrates; linked rho=2 lambda where stated.
- **Authored status:** [PI|DR|EC].
- **Dependencies:** W5-SOL-PHASE-POLAR, W5-SOL-ISC-PHASE, plus W6 Q4/staircase rows.
- **Key integers:** finite-prefix s=2 values 16,135/8,64/5,40/3,160/11; ISC finite iff s<n-1; registered staircase 0,16,18,18,19.
- **EC command:** no supplied checker assertion; independent numeric inversion of (5.57) passed for n=3,...,11 sample points.
- **Own verdict:** **ACCEPT AS ALGEBRA/BOOKKEEPING; [EC] NOT ATTESTED.** It does not close arbitrary finite-prefix phases and does not claim to; the obstruction map correctly says those remain partially closed.

### 10. W6-BP1-LEAF-ENTROPY-OBSTRUCTION
- **Claim shape:** At theta=v_n and every n>=3, an almost-full leaf violates the pointwise self-information tangent with the optimal one-bit gain, ruling out that proof route but not BP1.
- **Path:** 21_SOLPRO_W6_THEORY.txt:2685-2871.
- **Gauge:** One-demand BP1; lower-capped heavy vertex; leaf A=the n-cube minus one point; deterministic one-bit encoder benchmark.
- **Authored status:** [DR|EC].
- **Dependencies:** W5-BP1-RED, W5-SOL-BP1-RESTRICTED.
- **Key integers:** R_n>ln(2)/2>1/3; s_1<=sqrt((n+24)/(25n))/2<=3/10; supplied norm check n=3,...,256.
- **EC command:** python3 23_SOLPRO_W6_CHECKS.py; independent ratio/norm check at n=3,4,10,64,256.
- **Own verdict:** **ACCEPT.** The Fourier/Parseval bound and almost-full-leaf ratio establish only a local charging obstruction. BP1 itself remains open, as the theory explicitly says.

## Cited Core-ID dependency register

These are every principal Core ID printed in the W6 theorem index. Their paths and authored statuses are **NOT_IN_ZIP** for peers/SOLPRO_W6; only their roles can be inventoried from W6's own index/proof references.

| Core ID | One-line role/claim shape visible in W6 | Gauge/integer visible here | Audit verdict |
|---|---|---|---|
| W5-SOL-MDC-NOMSG-18/19 | Prior no-message Q4 18/19 obstruction | m=18/19, registered gauge | Dependency only; NOT_IN_ZIP |
| W5-SOL-COVERAGE-LEAF | Prior leaf coverage/success bound | prefix leaves | Dependency only; NOT_IN_ZIP |
| W5-SOL-Q4-LENGTH-SPECTRUM | Exact Q4 C_16 spectrum | 16 states, 16 values | Reproduced by new checker; source NOT_IN_ZIP |
| W4-Qn-SEPARABLE | General-n one-demand separable floor | rho=40 use | Analytic use plausible; source NOT_IN_ZIP |
| W5-SOL-MDC-Q4-FULL-18/19 | Prior exact Q4 complete-prefix registered transition | (40,20), m=18/19 | Python/Cont-2 regression passes; source NOT_IN_ZIP |
| W4-Qn-3PLUS | Prior Q_n, n>=3 fragment used by staircase | general n | Dependency only; NOT_IN_ZIP |
| W4-FLOOR-Q3-DOWN | Exact Q3 lower-capped scalar floor | pairs (0,60),(8,30),(15,16),(24,0); seams 8,15,135/8 | Independently rederived in W6 Python; source NOT_IN_ZIP |
| W5-MDC-FABLE | Fable opaque exact-reference policy/ledger | F ledger in theorem 5.7 | Imported policy not reconstructible here |
| W5-MDC-SEQ | Kimi sequential parity/complement policy/ledger | K=(8,0,4,n-1) | Imported policy not reconstructible here |
| W4-LINEAR-ALIAS-RANK | Binary linear alias/rank semantics | rank n-1 and residual rank one | Algebra audited; source NOT_IN_ZIP |
| W5-SOL-ISC-HULL | Prior ISC achievable/lower hull | psi scalar floor | Dependency only; NOT_IN_ZIP |
| W5-SOL-AGENCY-RD | Binary agency RD | 1-H_2(D) | Recovered as q=2; source NOT_IN_ZIP |
| W5-SOL-PHASE-POLAR | Prior phase-polar notation | scalar polar | Dependency only; NOT_IN_ZIP |
| W5-SOL-ISC-PHASE | Prior ISC phase root | finite iff s<n-1 | Numeric formula sampled; source NOT_IN_ZIP |
| W5-BP1-RED | Prior BP1 reduction | arbitrary prefix remains open | Dependency only; NOT_IN_ZIP |
| W5-SOL-BP1-RESTRICTED | Proved restricted BP1 fragments | coordinate/fixed-rank classes | Dependency only; NOT_IN_ZIP |

Cont-2 is also cited as a dependency but is not a theorem ID in this index. Its Python checker hash matches the outer flat renamed copy and its output reproduces.

## Independent scrutiny of requested focal points

### m_crit
The staircase proof is complete at its locked gauge. For n>=4 and m<=19, W6-BLOCK-FANO-BARRIER excludes every r>=2 tree. The r=1 leaf is decided by exact G_0 signs. n>=6 reduces to n=6 by padded-heavy majorization. n=3 uses an independently enumerated Q3 floor for m=1,2 and exact projection thresholds through m=16. n=2 fails by the identity baseline L=3<4. Explicit no-message policies fail every later m. Verdict: **exact values 0,16,18,18,19 accepted**.

### Full prefix-tree completeness
The exact Q4 tail does not enumerate tree topologies, but it does not need to: C_16(r) is topology-minimal for each leaf count, occupancy bounds success for that leaf count, r=1 is proved the largest threshold, and r runs through 1,...,16. Conditioning on a complete random seed covers randomized policies. The general surface remains only sufficient away from Q4 tail/registered staircase, as correctly disclosed. Verdict: **Q4 tail and registered staircase complete; arbitrary general surface not exact**.

### MDC
The four-objective separation is mathematically elementary once the imported ledgers are granted. Both no-morphism directions are valid only under the package's explicit postprocessing/opacity/residual-rank definition. That is a conditional separation, not a universal category-independent impossibility. Verdict: **accepted under locked semantics; dependency and EC attestation incomplete**.

### BP1
The almost-full leaf has advantage 1/[2(2^n-1)] and self-information log2(2^n/(2^n-1)); their ratio exceeds ln(2)/2. Parseval bounds the best one-bit slope below 3/10. This kills leafwise tangent charging for every n>=3, but says nothing negative about a global tree potential. Verdict: **obstruction accepted; BP1 remains open**.

### Agency
The soft-to-sampled-action converse and R=A achievability identify the information-only problem with conditional Hamming RD. q-ary symmetry and KKT water filling are standard and independently sampled here. The package must add q_s>=2 or a q_s=1 convention, and its supplied EC is weak. Verdict: **DR accepted, EC partial**.

### Phase table
The polar scaling is correct: sequential M uses physical factor (m+1)/2 and target 6-2/(m+1); L uses one-half of phi_F(8); the linked slice takes the maximum after converting both to rho. ISC inversion (5.57) numerically returns 4+2s and has finite rho exactly for s<n-1. Verdict: **algebra accepted; it is a substrate-indexed dictionary, not a new exact all-regime phase theorem**.

## EC commands and observed results

Run from the flat source root.

1. Supplied W6 Python certificate:

    python3 23_SOLPRO_W6_CHECKS.py

   **PASS.** Final line: ALL W6 THEORY CHECKS PASS.

2. Exact recorded-output reproduction:

    cmp <(python3 23_SOLPRO_W6_CHECKS.py) 24_SOLPRO_W6_CHECKS.out
    cmp <(python3 12_SOLPRO_W5_CONT2_CHECKS.py) 26_SOLPRO_W6_CONT2_PY_RERUN.out

   **PASS.** Both comparisons returned zero.

3. PDF completeness:

    pdfinfo 22_SOLPRO_W6_THEORY.pdf
    pdftotext -layout 22_SOLPRO_W6_THEORY.pdf -

   **PASS.** 35 pages, unencrypted; extracted bytes=123101 and exactly equal 21_SOLPRO_W6_THEORY.txt. Peer PDF/TXT gives the same exact result.

4. Independent in-memory agency/phase/BP1 probes:

    python3 - <<'PY'  # direct q-ary mutual information, KKT slopes, ISC inversion, BP1 ratios

   **PASS.** agency maximum numerical error 8.88e-16; water-fill slope spread 4.44e-16; sampled ISC inversions and BP1 inequalities passed.

5. W6 independent C++:

    c++ -std=c++20 -O2 W6_THEORY_CHECKS.cpp -o /tmp/w6checks && /tmp/w6checks

   **NOT RUN: NOT_IN_ZIP.** The source does not exist anywhere under the flat root.

## Complete file-read ledger

Each row represents two byte-identical files. Direct reads were full-file. The PDF was fully converted with pdftotext -layout and exactly matched the fully read TXT. Thus all 18 requested files were consumed completely.

| Flat path | Peer path | Bytes each | SHA-256 | Read evidence |
|---|---|---:|---|---|
| 20_SOLPRO_W6_PROVENANCE.txt | peers/SOLPRO_W6/00_PROVENANCE.txt | 382 | 75b96f37d87c2c2c8c380d64e6a3349c6d4e09398fed49a3e5285a070ee47324 | full direct read |
| 21_SOLPRO_W6_THEORY.txt | peers/SOLPRO_W6/RADC-Wave-6-Theory.txt | 123101 | bbabd3b9481095f0eef0b4456cb52c7717efda2f70e2b3f9cbd28858f3844bef | lines 1-3120 read in complete contiguous chunks |
| 22_SOLPRO_W6_THEORY.pdf | peers/SOLPRO_W6/RADC-Wave-6-Theory.pdf | 319648 | 4638787eacb0335cef13fea812c32c7db214cd58afbe51f5f2a00ea681687690 | 35 pages; complete extracted text exact to TXT |
| 23_SOLPRO_W6_CHECKS.py | peers/SOLPRO_W6/checkers/W6_THEORY_CHECKS.py | 14929 | dc3a329e4445a0bbb6b72bbbb58dc1051f400acd78c8a2f6855858ddfdb04c22 | lines 1-406 full read and executed |
| 24_SOLPRO_W6_CHECKS.out | peers/SOLPRO_W6/reruns/W6_THEORY_CHECKS.out | 2906 | 019524ad110b3702a3c999bcf773d99bea1a8d3dd5c27147a216c07ff81ebc48 | full direct read; reproduced exactly |
| 25_SOLPRO_W6_CHECKS_CPP.out | peers/SOLPRO_W6/reruns/W6_THEORY_CHECKS_CPP.out | 299 | ed474de94288416ab36cfea6285b2ca00fc2ef0f3a52019176a960c18fd40c22 | full direct read; source absent |
| 26_SOLPRO_W6_CONT2_PY_RERUN.out | peers/SOLPRO_W6/reruns/CONT2_PY_RERUN.out | 810 | 0ad17b44095afbb814d1195bc4e943ed7f0b16af417607bc5ed41f96572c6463 | full direct read; reproduced exactly |
| 27_SOLPRO_W6_CONT2_CPP_RERUN.out | peers/SOLPRO_W6/reruns/CONT2_CPP_RERUN.out | 399 | 28699fe84c87348cdcb60209a0f5d0ab2aee97b4a9de3bfae9b235ea68d6222a | full direct read |
| 28_SOLPRO_W6_SHA256.txt | peers/SOLPRO_W6/W6_THEORY_SHA256.txt | 911 | 251f01b287a2b9e216a23658ed02b2a0606dedc8cafaff63ded439f306fd6c34 | full direct read; mapped hashes checked |

## NOT_IN_ZIP

For the audited peers/SOLPRO_W6 return:

- **W6_THEORY_CHECKS.cpp -- absent everywhere in the flat root.** Manifest hash exists; source does not.
- **12_SOLPRO_CONT2_CHECKS.py and 13_SOLPRO_CONT2_CHECKS.cpp -- absent from the peer return.** Equivalent hash-matching outer-flat files are renamed 12_SOLPRO_W5_CONT2_CHECKS.py and 13_SOLPRO_W5_CONT2_CHECKS.cpp.
- **Frozen substrate documents and all 16 cited W4/W5 theorem sources -- absent from the peer return.** The W6 TXT asserts they were read, but this return cannot independently prove that provenance.
- **Primary PDF/TXT/provenance hashes -- absent from W6_THEORY_SHA256.txt**, although the files themselves are present and flat/peer pairs are byte-identical.

## Residual risks

- Imported Core/Fable/Kimi statements were outside the mandated full-read set, so this audit accepts only the W6 package's paraphrases of their ledgers.
- The missing W6 C++ source prevents independent implementation review and rerun.
- Supplied EC should be interpreted as regression evidence for selected integer identities, not machine verification of every DR claim.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Five severity-ranked findings cite concrete paths/lines; all ten W6 theorem IDs have claim, path, gauge, status, dependencies, integers, audit verdict, and EC command; residual risks and NOT_IN_ZIP are explicit."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/9c2d6acf-a232-4cf0-a209-67fdaf416d3b/analysis-xhigh/10_solpro_w6.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "python3 23_SOLPRO_W6_CHECKS.py",
      "result": "passed",
      "summary": "All supplied W6 Python assertions passed."
    },
    {
      "command": "cmp <(python3 23_SOLPRO_W6_CHECKS.py) 24_SOLPRO_W6_CHECKS.out; cmp <(python3 12_SOLPRO_W5_CONT2_CHECKS.py) 26_SOLPRO_W6_CONT2_PY_RERUN.out",
      "result": "passed",
      "summary": "Both recorded Python outputs reproduced byte-for-byte."
    },
    {
      "command": "pdfinfo 22_SOLPRO_W6_THEORY.pdf; pdftotext -layout 22_SOLPRO_W6_THEORY.pdf -",
      "result": "passed",
      "summary": "35-page unencrypted PDF extracted to exactly the 123101-byte TXT; peer copy matched."
    },
    {
      "command": "shasum -a 256 ./2[0-8]* peers/SOLPRO_W6/**",
      "result": "passed",
      "summary": "All nine flat/peer pairs are byte-identical and all supplied mapped manifest hashes match."
    },
    {
      "command": "independent in-memory q-ary RD, water-fill KKT, ISC inversion, and BP1 ratio probes",
      "result": "passed",
      "summary": "Maximum q-ary formula error 8.88e-16; KKT slope spread 4.44e-16; sampled ISC/BP1 checks passed."
    },
    {
      "command": "c++ -std=c++20 -O2 W6_THEORY_CHECKS.cpp -o /tmp/w6checks && /tmp/w6checks",
      "result": "not-run",
      "summary": "W6_THEORY_CHECKS.cpp is NOT_IN_ZIP and absent from the entire flat root."
    }
  ],
  "validationOutput": [
    "ALL W6 THEORY CHECKS PASS",
    "RECORDED_OUTPUTS_REPRODUCE=PASS",
    "PDF_TEXT exact=True for flat and peer copies",
    "INDEPENDENT_MICROCHECKS=PASS agency_maxerr=8.88e-16 slope_spread=4.44e-16",
    "Complete-root search found no W6_THEORY_CHECKS.cpp"
  ],
  "residualRisks": [
    "Missing W6 C++ source prevents independent C++ attestation.",
    "Cited W4/W5/Fable/Kimi theorem sources are not in the audited peer return.",
    "Several EC tags exceed the assertions actually implemented in the supplied Python checker."
  ],
  "noStagedFiles": true,
  "diffSummary": "Read-only audit: no source changes; wrote only the required Markdown findings artifact.",
  "reviewFindings": [
    "high: 28_SOLPRO_W6_SHA256.txt:2 and 21_SOLPRO_W6_THEORY.txt:2937 - claimed independent W6 C++ checker source is absent, so its output is not reproducible.",
    "medium: 23_SOLPRO_W6_CHECKS.py:355-404 - EC coverage is partial for MDC, agency, phase-polar, and BP1 despite theorem EC tags.",
    "medium: peers/SOLPRO_W6 - frozen W4/W5/Core dependencies and Cont-2 sources are not self-contained in the peer return.",
    "medium: 28_SOLPRO_W6_SHA256.txt - manifest omits primary PDF/TXT/provenance and uses stale absolute paths.",
    "low: 21_SOLPRO_W6_THEORY.txt:1950-2399 - MDC needs n>=2 and q-ary agency formulas need q_s>=2 stated explicitly."
  ],
  "manualNotes": "Qualified mathematical pass. Exact m_crit and Q4 full-prefix tail were independently scrutinized without finding a counterexample; package reproducibility and EC scope remain the material concerns."
}
```
