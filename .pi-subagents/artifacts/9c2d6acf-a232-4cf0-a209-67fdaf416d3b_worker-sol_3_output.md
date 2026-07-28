# Wave 7 xhigh read-only audit: KIMIK3_THINKING W6

## Outcome

**Verdict: useful peer package, not attested for freeze as written.** Exact arithmetic for the principal no-message faces, spectra, MDC ledgers, agency crossover, and BP1 table reproduces. Five blocking or material gaps remain: the A3/A9 strict-dominance edge is false, permanent MDC non-reduction lacks a formal reduction category, the claimed U1 master table is absent from the KIMIK3 corpus, B5 overclaims its finite enumeration, and the EC/reproduction manifest is incomplete.

Source files were not edited. Only this audit artifact was written.

## P1-P5 decision table

| Target | Package contribution | Own verdict | Attestation |
|---|---|---|---|
| P1 general-n sequential full-prefix | G7 closes n=3 at mcrit=16; G8 gives n=5 [1,3] and [11,18], with [4,10] open; G9/G10 give n=4 rho/lambda faces | **PARTIAL, CONDITIONAL.** Own exact EC reproduces all printed G7/G8/G9 endpoint fractions. G7 full dominance still uses PI W4 Q3-down floors for m<=3 and latency; G8 uses the PI Q5 floor. No general-n law is proved. | Not freeze-attested as unconditional |
| P2 MDC rank stratification | M3-M10 give ledger separation and propose permanent dual track | **PARTIAL.** Ledger arithmetic and the separating cell reproduce. M4-M6 do not define a reduction category strongly enough to prove universal non-reduction. No residual-rank master theorem, leaf-floor coincidence table, or MDS-triviality statement is delivered. | Dual-track caution is sound; permanent-separation theorem is not attested |
| P3 BP1 | n<=4 subset-DP claim; n=5 optimal-root fragment; antipodal obstruction; t1 table n=2..15 | **PARTIAL.** B1/B4a/B4c algebra and the t1 fractions check. B5 proves existence of an infinite greedy obstruction but not its claimed global uniqueness. B6 lacks an all-n proof. B4b/B4d remain EC-only from artifacts outside the KIMIK3 folder. | Small-n claims plausible, not independently enumerated here |
| P4 agency RD / decision-TV | A4 hybrid threshold/frontier, A5 decision-TV variants, A7 phase map, A9 assembly | **MOSTLY SOUND, ONE FALSE EDGE.** A4a/A4c formulas and D-dagger reproduce. At n=2, s=h=q=c=0, D=1/2, A3/A9 have gammaM=gammaL=gammaD=0, contradicting the required strict margin. No lossy multi-demand D>0 theorem is supplied. | Repair boundary before freeze |
| P5 master phase surface | U1 is declared PROVED and repeatedly cited | **NOT ATTESTED.** No actual unified rho-star/mcrit/no-message/full-prefix/batch/sequential table or U1 proof occurs in flat 41 or peers/KIMIK3_THINKING. A checker name exists only in the DEEPSEEK collation area and was not content-inspected. | Fails P5 delivery in the audited corpus |

## Concrete findings

1. **HIGH -- A3/A9 are false at an admitted boundary.** In 41_KIMIK3_THINKING_W6_PACKAGE.md:317-319 and :381-383 the domain includes 0 <= s < n-1 and D <= D*. Take uniform n=2, h=q=c=s=0, D*=1/2. Then f(D*)=R_NR(D*)=0, so gammaM=gammaL=gammaD=0. This violates the statement-lock convention requiring at least one strict coordinate. Exclude (s,D)=(0,1/2), require q+2c+f(D*)>0, or weaken the dominance claim.
2. **HIGH -- M4/M5/M6 do not prove M10 permanent non-reduction.** At 41_KIMIK3_THINKING_W6_PACKAGE.md:267-285, “gauge-respecting reduction” is never formally defined. M4 assumes such a reduction preserves the expand-count distribution; M5 rules out only direct affine-ledger representation; M6 assumes theta-preserving reparameterization. These are useful invariants after a reduction category is fixed, not a universal impossibility theorem. C1-C6 establish distinct ledgers/scopes; C7-C8 do not close all reductions.
3. **HIGH -- U1/P5 deliverable is absent.** The index row at :215 and freeze line :489 claim a PROVED master table, but the document contains only separate snippets. No U1 theorem section or table gives the advertised joint columns. peers/KIMIK3_THINKING contains only provenance plus the duplicate package.
4. **MEDIUM -- B5 global uniqueness exceeds its certificate.** At :353-355 the package enumerates only subsets of sizes 2 through 4, 41,416 sets at n=5, then says density 1/2 occurs “nowhere else” over all subsets/classes. The analytic argument covers size 2 only. It does prove that antipodal pairs attain 1/2, which is enough to kill the proposed per-split greedy proof route; it does not exclude larger equality cases.
5. **MEDIUM -- B6 jumps from finite EC to all n.** At :357-359 it proves/checks e_anti>1/3 only for n=8..101 and monotonicity only for n=8..20, yet concludes rho_kill=12 for every n>=8. The limit e_anti->2/5 does not exclude a later dip. Own EC extends the inequality through n=1024, minimum margin 1/384 at n=8, but an analytic all-n bound is still missing.
6. **MEDIUM -- G7/G8 “PROVED” statuses remain PI-conditional.** G7 explicitly uses W4 Q3-down floor breakpoints for m<=3 and L at :241-248. G8 uses Fable Q5 rho_cert(5)<=18 at :251. The package discloses this, but the headline status should be “conditional on PI floors,” not an unconditional peer theorem.
7. **MEDIUM -- claimed EC inventory is incomplete.** At :403 the package says 31 captured .out files. The extracted directory contains 29 .out plus one O3_AUDIT_LOG.md. n5_optclass.out and n5_all16.out are absent although their sources support the corrected B4d claim. CONT1_CHECKS.py is absent, so the published E6 reproduction command cannot run from this extraction.
8. **MEDIUM -- authorship of evidence is mixed.** The text is byte-identical across flat 41, peers/KIMIK3_THINKING, and the duplicate under peers/DEEPSEEK_W6, and provenance explicitly labels it KIMIK3_THINKING. The same provenance labels swarm_lanes and checker/EC material under DEEPSEEK as DeepSeek-run material. Attribute the package text to KIMIK3_THINKING, but do not call the supporting checker corpus KIMIK3-owned without a per-file manifest.
9. **LOW -- G4 contains an unresolved numeric wording defect.** The theorem-index row at :178 says “barrier min refined 5.514 -> 26394686/1953125 = 13.514.” The exact fraction equals about 13.514; 5.514 is either the old value or a typo and should be labeled.

## Complete new-ID inventory

Verdict codes: **PASS** = analytic argument or own EC supports the scoped claim; **COND** = depends on PI or uninspected external EC; **GAP** = statement is broader than its proof; **FALSE-EDGE** = explicit counterexample; **INFO** = not a theorem.

### P1 / G series

| ID | Claim, path, gauge | Claimed status/tag | Dependencies and key integers | EC and own verdict |
|---|---|---|---|---|
| W6-DS-G1 | Coverage-leaf bound P_T <= 1-p_cov(1-r/2^n), all n, theta, randomized hull | PROVED / DR | Uniform X; covering demand pins at most one source per leaf; N=2^n | Self-contained proof. **PASS** |
| W6-DS-G2 | Prefix spectra C8,C16,C32 and C64[1..12]; ell>=2 threshold | PROVED / EC | Recurrence C_N(r); thresholds r=5,7,11 for N=8,16,32 | Independent DP reproduced every printed entry and thresholds. **PASS** |
| W6-DS-G3 | One-demand reduction M_T >= (m+1)F_Theta(80/(m+1))/2; n=3 needs F=8 at t>=135/8 | PROVED / DR | Gauge rho=40; imported floor F_Theta; W4 Q3-down breakpoint | Reduction shape plausible; floor is PI. **COND** |
| W6-DS-G4 | n=4 barrier for m=10..18; refined minimum 26394686/1953125 ~=13.514 | PROVED re-derived / EC | (rho,lambda)=(40,20), C16 and coverage bounds | g456 output claimed under DEEPSEEK, not inspected; wording has 5.514 typo. **COND** |
| W6-DS-G5 | n=4, m=18 sharp margins in down/cap classes | PROVED re-derived / EC | Registered gauge and peer Q4 floors; exact fractions are not printed in the theorem row/proof | No standalone proof or printed certificate in KIMIK3 corpus. **COND** |
| W6-DS-G6 | m>=19 obstruction, including -3/2 | PROVED re-derived / EC | Registered n=4 parity/no-message baseline | Prior substrate claim only; no standalone derivation here. **COND** |
| W6-DS-G7 | n=3 down: parity (3m+2,0,4) dominates full hull iff 1<=m<=16; gammaL=0 | PROVED / DR+EC, strip PI | theta vertex (7,4,4)/15; m16 margin 845049722020265693/437893890380859375; m17 negative -22519522704133297/437893890380859375; uniform m17 -218455/14348907; barrier B2(4)=6998/1125 | Own exact EC reproduces all no-message fractions. Barrier proof is coherent. Small-m and latency floors are PI. **COND**, not unconditional |
| W6-DS-G8 | n=5 down: dominance [1,3] and [11,18], failure m>=19, [4,10] open | PROVED fragment / DR+EC, PI at <=3 | vertex (9,4,4,4,4)/25; m18 +887975035189461090631639/582076609134674072265625; m19 -254541365995396231447867/582076609134674072265625; missing F5,down on (1600/121,18) | Own exact endpoint EC matches. Q5 floor remains PI and middle strip is honestly open. **COND/PARTIAL** |
| W6-DS-G9 | n=4 exact no-message mcrit(rho), barrier and full-phase rho thresholds | PROVED / DR+EC | rho 20,24,28,32,36,40,48,56,64,80 maps to mcrit 8,10,12,14,16,18,21,25,29,36; barrier 72479248046875/3157132488062; down full 141143798828125/3563296863977; cap full 74000000000000000000/1870074685943080277 | Own Fraction EC reproduces both tables and all three fractions. Full-hull conclusion retains imported floor/barrier assumptions. **PASS arithmetic / COND hull** |
| W6-DS-G10 | Lambda decouples: gammaL >= F(2lambda)/2-4; lambda*=rho*/2; n=3 ceiling 0 | PROVED / DR+EC | Q4d 80/11, Q4u 32/5, Q3d 135/16, Q3u 8; registered lambda=20 nonbinding | Algebra is sound given F. Exact thresholds depend on PI floor envelope. **COND** |
| W6-DS-G11 | Method is n-invariant but all numeric certificates are n-specific | PROVED / DR | Six dependency classes: spectrum, coverage, floor, saturation, phase, latency | Correct limitation statement, not a new phase theorem. **PASS** |
| W6-DS-G12 | Eight exact-arithmetic swarm practices | DONE / BE | No mathematical integer beyond “8 practices” | Engineering note, not theorem. **INFO** |

### P2 / MDC series

| ID | Claim, path, gauge | Claimed status/tag | Dependencies and key integers | EC and own verdict |
|---|---|---|---|---|
| W6-DS-M3 | One n=4 down-vertex sequential separating cell | PROVED / EC, margins PI | p_c=7/25; Fable (218/25,0,127/25); Kimi (8,0,4); identity (15,0,5); margins (7,0,1) use PI G2=15,H2=10 | Own exact ledger EC matches. Raw cell separation **PASS**; hull margin **COND** |
| W6-DS-M4 | No Fable-to-Kimi reduction because expand distributions differ | PROVED / DR+EC | Fable E[#exp]=2-p_c with positive two-expand mass; Kimi one expand a.s.; mean equality only p_c=1 | Invariant is asserted without formal reduction category. **GAP** |
| W6-DS-M5 | No Kimi-to-Fable reduction by p_c nonrepresentation | PROVED / DR+EC | Batch M=5 forces p_c=4; sequential M=8 or L=4 forces p_c=1 | Correct direct-family algebra; not a general reduction theorem. **GAP** |
| W6-DS-M6 | Probabilistic p_c(theta) versus algebraic rank r_A=1 prevents reduction | PROVED / DR+EC | 15 nonempty Q; p_c slopes -1,-3/2; Kimi constant | Distinguishes parameter laws under theta-preserving maps only. **GAP** as universal non-reduction |
| W6-DS-M7 | Shared carried-token accounting; M gap equals 1-p_c | PROVED lemma / EC | Fable M=9-p_c; Kimi seq M=8; batch M=5 | Own rational EC matches. **PASS** |
| W6-DS-M10 | Permanent dual-track separation, certificates C1-C8 | PROVED / DR+EC | Uniform C1 (35/4,0,41/8) vs (5,0,4)/(8,0,4); Eexp 7/4,43/25 vs 1; vertex L 127/25>5; C5-C8 depend M3-M6 | C1-C6 support distinct models and prudent separate labels. C7-C8 do not prove permanence without a reduction definition. **GAP** |

### P4 / Agency series

| ID | Claim, path, gauge | Claimed status/tag | Dependencies and key integers | EC and own verdict |
|---|---|---|---|---|
| W6-DS-A1 | Agency binary RD converse and all-theta strengthening R=1-H2(D) | PROVED audit / DR+EC | H1-H3; 400 random schemes claimed | Converse chain is sound. Equality also needs inherited achievability, not reproved here. **COND** |
| W6-DS-A2 | NR water filling, uniqueness, envelope, strict advantage | PROVED audit / DR+EC | d_i=1/(1+2^(mu theta_i)); 2000-grid/min G 1.62e-4 claimed | KKT/monotonicity proof is coherent for n>1, full support, interior D. **PASS analytic** |
| W6-DS-A3 | Corridor is closed D<=D*, with strict M at boundary | PROVED audit/addendum / DR+EC | G(D*)=s; gammaM=f(D*)+q+2c claimed >0 | Counterexample n=2,s=h=q=c=0,D*=1/2 yields 0. **FALSE-EDGE** |
| W6-DS-A4a | Hybrid sharp threshold rho*(D)=1+log2(1-D), D0*=1-2^(rho-1) | PROVED / DR+EC | EC errors 1.4e-7 and 1e-4 claimed | Convex chord proof and endpoints check. **PASS** |
| W6-DS-A4b | Pure latency charge rho=0 collapses rate to 0 | PROVED / DR | Coin-flip D0=1/2; expand probability 1-2D | Follows from A4a within the stated hybrid model. **PASS** |
| W6-DS-A4c | Model-H ledger frontier and unique D-dagger | PROVED / DR+EC | Delta margins H2(D)-2D(1+2h+q), H2(D)-2D(1+s); at (1,0,1), H2(D)=6D and D=0.041586864956... | Own numeric root residual is 0 at displayed precision. **PASS scoped** |
| W6-DS-A5a | k-action 0-1 agency RD is 1-H2(D), k>=2 | PROVED / DR+EC | Binary source; extra actions | Fano/data-processing argument supports stated formal model. **PASS scoped** |
| W6-DS-A5b | Soft-decision TV RD and two variants | PROVED / DR+EC | 1-H2(D); 1-H2(D/Delta); endpoint-free 1-H2(2(D-1/4)), D in [1/4,1/2] | Channel reduction and endpoint achievability are coherent. **PASS scoped** |
| W6-DS-A6 | AOT-6 correction: mixed alias interpolates I=beta n | PROVED correction / EC | n=2 examples beta=1/4,1/2,3/4 give I=1/2,1,3/2 | Direct construction proves correction. **PASS** |
| W6-DS-A7 | rho*(s)=max(4+4s,20s/3,80(s-1)/7) on s<=3 | PROVED audit / EC | seams s=3/2 ->10, 12/5->16; values 40/3,120/7,160/7 | Own arithmetic reproduces all branches/seams. Polar-floor identity is inherited. **PASS arithmetic / COND floor** |
| W6-DS-A8 | Class phase consistency and Q3d attribution gap | PROVED audit / EC | chain 64/5 <40/3<160/11<16<135/8; T=8; five vertices | Arithmetic is consistent; Q3d binding pair remains explicitly PI/open. **PASS as audit, not proof of PI input** |
| W6-DS-A9 | Assembled lossy multi-objective region plus exact D=0 m strips | PROVED assembly / DR | 0<=s<n-1; D<=D*; n=4 m<=18, n=3 m<=16; m>=2,D>0 open | Inherits A3 false corner and P1 PI floors. **FALSE-EDGE/COND** |
| W6-DS-A10 | Four agency obstruction one-liners | DONE / PI | “4” claimed; lines are not presented in theorem/proof section | Dependency note, not auditable theorem. **INFO/NOT ATTESTED** |

### P3 / BP1 series

| ID | Claim, path, gauge | Claimed status/tag | Dependencies and key integers | EC and own verdict |
|---|---|---|---|---|
| W6-DS-B1 | Fable/Kimi e_anti bridge identity | PROVED / DR+EC | e(n=3..8)=1/4,11/40,121/400,5/16,145/448,43/128 | Algebra and all listed fractions independently reproduce. **PASS** |
| W6-DS-B2 | Support functional is weighted-majority error; Hamming-nearest can be worse | PROVED precision / DR+EC | n=8: 93/256 and 23/64 vs 43/128; counts 2^(n-1) or n2^(n-1) | Max identity proof supports monotonicity; broad enumeration/count law is EC-only. **COND** |
| W6-DS-B3 | BP1 three-way equivalence and t1=inf 2ell/(1/2-e) | PROVED restatement / DR, PI reduction | t1=2/s1 | Displayed algebra is sound; full equivalence imported from Fable. **COND** |
| W6-DS-B4a | Deep-leaf ratio <=1/4 for min depth >=2 | PROVED / DR | G<=d2^(n-1), ell>=2 | Self-contained. **PASS** |
| W6-DS-B4b | Universal amortized tangent for all trees in five n<=4 classes | PROVED EC-complete / EC | subset DP over at most 16 sources; slope decrement 1/(d2^n) | Certificate method is valid, but run/output was not independently inspected. **COND** |
| W6-DS-B4c | Root-split sufficient condition R(B)+R(C)<=slack | PROVED / DR | Binding slack=0 requires excess-free sides | Algebra shown. **PASS conditional on definitions** |
| W6-DS-B4d | n=5 every optimal-root tree satisfies BP1 | PROVED / EC | E=242; 16 bipartitions, 32 sides; margin >=1/800; suboptimal roots remain open | Corrected source exists only under DEEPSEEK; two supplementary captures are absent. **NOT INDEPENDENTLY ATTESTED** |
| W6-DS-B5 | Density 1/2 attained exactly at antipodal pairs, six classes | PROVED / EC+DR | 41,416 sets of sizes <=4; second densities 11/30,1/3,2/5,2/5,3/8,21/50 | Antipodal attainment and infinite greedy obstruction **PASS**; “nowhere else” **GAP** |
| W6-DS-B6 | rho_kill=max(12,4/e_anti); equals 12 for all n>=8; limit 12 | PROVED / DR+EC | n7 145/448<1/3; n8 43/128>1/3 by 1/384; checked to101 | Own EC passes through 1024. No all-n inequality proof. **GAP for n>=8 universal; supported numerically** |
| W6-DS-B7 | Second F segment is one-bit line in five classes | PROVED / EC | breakpoints: cap 10,16,160/7; down 80/9,16,80/3; uniform 32/3,16,20,22; Q3d 8,15,135/8; Q3u 8,16 | Line-crossing arithmetic is consistent; floor envelope remains EC/PI. **COND** |
| W6-DS-B8 | BP1 verdict: n<=4 proved, n=5 fragment, general open; corrected t1 table | PROVED verdict / EC+DR | t1 n=2..15: 20/3,8,80/9,800/79,32/3,896/79,256/21,4608/371,12800/987,1408/105,2560/187,532480/37653,573440/39897,819200/55913 | Own exact e_anti calculation reproduces every fraction. Status correctly leaves general n open. **PASS table / COND small-n proofs** |

### P5 / unification and relock

| ID | Claim, path, gauge | Claimed status/tag | Dependencies and key integers | EC and own verdict |
|---|---|---|---|---|
| W6-DS-U1 | Master rho-star/m-phase/MDC dual-track table | PROVED table / DR+EC | Supposed parallel rows across n,s,Theta,timeline | Actual table/proof absent from KIMIK3 corpus. **NOT ATTESTED** |
| W6-DS-U3 | RACE half-spaces vs orthant are general vs collapsed gauge forms | PROVED resolution / DR | Gauge (40,20); baseline (10,0,5); A=2wM+wL, B=rho wM+wD+lambda wL | Algebraic specialization is coherent. **PASS scoped** |
| W6-DS-U4 | DLU ledger unique, paths nonunique; cone/radii | PROVED audit / EC | inverse h=M-L,q=2L-M-3; cone u/2<=v<=u; radii (sqrt2,1,2); M gap 1 | Coordinate algebra reproduces. Full policy uniqueness is inherited. **PASS algebra / COND model** |
| W6-DS-CONT2-RELOCK | Re-attest Q4 full-prefix arithmetic | PROVED re-attestation / EC | C16, p10=6560848/9765625, m17/18 margins, m19 -3/2; claimed 8/8,45/45,6/6 | Equivalent substrate files exist, but literal command tree is absent and CONT1_CHECKS.py is missing. Not rerun under this authorship-only restriction. **NOT INDEPENDENTLY ATTESTED** |

## Inherited theorem-ID dependencies

These are citations, not new W6-DS results, and were not re-proved in the audited KIMIK3 corpus:

| Cited ID | Use here | Own status |
|---|---|---|
| W5-SOL-MDC-Q4-FULL-18/19 | Registered Q4 m phase and obstruction | PI/claimed EC re-attestation |
| W5-SOL-AGRD-* | A1-A3 and A7 agency substrate | PI, DR-audited only |
| W5-MDC-FABLE-0..5, W5-MDC-FABLE-4/5 | Fable ledgers and hull floors | PI; checker claims only |
| W5-MDC-KIMI-*, W5-MDC-SEQ | Kimi parity ledgers/floors | PI; checker claims only |
| W5-ANTI-OPT, W5-LPP-* | BP1 one-bit and support-functional core | Partly bridged by B1/B2; general content PI |
| W5-BP1 | Three-way reduction | PI, restated by B3 |
| W5-Q5-SW | rho_cert(5)<=18 | PI; gates G8 |
| W4-DP/FLOOR/PHASE and W4-PHASE-Q4-H | Q3/Q4 floors, breakpoints, phase map | PI; not re-derived |
| W6-GROK-COV-LEAF-GEN | Peer equivalent of G1 | G1 independently proves the scoped bound |

## Independent EC performed

All calculations used Python stdlib Fraction or direct float bisection; no package checker content was consumed.

- G2: reproduced complete C8/C16/C32 and C64[1..12]; first C_N(r)>=2N is r=5,7,11,17 for N=8,16,32,64.
- G7/G8: reproduced every displayed exact m=15/16/17 and m=18/19 no-message margin, including uniform n=3 m=17.
- G9: reproduced both ten-entry mcrit tables, both full-phase threshold fractions, and the barrier fraction.
- M3/M10: reproduced all p_c=7/25 and 1/4 ledgers and expected expand counts.
- A4c/A7/B1/B6/B8: D-dagger=0.04158686495638442; all A7 seams; e_anti values; all t1 fractions; e_anti>1/3 through n=1024 with minimum 1/384 at n=8.

This EC checks arithmetic, not the universal full-prefix/subset-tree quantifiers for G7, G8, B4b, or B4d.

## Provenance and authorship

- SHA256 a16dd2fe3ea9967634690b2793106912376b288c7b3ef46d5061ccd07965d75c identifies all three package copies:
  - 41_KIMIK3_THINKING_W6_PACKAGE.md
  - peers/KIMIK3_THINKING/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md
  - peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md
- SHA256 6972da96809f31c7081f637cc307ed4de322ee8160fc164ea7e3328600425c2c identifies the identical provenance copies at peers/KIMIK3_THINKING/00_PROVENANCE.txt, peers/DEEPSEEK_W6/00_PROVENANCE.txt, and flat 40_DEEPSEEK_W6_PROVENANCE.txt.
- Provenance explicitly says KIMIK3_THINKING and DEEPSEEK_SWARM are distinct author labels. Therefore the reviewed document is **KIMIK3_THINKING-authored by bundle label**, even when duplicated under DEEPSEEK_W6 and even though its IDs use W6-DS.
- Evidence artifacts lack a per-file author manifest. The provenance calls DEEPSEEK swarm_lanes and checker/EC materials “from that run.” Treat package authorship as resolved, evidence authorship as mixed/unresolved.

## File-read ledger

| Path | Mode | Coverage/purpose |
|---|---|---|
| 41_KIMIK3_THINKING_W6_PACKAGE.md | FULL | All 507 lines / 69,082 bytes, chunk-read plus full-stream read |
| peers/KIMIK3_THINKING/00_PROVENANCE.txt | FULL | All 10 lines / 606 bytes |
| peers/KIMIK3_THINKING/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md | FULL | Full-stream read; byte-for-byte cmp and SHA match to flat 41 |
| peers/DEEPSEEK_W6/00_PROVENANCE.txt | FULL, authorship only | All 10 lines; identical provenance |
| peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md | FULL-by-identity, authorship only | SHA match to flat 41; no separate theorem audit |
| 40_DEEPSEEK_W6_PROVENANCE.txt | FULL, authorship only | All 10 lines; identical provenance |
| 02_WAVE7_THEORY_CAMPAIGN.md:40-75 | PARTIAL | Read only to lock P1-P5 definitions and fail gates |
| peers/DEEPSEEK_W6/checkers/**, ec_out/** | METADATA ONLY | Names/counts/existence for EC and NOT_IN_ZIP; no source/output content read |
| peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE.md and swarm_lanes/** | NOT READ | Excluded by authorship-only restriction |

peers/KIMIK3_THINKING contains exactly two files; both were fully accounted.

## NOT_IN_ZIP / path remaps

Literal references are evaluated against the extracted source root.

| Referenced item | Result |
|---|---|
| Pareto/wave6-returns/DEEPSEEK_W6/ | **NOT_IN_ZIP**; content is remapped to peers/DEEPSEEK_W6/ |
| .radc-pack/wave6/ec/ | **NOT_IN_ZIP**; partial Cont-2 counterpart is substrate/cont2/ |
| CONT1_CHECKS.py | **NOT_IN_ZIP** anywhere; E6 reproduction is blocked |
| 17_SOLPRO_CONT2_SHA256.txt | **NOT_IN_ZIP** under that name; apparent flat remap is 16_SOLPRO_W5_CONT2_SHA256.txt |
| 00_RADC_FORMAL_CORE_V1_FREEZE.md | **NOT_IN_ZIP** under that name; flat remap is 01_RADC_FORMAL_CORE_V1_FREEZE.md |
| n5_optclass.out | **NOT_IN_ZIP**; source n5_optclass.c exists under peers/DEEPSEEK_W6/checkers/tier5/ |
| n5_all16.out | **NOT_IN_ZIP**; source n5_all16.c exists under peers/DEEPSEEK_W6/checkers/tier5/ |
| Claimed “31 captured .out files” | **COUNT MISMATCH**: 29 .out plus one .md audit log are present |

## Residual risks

- This audit intentionally did not inspect or execute DEEPSEEK checker contents, per the authorship-only restriction. Universal EC claims remain un-attested.
- The inherited Formal Core, W4, and W5 theorem bodies were not part of the full-read set. Any row depending on them remains PI/conditional.
- Own EC validates exact arithmetic only. It is not a replacement for exhaustive policy/subset-tree enumeration.
- Package timestamps say 2026-07-27; this audit verifies bundle bytes and mathematics, not historical runtime identity.