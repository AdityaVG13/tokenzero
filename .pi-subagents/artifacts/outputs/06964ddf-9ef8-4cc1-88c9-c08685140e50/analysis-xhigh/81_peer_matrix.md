# Wave 7 xhigh peer matrix

## Integrator disposition

Read-only integration. All seven reports were read byte-completely: analysis/xhigh/00_substrate_methods.md, 10_solpro_w6.md, 20_kimi_w6.md, 30_kimik3_w6.md, 40_deepseek_w6.md, 50_grok_w6.md, and 60_qwen_w6.md. Their cited-source requirement was over-satisfied by byte-reading every regular file in wave7-attach-FLAT: 337 files, 26,255,300 bytes, aggregate path/content SHA-256 53ae2c74d72ba306f80ca569cda93e19deeef3600a151bce2ee34fffe856f2de.

Verdict lock used below: ACCEPT = analytic/scoped statement survives; RECHECK-PASS = exact computation or independent rerun survives; RECHECK-FAIL = stated claim/certificate has a concrete counterexample or failed check; OPEN = not established, conditional, partial, SB, or missing evidence; INEQUIVALENT = distinct formal objects, not a universal no-reduction result.

## Exhaustive deduplicated theorem/claim matrix

Rows are deduplicated by exact ID. Where the same W6-DS ID occurs in the KimiK3 return and an owned DeepSeek lane, one row names both provenances and both exact paths. Peer-defined family/object IDs with parentheses are retained. No numeric ID range is used.

| peer | exact theorem ID | one-line claim shape | exact bundle-relative path | own verdict | status/dependency caveat |
|---|---|---|---|---|---|
| SOLPRO_W6 | W6-OCCUPANCY-TRANSVERSAL | Bounds r-leaf success by the occupancy moment, with exact r=1 and Schur extremizers. | 21_SOLPRO_W6_THEORY.txt | ACCEPT | DR survives; supplied EC checks recurrence, not the Schur proof. |
| SOLPRO_W6 | W6-PREFIX-SPECTRUM-N | Gives the exact minimum weighted external path length C_N(r) for equiprobable binary prefix partitions. | 21_SOLPRO_W6_THEORY.txt | RECHECK-PASS | DR+EC; root-split DP agrees through N=64. |
| SOLPRO_W6 | W6-FULLPREFIX-CERT-SURFACE | Combines prefix length, occupancy, and block Fano into computable sufficient dominance surfaces. | 21_SOLPRO_W6_THEORY.txt | ACCEPT | Certificate only, not an exact arbitrary-n phase law. |
| SOLPRO_W6 | W6-Q4-UNLINKED-TAIL-RECTANGLE | Gives the exact Q4 m>=8 unlinked rho/lambda dominance rectangle with r=1 active. | 21_SOLPRO_W6_THEORY.txt | RECHECK-PASS | Q4 down/cap and complete randomized prefix hull only; imports Cont-2. |
| SOLPRO_W6 | W6-BLOCK-FANO-BARRIER | Every nontrivial prefix realization has M>3m+2 for n>=4 and 2<=m<=19 at rho=40. | 21_SOLPRO_W6_THEORY.txt | RECHECK-PASS | Exact finite sweep plus analytic tails; memory branch only. |
| SOLPRO_W6 | W6-SEQ-DOWN-STAIRCASE | Exact registered-gauge staircase is mcrit=(0,16,18,18,19 for n>=6). | 21_SOLPRO_W6_THEORY.txt | RECHECK-PASS | Theta-down, complete randomized no-recovery prefix hull; imports W4 floors and Cont-2. |
| SOLPRO_W6 | W6-MDC-OPAQUE-RANK-SEPARATION | Fable and Kimi are Pareto-incomparable and not connected by the newly defined opacity/rank-preserving morphisms. | 21_SOLPRO_W6_THEORY.txt | INEQUIVALENT | Valid only inside the explicit morphism category; imported ledgers are absent from the peer return. |
| SOLPRO_W6 | W6-AGTV-CONDITIONAL-RD | Decision-TV agency RD reduces to conditional Hamming RD, including q-ary and heterogeneous water filling. | 21_SOLPRO_W6_THEORY.txt | ACCEPT | Requires S independent of X, deterministic correct action, and q_s>=2; supplied EC is partial. |
| SOLPRO_W6 | W6-PHASE-POLAR-MASTER | Places finite-prefix, ISC, and sequential scalar-floor phases in one polar bookkeeping algebra. | 21_SOLPRO_W6_THEORY.txt | ACCEPT | Algebra/registry only; authored EC tag is not attested and arbitrary finite-prefix phases stay open. |
| SOLPRO_W6 | W6-BP1-LEAF-ENTROPY-OBSTRUCTION | An almost-full leaf defeats the pointwise entropy-tangent proof route at the heavy vertex. | 21_SOLPRO_W6_THEORY.txt | ACCEPT | Local route obstruction only; BP1 itself remains open. |
| KIMI_W6 | W6-PARITY-N-INV | Sequential parity has ledger (3m+2,0,4) for every n. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Rank-area model lock required. |
| KIMI_W6 | W6-LEAF-OCC | Bounds leaf success by E[min(1,r 2^-abs(Q_m))] and identifies the r=1 occupancy moment. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Iid demand lock; adaptive demands are not covered. |
| KIMI_W6 | W6-NOMSG-VERTEX | Reduces no-message occupancy to heavy/band vertices by Schur convexity. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Scoped to the registered demand polytopes. |
| KIMI_W6 | W6-NOMSG-LAW | Gives the exact no-message cutoff law and rho-star no-message surface. | 31_KIMI_W6_PACKAGE.md | RECHECK-PASS | No-message cutoff is not the same as full-hull mcrit, especially n=2. |
| KIMI_W6 | W6-TREE-BARRIER-N | Claims every nontrivial tree lies above parity for 3<=m<=19. | 31_KIMI_W6_PACKAGE.md | RECHECK-PASS | Accepted at locked iid gauge; all-kink remediation evidence is not shipped as standalone code. |
| KIMI_W6 | W6-GENN-PHASE | Claims the exact Theta-down registered-gauge full-prefix mcrit staircase in n. | 31_KIMI_W6_PACKAGE.md | RECHECK-PASS | Accepted with imported W4 floor and latency dependencies; package attestation is incomplete. |
| KIMI_W6 | W6-BATCH-PHASE | Batch parity (5,0,4) dominates the full batch hull for m>=1,n>=3 at (40,20). | 31_KIMI_W6_PACKAGE.md | ACCEPT | Depends on registered batch model and inherited floors. |
| KIMI_W6 | W6-RHO-SURFACE | Calls max(no-message, tree, latency thresholds) the exact linked/unlinked phase surface. | 31_KIMI_W6_PACKAGE.md | OPEN | Tree component is only sufficient-certified, so the global iff/exact wording is not established. |
| KIMI_W6 | W6-MDC-STRAT | Separates two-demand critical dimension by residual rank. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Scoped rank-area statement; imported candidate ledgers remain PI. |
| KIMI_W6 | W6-MDC-LEAFCOIN | Reports Fable/Kimi two-demand floor coincidence at five registered vertices. | 31_KIMI_W6_PACKAGE.md | RECHECK-PASS | Finite vertex EC only. |
| KIMI_W6 | W6-MDC-MDS | Binary U_r,n is linear-realizable only for r in {1,n-1,n}. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Binary linear category only. |
| KIMI_W6 | W6-BP1-E1-UNIFORM | Gives the exact uniform one-bit error via the majority ball. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Analytic all-n one-bit statement. |
| KIMI_W6 | W6-BP1-UNIFORM-RED | Reduces uniform BP1 to a subset functional and confirms n<=4 by EC. | 31_KIMI_W6_PACKAGE.md | RECHECK-PASS | EC closes n<=4 only; greedy route is killed, general BP1 remains open. |
| KIMI_W6 | W6-BP1-CRUDE | Gives the universal crude first-breakpoint lower bound t1>=4. | 31_KIMI_W6_PACKAGE.md | ACCEPT | Registered vertex classes. |
| KIMI_W6 | W6-AGRD-DTV | Claims adaptive-demand invariance and a decision-TV converse with rate conditioned on demands. | 31_KIMI_W6_PACKAGE.md | RECHECK-FAIL | Adaptive S can reveal X for free; H(X given S) replaces H(X), and iid occupancy is unavailable. |
| KIMI_W6 | W6-MASTER-TABLE | Purports to unify rho-star(n,s,Theta), sequential m, MDC, and prefix phases. | 31_KIMI_W6_PACKAGE.md | OPEN | Inherits the non-exact RHO-SURFACE and mixed PI/EC rows. |
| KIMI_W6 | W6-BP1 | Peer-defined BP1 family comprising E1-UNIFORM, UNIFORM-RED, and CRUDE. | 31_KIMI_W6_PACKAGE.md | OPEN | Family label only; n>=5/general closure is not delivered. |
| KIMIK3_THINKING | W6-DS-G1 | Gives the arbitrary-n coverage-leaf bound for randomized hulls. | 41_KIMIK3_THINKING_W6_PACKAGE.md | ACCEPT | Self-contained scoped proof. |
| KIMIK3_THINKING | W6-DS-G2 | Computes C8,C16,C32 and a C64 prefix with ell>=2 thresholds. | 41_KIMIK3_THINKING_W6_PACKAGE.md | RECHECK-PASS | Exact DP reproduced. |
| KIMIK3_THINKING | W6-DS-G3 | Reduces multi-demand memory to an imported one-demand floor. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Q3-down floor is PI. |
| KIMIK3_THINKING | W6-DS-G4 | Gives an n=4 m=10..18 barrier and refined minimum. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Checker output was not independently inspected; theorem row has a 5.514/13.514 wording defect. |
| KIMIK3_THINKING | W6-DS-G5 | States sharp n=4,m=18 down/cap margins. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | No standalone proof or printed certificate in the KimiK3 corpus. |
| KIMIK3_THINKING | W6-DS-G6 | States the m>=19 obstruction, including -3/2. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Prior substrate claim only in this corpus. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-G7 | Claims the corrected n=3 full phase mcrit=16. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/G1_G12_CONT2_GENERALIZATION.md | OPEN | Exact endpoints pass, but the DeepSeek lane uses a conflicting Theta definition/status and small-m floors remain PI. |
| KIMIK3_THINKING | W6-DS-G8 | Gives the n=5 phase fragments [1,3] and [11,18], with [4,10] open. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Endpoint arithmetic passes; Q5 floor and middle strip are unresolved. |
| KIMIK3_THINKING | W6-DS-G9 | Gives n=4 no-message mcrit(rho), barrier, and full-phase rho thresholds. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Arithmetic rechecks pass; full-hull thresholds still import floors/barriers. |
| KIMIK3_THINKING | W6-DS-G10 | Decouples lambda and derives lambda-star=rho-star/2 from one-demand floors. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Exact thresholds depend on PI floor envelopes. |
| KIMIK3_THINKING | W6-DS-G11 | Records that the method is n-invariant while numeric certificates are n-specific. | 41_KIMIK3_THINKING_W6_PACKAGE.md | ACCEPT | Limitation statement, not a new phase theorem. |
| KIMIK3_THINKING | W6-DS-G12 | Lists eight exact-arithmetic swarm practices. | 41_KIMIK3_THINKING_W6_PACKAGE.md | ACCEPT | Engineering note, not a mathematical theorem. |
| KIMIK3_THINKING | W6-DS-M3 | Gives one n=4 down-vertex Fable/Kimi separating ledger cell. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Raw ledger cell rechecks; hull margins import PI floors. |
| KIMIK3_THINKING | W6-DS-M4 | Claims no Fable-to-Kimi reduction from differing expand distributions. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | No formal reduction category is defined. |
| KIMIK3_THINKING | W6-DS-M5 | Claims no Kimi-to-Fable reduction from p_c nonrepresentation. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Direct-family algebra does not prove universal non-reduction. |
| KIMIK3_THINKING | W6-DS-M6 | Claims probabilistic p_c versus algebraic rank prevents reduction. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Distinguishes theta-preserving parameter laws only. |
| KIMIK3_THINKING | W6-DS-M7 | Gives common carried-token accounting and memory gap 1-p_c. | 41_KIMIK3_THINKING_W6_PACKAGE.md | RECHECK-PASS | Rational ledger arithmetic reproduced. |
| KIMIK3_THINKING | W6-DS-M10 | Claims permanent dual-track separation from certificates C1-C8. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Distinct models survive; permanence/no-reduction needs a category. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A1 | Audits binary agency RD and all-theta strengthening. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Converse is sound; equality imports achievability. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A2 | Gives non-recovery water filling, uniqueness, and strict advantage. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Full support, n>1, and interior D required. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A3 | Claims a closed corridor with strict memory margin at the boundary. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | RECHECK-FAIL | n=2,s=h=q=c=0,D=1/2 gives zero, not strict, margin. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A4a | Gives the hybrid threshold rho-star(D)=1+log2(1-D). | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Valid for the lane's locked chord model; do not identify it with the conflicting canonical hybrid. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A4b | Shows pure latency charge collapses the hybrid rate to zero. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Scoped consequence of A4a. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A4c | Gives Model-H ledger frontiers and a unique D-dagger. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | RECHECK-PASS | Numeric root passes; separate model from canonical HYBRID-LOSSY. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A5a | Gives binary-source k-action 0-1 agency RD. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Binary truth and stated action container only. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A5b | Gives conditional decision-TV RD and two binary variants. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Conditional point-mass truth; marginal-TV is not equivalent. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A6 | Corrects opacity interpolation to I=beta n. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Direct finite construction. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A7 | Gives the piecewise rho-star(s) corridor map for s<=3. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Seam arithmetic passes; polar floor identity is inherited. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A8 | Audits five class thresholds and the Q3-down attribution gap. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Arithmetic passes; Q3-down binding pair stays PI/open. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A9 | Assembles a lossy region plus exact D=0 m strips. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | RECHECK-FAIL | Inherits A3's false strict corner and PI sequential floors. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-A10 | Lists four agency obstruction lines. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | PI/SB dependency map, not an auditable theorem. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B1 | Gives the Fable/Kimi antipodal one-bit bridge identity. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | RECHECK-PASS | Exact fractions reproduce. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B2 | Identifies support-functional majority error and a Hamming-nearest counterexample. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Identity is sound; broad enumeration/count law is EC-only. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B3 | Restates BP1 equivalence and t1=inf 2ell/(1/2-e). | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Full equivalence is imported from Fable. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B4a | Bounds every minimum-depth-two leaf ratio by 1/4. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | ACCEPT | Self-contained restricted lemma. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B4b | Claims universal amortized tangents for five n<=4 classes. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Certificate method is plausible, but this audit chain did not independently inspect the universal run. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B4c | Gives a root-split sufficient condition. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | ACCEPT | Conditional on the stated excess/slack definitions. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B4d | Certifies selected n=5 optimal-root/subcube/depth-2 families. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Bounded fragment only; suboptimal roots and BP1(5) remain open. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B5 | Claims density 1/2 occurs exactly at antipodal pairs and obstructs greedy splitting. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Antipodal attainment/route obstruction survives; global uniqueness exceeds the size<=4 enumeration. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B6 | Gives rho-kill=max(12,4/e_anti) and claims value 12 for every n>=8. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Checked through n=1024; no analytic all-n inequality or global-threshold definition. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B7 | Gives the second one-bit floor segment in five classes. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Line arithmetic is consistent; floor envelope remains PI/EC. |
| KIMIK3_THINKING + DEEPSEEK_W6 lane | W6-DS-B8 | Records BP1 status and a corrected t1 table for n=2..15. | 41_KIMIK3_THINKING_W6_PACKAGE.md; peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | OPEN | Table rechecks; small-n universal proofs are conditional and general n stays open. |
| KIMIK3_THINKING | W6-DS-U1 | Claims a master rho-star/m-phase/MDC dual-track table. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Claimed table/proof is absent from the KimiK3 corpus. |
| KIMIK3_THINKING | W6-DS-U3 | Identifies RACE half-spaces and orthant forms as gauge specializations. | 41_KIMIK3_THINKING_W6_PACKAGE.md | ACCEPT | Scoped algebraic specialization. |
| KIMIK3_THINKING | W6-DS-U4 | Claims DLU ledger uniqueness, path nonuniqueness, and cone/radii formulas. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Coordinate algebra passes; policy/model uniqueness is inherited. |
| KIMIK3_THINKING | W6-DS-CONT2-RELOCK | Re-attests Q4 full-prefix arithmetic. | 41_KIMIK3_THINKING_W6_PACKAGE.md | OPEN | Literal command tree and CONT1_CHECKS.py are absent; no independent rerun under that audit restriction. |
| DEEPSEEK_W6 | W6-DS-C2N(r) | Peer-defined object family for general-n prefix external-path minima. | 42_DEEPSEEK_W6_PACKAGE.md | ACCEPT | Registry object, not promoted theorem. |
| DEEPSEEK_W6 | W6-DS-PCOV(n,m) | Peer-defined object family for general-n coverage lower bounds. | 42_DEEPSEEK_W6_PACKAGE.md | ACCEPT | Registry object, not promoted theorem. |
| DEEPSEEK_W6 | W6-DS-C2N-COMPUTE | Computes exact prefix spectra for n=3,4,5. | 42_DEEPSEEK_W6_PACKAGE.md | RECHECK-PASS | Finite n only; tier scripts reproduce. |
| DEEPSEEK_W6 | W6-DS-PCOV-TABLE | Gives a Theta-down coverage table for n=3..5. | 42_DEEPSEEK_W6_PACKAGE.md | OPEN | Marked DR+EC but leaves the n=5,m=20 entry as a placeholder. |
| DEEPSEEK_W6 | W6-DS-CONT2-REVERIFY | Independently reimplements the Q4 Cont-2 certificate. | 42_DEEPSEEK_W6_PACKAGE.md | RECHECK-PASS | Arithmetic survives; original Sol Pro sources are absent, so provenance is conditional. |
| DEEPSEEK_W6 | W6-DS-MDC-TRIAD | Says Fable, Kimi, and SolPro MDC paths are distinct and no reduction exists. | 42_DEEPSEEK_W6_PACKAGE.md | INEQUIVALENT | Distinct registered objects yes; universal no-reduction is downgraded to SB/recommendation. |
| DEEPSEEK_W6 | W6-DS-RHOKILL-RESOLVED | Defines rho-kill from one-bit and zero-message witnesses. | 42_DEEPSEEK_W6_PACKAGE.md | OPEN | Branch arithmetic passes; not proven to be the global exact threshold and E10 master is broken. |
| DEEPSEEK_W6 | W6-DS-BP1-STATUS | States n<=4 exact-DP status and general n>=5 open. | 42_DEEPSEEK_W6_PACKAGE.md | ACCEPT | Status statement survives with wording fix; n=5 fragments do not imply BP1(5). |
| DEEPSEEK_W6 | W6-DS-HYBRID-LOSSY | Proposes a coin-gated lossy+expand ISC construction. | 42_DEEPSEEK_W6_PACKAGE.md | OPEN | Canonical and lane formulas are different; rate-only SB, not M/L dominance. |
| DEEPSEEK_W6 | W6-DS-PHASE-MASTER | Calls a mixed ISC/W4/Cont-2 table a unified phase theorem. | 42_DEEPSEEK_W6_PACKAGE.md | OPEN | Draft row registry with mixed PI/DR/EC/SB; master runner fails at E10. |
| DEEPSEEK_W6 | W6-DS-COVERAGE-LEAF-GEN | Generalizes the coverage-leaf bound to arbitrary n. | 42_DEEPSEEK_W6_PACKAGE.md | ACCEPT | Deterministic prefix trees plus seed conditioning; locked no-recovery class. |
| DEEPSEEK_W6 | W6-DS-AGENCY-DECTV | Drafts a finite-action decision-TV agency model. | 42_DEEPSEEK_W6_PACKAGE.md | OPEN | Keep SB; valid binary point-mass specialization does not justify generic K-ary wording. |
| DEEPSEEK_W6 lane | Cont-1 | Four overloaded agency audit rows under one ID. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Peer itself marks the identifier overloaded; not a stable single theorem. |
| DEEPSEEK_W6 lane | A4-HYBRID | Gives a lane-specific coin-gated hybrid construction. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Formula conflicts with canonical W6-DS-HYBRID-LOSSY. |
| DEEPSEEK_W6 lane | A4-HYBRID-BOUND | Bounds the hybrid crossover D-dagger. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Final in-text correction makes it an upper, not lower, bound. |
| DEEPSEEK_W6 lane | A5-TV-FANO | Reduces conditional decision-TV against binary point truth to Hamming/Fano. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Binary point-mass truth only. |
| DEEPSEEK_W6 lane | A5-TV-AGENCY | Extends the scoped binary decision-TV model to a finite action container. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Does not imply generic K-ary uniform-source RD. |
| DEEPSEEK_W6 lane | A5-K-ARY | Claims a generic K-ary agency extension. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Remains SB; generic formulation is not proved. |
| DEEPSEEK_W6 lane | A6-OPACITY | Audits opacity interpolation. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | ACCEPT | Scoped finite construction. |
| DEEPSEEK_W6 lane | A6-MULTI | Proposes a multi-object opacity extension. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Optional/model-dependent synthesis. |
| DEEPSEEK_W6 lane | A7-CORRIDOR | Gives a piecewise corridor/rho map. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Arithmetic is scoped; floor identities remain PI. |
| DEEPSEEK_W6 lane | A8-ISC-PHASE | Registers five ISC/class phase thresholds. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Q3-down attribution remains open/PI. |
| DEEPSEEK_W6 lane | A9-INTERVAL | Gives a partial lossy-dominance interval. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | Partial and model-specific; no multi-demand D>0 closure. |
| DEEPSEEK_W6 lane | A10-OBSTRUCT | Lists agency obstructions. | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | OPEN | PI/SB map, not a theorem. |
| DEEPSEEK_W6 lane | M1-ZE (alias MDC-2) | Gives the Fable zero-error two-demand ledger. | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | OPEN | Integer checks reproduce; original Fable policy source is missing. |
| DEEPSEEK_W6 lane | M1-CRIT (alias MDC-3) | Gives the Fable critical-dimension condition. | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | OPEN | Candidate-specific and dependent on missing PI floors. |
| DEEPSEEK_W6 lane | M1-HULL (alias MDC-4) | Promotes the Fable condition to a hull statement. | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | OPEN | Full lossy-hull implication is imported, not re-established. |
| DEEPSEEK_W6 lane | M2-NEC (alias MDC-NECESSITY) | Gives a ledger-model necessary condition. | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | ACCEPT | Does not prove policy uniqueness or universal non-reduction. |
| DEEPSEEK_W6 lane | M6-DIFF | Shows Fable and Kimi have different parameter dependence. | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | INEQUIVALENT | Distinguishes locked formulas only; no general reduction theorem. |
| DEEPSEEK_W6 lane | U2-1 | Identifies the m=1 multi-demand ledger with the W4 single-demand ledger. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | ACCEPT | Algebraic dimensional reduction. |
| DEEPSEEK_W6 lane | U2-2 | Bridges m<=9 to one-demand floors. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Frozen W4 floors are imported. |
| DEEPSEEK_W6 lane | U2-3 | Locates the transition from one-demand floors to coverage-leaf bounds. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | ACCEPT | Structural synthesis only. |
| DEEPSEEK_W6 lane | U2-4 | Attributes the Q4 m=18/19 boundary to the no-message face. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | RECHECK-PASS | Exact Q4 arithmetic survives; imports Cont-2. |
| DEEPSEEK_W6 lane | U3-1 | Equates two phase descriptions at collapsed gauges. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | ACCEPT | Scoped algebra. |
| DEEPSEEK_W6 lane | U3-2 | Claims a stronger general-gauge structural characterization. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Depends on imported Kimi structure. |
| DEEPSEEK_W6 lane | U3-3 | Claims maximality of the Kimi cone/region. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | LP assertion is not independently attested. |
| DEEPSEEK_W6 lane | U4-1 | Equates two descriptions of ledger uniqueness and path nonuniqueness. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | ACCEPT | Model-locked synthesis; not policy uniqueness. |
| DEEPSEEK_W6 lane | U4-2 | States the formal DLU ledger/path uniqueness distinction. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | ACCEPT | Coordinate statement only. |
| DEEPSEEK_W6 lane | U5-1 | Models asymptotic Markov-demand cost as a max-mean tropical cycle. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Optional SB/toy algebra. |
| DEEPSEEK_W6 lane | U5-2 | Treats the opaque handle as a tropical identity element. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Optional SB interpretation. |
| DEEPSEEK_W6 lane | U5-3 | Treats residual-rank-one parity as a tropical accelerator. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Optional asymptotic analogy, not frozen theory. |
| DEEPSEEK_W6 lane | U6-1 | Proposes a sparse-gradient dual-weighted expand bound. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | SB task-class bridge. |
| DEEPSEEK_W6 lane | U6-2 | Gives the sparse-gradient token ledger at (40,20). | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Toy/model-dependent row. |
| DEEPSEEK_W6 lane | U6-3 | Gives an n=100,k=3 numerical sparse-gradient example. | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | OPEN | Example only; no general theorem. |
| GROK_W6 | W6-GROK-CONT2-RELOCK | Re-attests Q4 Cont-2 arithmetic and the m=18/19 transition. | 53_GROK_W6_02_THEOREM_INDEX.md | RECHECK-PASS | Renamed checker passes; historical command/path is absent and logic is inherited. |
| GROK_W6 | W6-GROK-COV-LEAF-GEN | Gives the arbitrary-n coverage-leaf inequality. | 53_GROK_W6_02_THEOREM_INDEX.md | ACCEPT | Statement survives, but the displayed conditional leaf proof line needs repair. |
| GROK_W6 | W6-GROK-LENGTH-SPECTRUM-N | Computes C8,C16,C32. | 53_GROK_W6_02_THEOREM_INDEX.md | RECHECK-PASS | Bundled log prints only part of C32; audit expanded it from the same DP. |
| GROK_W6 | W6-GROK-CONT2-NOMSG-MFAIL | Calls a crude P0>=2^-n obstruction bound the first no-message crossover. | 53_GROK_W6_02_THEOREM_INDEX.md | RECHECK-FAIL | Bound is sufficient only; n=3 crosses at 17 while formula returns 18. |
| GROK_W6 | W6-GROK-CONT2-N3-EXACT | Gives the n=3 heavy-vertex no-message crossing at 16/17. | 53_GROK_W6_02_THEOREM_INDEX.md | RECHECK-PASS | Vertex/no-message result only; omitted monotonicity is needed for all m>=17. |
| GROK_W6 | W6-GROK-CONT2-LIFT-BARRIER | Shows Q4 constants cannot be naively substituted into arbitrary n. | 53_GROK_W6_02_THEOREM_INDEX.md | ACCEPT | Methodological obstruction, not a general-n phase theorem. |
| GROK_W6 | W6-GROK-CONT2-FULL-N | Seeks full-prefix parity dominance for n!=4. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Correctly labeled SB/open. |
| GROK_W6 | W6-GROK-MDC-SEP | Claims Fable/Kimi nonidentity and universal non-reduction. | 53_GROK_W6_02_THEOREM_INDEX.md | INEQUIVALENT | Nonidentity passes; universal no-reduction does not. |
| GROK_W6 | W6-GROK-MDC-FABLE-NCRIT | Gives the candidate-specific Fable ncrit=5 condition. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Algebra passes; full lossy hull and DP floors are PI. |
| GROK_W6 | W6-GROK-MDC-KIMI-LEDGER | Gives Kimi batch/sequential ledgers and conditional margins. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Arithmetic passes; F2/G2 floors are hardcoded PI. |
| GROK_W6 | W6-GROK-MDC-MERGE | Proposes one merged MDC label. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Correctly blocked; distinct namespaces must remain. |
| GROK_W6 | W6-GROK-BP1-EQUIV | Imports BP1 equivalence and checks five rational identities. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Tautological EC plus PI theorem; no new closure. |
| GROK_W6 | W6-GROK-BP1-LOCAL-KILL | Uses antipodal pairs to kill any per-subset local-density proof. | 53_GROK_W6_02_THEOREM_INDEX.md | ACCEPT | Valid route obstruction; does not disprove BP1. |
| GROK_W6 | W6-GROK-BP1-T1-TABLE | Computes t1 candidates for n=2..15. | 53_GROK_W6_02_THEOREM_INDEX.md | RECHECK-PASS | Exact arithmetic table, explicitly SB as a floor theorem. |
| GROK_W6 | W6-GROK-BP1-GENERAL-N | Seeks the amortized tangent for all n>=5. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Correctly labeled open. |
| GROK_W6 | W6-GROK-AG-SOFT | Imports binary ISC R_ag(D)=1-H2(D). | 53_GROK_W6_02_THEOREM_INDEX.md | ACCEPT | Standard/PI endpoint, not new. |
| GROK_W6 | W6-GROK-AG-HYBRID-TV | Shows expand/soft time-sharing lies above the convex binary RD curve. | 53_GROK_W6_02_THEOREM_INDEX.md | ACCEPT | Narrow binary ISC class only. |
| GROK_W6 | W6-GROK-AG-PROD | Names production multi-agent decision-TV. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Out of scope/SB; no claim delivered. |
| GROK_W6 | W6-GROK-PHASE-TABLE | Collects W4, Cont-2, Fable, and Kimi phase constants. | 53_GROK_W6_02_THEOREM_INDEX.md | OPEN | Reproducible orientation table, not a master phase theorem. |
| QWEN_W6 | W6-QWEN-COVERAGE-N | Gives arbitrary-n coverage and heavy-vertex lower bounds. | 61_QWEN_W6_PACKAGE.md | ACCEPT | Self-contained; low-m lower bound may be negative but remains valid. |
| QWEN_W6 | W6-QWEN-SPECTRUM | Computes exact C8,C16,C32,C64 spectra. | 61_QWEN_W6_PACKAGE.md | RECHECK-PASS | n=3..6 only; embedded checker, no standalone file. |
| QWEN_W6 | W6-QWEN-BARRIER-N | Claims its spectrum-coverage barrier stays positive through each reported mcrit. | 61_QWEN_W6_PACKAGE.md | RECHECK-FAIL | Formula is valid, but n=6,m=10..14 values are negative. |
| QWEN_W6 | W6-QWEN-NOMSG-N | Gives exact heavy-vertex no-message occupancy and decreasing gaps. | 61_QWEN_W6_PACKAGE.md | RECHECK-PASS | Exact formulas/margins rerun. |
| QWEN_W6 | W6-QWEN-OBSTR | Gives a universal sufficient no-message obstruction onset. | 61_QWEN_W6_PACKAGE.md | ACCEPT | Not the law-dependent exact critical point. |
| QWEN_W6 | W6-QWEN-PHASE-N | Reports mcrit(3,4,5,6)=(16,18,18,19). | 61_QWEN_W6_PACKAGE.md | RECHECK-FAIL | n=6,m=10..14 is uncovered; last-passing-point code does not test contiguity. |
| QWEN_W6 | W6-QWEN-N3-SHARP | Gives the exact n=3 registered-gauge 16/17 sign change. | 61_QWEN_W6_PACKAGE.md | RECHECK-PASS | Exact fractions rerun under the lock. |
| QWEN_W6 | W6-QWEN-MASTER | Gives finite-n floors, obstruction/mcrit rows, and an infinity row. | 61_QWEN_W6_PACKAGE.md | RECHECK-FAIL | n=6 gap, unattested exact F5/F6, and SB infinity mcrit mislabeled EC. |
| QWEN_W6 | W6-QWEN-MDC-SEP | Compares assumed Fable/Kimi ledgers and their equality condition. | 61_QWEN_W6_PACKAGE.md | OPEN | Conditional algebra only; cited W5 ledger files are absent/miscited. |
| QWEN_W6 | W6-QWEN-MDC-MECHANISM | Attributes Fable savings to collision mass and Kimi savings to residual rank one. | 61_QWEN_W6_PACKAGE.md | OPEN | No self-contained policy/rank proof or independent EC; not P2 rank stratification. |

## Complete peer log

| peer/scope | source paths read completely | ID accounting | integrated disposition |
|---|---|---|---|
| SUBSTRATE + METHODS | Mandatory flat 00-18, substrate/cont2 payloads, 70_ADJACENT_MATH_AI_PROOF_METHODS.md, 71_W5_GROK_CONFLICT_MATRIX.md, 72_OMEGA_FRANKENSIM_MATH_TRANSFER.md | Substrate IDs used as dependencies, not misattributed as Wave-6 peer-owned IDs. | Cont-2 Q4 is attested; Cont-1 no-message needs the Cont-2 repair; methods files add no theorem. |
| SOLPRO_W6 | Flat 20-28 and every peers/SOLPRO_W6 regular file, including PDF/TXT/output/manifest pairs. | 10 promoted W6 IDs; 16 inherited Core IDs remain dependency-only. | Mathematics qualified-pass; missing W6 C++ source and narrow EC coverage prevent package-level attestation. |
| KIMI_W6 | Flat 30-37, manifest-listed peers/KIMI_W6 files, generated runtime files, and duplicate maps. | 16 indexed theorem IDs plus peer-defined W6-BP1 family. | Sequential/MDC/BP1 fragments survive; adaptive AGRD-DTV fails; exact master surface remains open. |
| KIMIK3_THINKING | Flat 41, both peers/KIMIK3_THINKING files, provenance duplicates, and byte-identical package copies. | 46 rows in the report's complete new-ID inventory. | Useful mixed package; A3/A9 false edge, MDC category gap, B5/B6 overreach, and absent U1 table block freeze. |
| DEEPSEEK_W6 | Flat 40,42,43 and 104 peer files totaling 812,718 bytes, including lanes, checkers, outputs, reruns, and one binary. | 10 canonical promoted IDs, 2 registry object families, explicit A/B/G candidates deduplicated with KimiK3, and all named A/M/U lane claim IDs expanded above. | Exact EC is substantial; canonical package, lane drafts, mixed provenance, and broken E10 master must remain separated. |
| GROK_W6 | Ten flat 50-59 documents, their ten peer mirrors, three Grok checkers, four stored outputs, README, and runner. | 19 indexed W6-GROK IDs. | Three direct checkers pass; aggregate runner fails; m_fail exact-onset wording fails. |
| QWEN_W6 | Flat 60-61, three package duplicates, SHA manifest, operator manifest, and campaign control file. | 10 W6-QWEN IDs. | Coverage/spectrum/no-message survive; n=6 barrier gap defeats PHASE-N and MASTER attestation. |

## NOT_IN_ZIP merge, deduplicated

| class | exact missing/stale reference | merged resolution |
|---|---|---|
| stale absence marker | QWEN_W6 NOT_IN_ZIP unless present | False for this extraction: flat 60-61 and peers/QWEN_W6 package/provenance/SHA files are present. |
| absent public docs | docs/racc-public.md; docs/RACC_RESEARCH_DISTILL.md | No flat equivalents found. |
| absent W5 peer islands | wave5-returns/FABLE/; wave5-returns/KIMI/; wave5-returns/GROK_DEEP_RESEARCH/ | Not present; imported peer-island/Core claims cannot be provenance-rechecked from substrate. |
| freeze remap | freeze/RADC_FORMAL_CORE_V1_FREEZE.md; 00_RADC_FORMAL_CORE_V1_FREEZE.md | Original names absent; flat 01_RADC_FORMAL_CORE_V1_FREEZE.md is present. |
| README remap | 01_README_OPEN_FIRST.txt | Original number absent; flat 03_README_OPEN_FIRST.txt is present. |
| Cont-2 layout remap | wave5-returns/SOLPRO/cont2/; 10_SOLPRO_CONT2.md; 12_SOLPRO_CONT2_CHECKS.py; 13_SOLPRO_CONT2_CHECKS.cpp; 14_SOLPRO_CONT2_GRID.cpp; 15_SOLPRO_CONT2_CHECKS.out; 17_SOLPRO_CONT2_SHA256.txt | Mirrored under substrate/cont2 and flat 10-16 with W5 in the basenames; hash-matching replacements exist. |
| Cont-1 missing evidence | wave5-returns/SOLPRO/cont1/; RADC_W5_SOLPRO_CONTINUATION_1.md; 20_SOLPRO_CONT1.md; 21_SOLPRO_CONT1_CHECKS.py; 22_SOLPRO_CONT1_CHECKS.out; CONT1_CHECKS.py | Text remaps to flat 17_SOLPRO_W5_CONT1.md; checker/output are absent and substrate/cont1 is empty. |
| Wave4 remap | sources/wave4/WAVE4_SOLPRO_PACKAGE_FULL.txt | Flat 18_WAVE4_SOLPRO_PACKAGE_FULL.txt is present. |
| SOLPRO W6 missing independent source | W6_THEORY_CHECKS.cpp | Absent everywhere; only manifest hash and recorded outputs exist. |
| SOLPRO peer-local dependency gap | peers/SOLPRO_W6 copies of frozen substrate and all 16 W4/W5 dependency sources | Absent from peer return; some outer-flat replacements exist, but peer-local provenance is incomplete. |
| SOLPRO manifest omission | primary W6 PDF, TXT, provenance, and manifest self-hash | Files are present, but W6_THEORY_SHA256.txt does not attest them. |
| DeepSeek legacy core names | RADC_FORMAL_CORE_V1_1_FREEZE.md | No equivalent freeze-v1.1 source found. |
| DeepSeek missing W5 inputs | 30_SOLPRO_W5_THEORY_FULL.txt; 40_FABLE_THEOREM_INDEX_EXTRACT.txt; 42_KIMI_W5_PACKAGE.md; 43_FABLE_W5_PACKAGE.md; W5_COMP_CERTIFICATES.md | Absent; imported PI rows stay conditional. |
| conflict/method remaps | 41_GROK_CONFLICT_MATRIX.md; 51_OMEGA_FRANKENSIM_MATH_TRANSFER.md | Exact names absent; likely flat replacements are 71_W5_GROK_CONFLICT_MATRIX.md and 72_OMEGA_FRANKENSIM_MATH_TRANSFER.md. |
| DeepSeek original return layout | Pareto/wave6-returns/DEEPSEEK_W6/ | Absent; remapped to peers/DEEPSEEK_W6/. |
| Kimi archive/runtime delta | original KIMI zip; peers/KIMI_W6/00_PROVENANCE.txt; peers/KIMI_W6/w6/__pycache__/w6_lib.cpython-312.pyc; peers/KIMI_W6/.tokenzero/**; flat 30-37 | Original archive is absent. Listed files are extraction/runtime/flattening additions not covered by the Kimi SHA manifest; they are present but not archive-attested. |
| Kimi manifest defect | SHA256SUMS.txt self-entry | 25 payload hashes pass; manifest self-hash fails by self-reference. |
| KimiK3 legacy return layout | Pareto/wave6-returns/DEEPSEEK_W6/; .radc-pack/wave6/ec/ | First remaps to peers/DEEPSEEK_W6; second has only a partial substrate/cont2 counterpart. |
| KimiK3 renamed sources | 17_SOLPRO_CONT2_SHA256.txt; 00_RADC_FORMAL_CORE_V1_FREEZE.md | Remap to flat 16_SOLPRO_W5_CONT2_SHA256.txt and 01_RADC_FORMAL_CORE_V1_FREEZE.md. |
| KimiK3 missing captures | n5_optclass.out; n5_all16.out | Absent; corresponding C sources exist under peers/DEEPSEEK_W6/checkers/tier5/. |
| KimiK3 count mismatch | claimed 31 captured .out files | Present count is 29 .out plus one markdown audit log. |
| Grok original layout | Pareto/wave6-returns/GROK_W6/; Pareto/wave6-attach-FLAT/60_GROK_W6_*.md | Remap to peers/GROK_W6 and flat 50-59. |
| Grok stale Cont-2 checker | Pareto/wave6-attach-FLAT/12_SOLPRO_CONT2_CHECKS.py; any 12_SOLPRO_CONT2_CHECKS.py; README ../../wave6-attach-FLAT/12_SOLPRO_CONT2_CHECKS.py; runner nested target | Exact target absent; flat 12_SOLPRO_W5_CONT2_CHECKS.py is the working replacement. README and aggregate runner exit 2. |
| Grok missing local dependencies | Pareto/wave5-returns/FABLE/checkers/w5f_final_checks.py; GROK-local w5_full_prefix_check.cpp; mdc_dp; w5b_twodemand.py | Basename-equivalent copies exist elsewhere for some items, but Grok did not ship/rerun local originals. |
| Qwen standalone EC files | w6_qwen_checks.py; w6_floors.py; core_recheck.log | Absent; two fenced programs are present inside 61_QWEN_W6_PACKAGE.md. |
| Qwen Cont-1 evidence | 21_SOLPRO_CONT1_CHECKS.py and claimed rerun output | Absent. |
| Qwen original author path | /Users/aditya/AI/TokenZero/docs/radc-wave6-qwen.md | Not in extraction; three byte-identical bundle package copies are present. |
| Qwen MDC dependencies | intended Fable/Kimi W5 ledger files cited as file 43/file 42 | Absent/miscited: flat 42/43 are DeepSeek W6 package/notes, not W5 ledgers. |

## Conflict-resolution matrix

| topic | SOLPRO_W6 | KIMI_W6 | KIMIK3_THINKING | DEEPSEEK_W6 | GROK_W6 | QWEN_W6 | integrator resolution |
|---|---|---|---|---|---|---|---|
| m_crit | Exact registered Theta-down staircase 0,16,18,18,19+ accepted. | Same staircase accepted with incomplete package attestation; separates no-message cutoffs. | n=3/n=5/n=4 fragments; several PI-conditioned. | Canonical only re-verifies Q4; G7 lane has a class-definition conflict. | n=3 no-message 16/17 only; general full phase open. | Reports 16,18,18,19 for n=3..6, but n=6 m=10..14 certificate fails. | Freeze only the SOLPRO/KIMI registered-gauge staircase, with no-message cutoffs kept separate as 14,16,18,18,19; Qwen is not an independent n=6 confirmation. |
| MDC | Proves separation only in an explicit opacity/rank-preserving morphism category. | Rank stratification/MDS scoped results survive. | Distinct ledgers pass; permanent non-reduction lacks a category. | Canonical distinctness survives; lane also contains a contradictory unified-matroid claim. | Nonidentity survives; universal non-reduction does not. | Conditional assumed-ledger arithmetic, not rank stratification. | Keep MDC-FABLE, MDC-KIMI, and MDC-SOLPRO as distinct IDs. State no-morphism only with SOLPRO's explicit category; reject universal permanence and unified-matroid promotion. |
| BP1 | Local almost-full-leaf route obstruction; BP1 open. | One-bit formulas and n<=4 EC survive; n>=5 open. | n<=4 conditional, n=5 fragments, B5 uniqueness and B6 all-n overreach. | Canonical status: n<=4 five classes, n=5 fragments, general open. | Antipodal route kill and t1 table only; general open. | No P3 theorem delivered. | Accept one-bit identities, route obstructions, and the five n<=4 EC classes. Do not infer BP1(5); general BP1 remains OPEN. |
| agency | Conditional finite decision-TV/Hamming RD survives under S independent of X. | Adaptive-demand invariance is false because S can carry information about X. | A3/A9 strict boundary is false; several narrow binary/hybrid fragments survive. | Canonical and lane hybrid formulas conflict; decision-TV remains binary point-mass scoped/SB. | Narrow binary ISC convex time-sharing theorem survives; production claim excluded. | P4 not delivered. | Split the models. ACCEPT conditional-Hamming/decision-TV only under independent demands and deterministic correct action; REJECT adaptive invariance; make the degenerate boundary non-strict; keep hybrid/multi-agent extensions OPEN. |
| master phase | Polar algebra is bookkeeping, not arbitrary-n exact closure. | RHO-SURFACE/MASTER exact iff overclaims a sufficient tree threshold. | U1 table absent. | Draft mixed-row registry; E10 master path fails. | Orientation table only. | MASTER fails on n=6, F5/F6 attestation, and infinity row. | No exact global master theorem exists. Retain a rowwise registry with per-row PI/DR/EC/SB and exact local slices only; overall verdict OPEN. |

## Concrete review findings

1. **CRITICAL -- adaptive agency theorem false.** 31_KIMI_W6_PACKAGE.md:26-43,361-385,503-517 and 37_KIMI_W6_PROOF_DEVELOPMENT.md:181-205 permit adaptive demands while conditioning rate on those demands; the demand sequence becomes a free X-channel.
2. **HIGH -- Qwen phase certificate has a real gap.** 61_QWEN_W6_PACKAGE.md:338-379,363-366,530-565 has negative barrier values at n=6,m=10..14 and a last-pass accumulator that never checks interval contiguity.
3. **HIGH -- multiple master claims are registries, not exact theorems.** 31_KIMI_W6_PACKAGE.md:271-281,388-409; 42_DEEPSEEK_W6_PACKAGE.md and peers/DEEPSEEK_W6/ec_master_verification.py:514,519,665; 61_QWEN_W6_PACKAGE.md:354-379,702-735.
4. **HIGH -- independent SOLPRO W6 C++ source missing.** 21_SOLPRO_W6_THEORY.txt:2937-2969 and 28_SOLPRO_W6_SHA256.txt:2 cite W6_THEORY_CHECKS.cpp, absent everywhere.
5. **HIGH -- Grok exact m_fail wording is false.** 52_GROK_W6_01_EXECUTIVE_VERDICT.md:46-67, 54_GROK_W6_03_PROOFS.md:113-151, and 58_GROK_W6_07_CORE_V1_1_DELTA.md:17-25 conflate a crude sufficient bound with first crossover.

## Residual risks

- Imported W4/W5/Fable/Kimi sources explicitly marked missing cannot be provenance-attested; all dependent rows remain scoped/OPEN even when arithmetic rechecks.
- This integration did not restate or newly prove peer arguments and did not rerun every checker. RECHECK verdicts preserve the complete xhigh audit reports' independently recorded reruns.
- W6-DS namespace overlap is not authorship evidence. Combined rows explicitly preserve KimiK3 package and DeepSeek lane provenance.
- Full-bundle byte-read guarantees cited-source coverage, not semantic correctness of binary/generated/runtime files.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "The artifact contains 145 deduplicated theorem/claim rows with exact bundle paths and controlled verdicts, a complete peer log, a merged NOT_IN_ZIP table, a five-topic conflict matrix, and five path-specific CRITICAL/HIGH findings."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/06964ddf-9ef8-4cc1-88c9-c08685140e50/analysis-xhigh/81_peer_matrix.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "byte-read seven reports and hash their concatenated content",
      "result": "passed",
      "summary": "7 reports, 190843 bytes; SHA-256 af03d0ba9efb56cf5d9d9a4c14feb9f0085662c6d09255a4a944059d3c574ad7"
    },
    {
      "command": "byte-read every regular file under wave7-attach-FLAT",
      "result": "passed",
      "summary": "337 files, 26255300 bytes; aggregate path/content SHA-256 53ae2c74d72ba306f80ca569cda93e19deeef3600a151bce2ee34fffe856f2de"
    },
    {
      "command": "extract and compare peer theorem IDs against the seven xhigh inventories and canonical source indices",
      "result": "passed",
      "summary": "All promoted/indexed IDs and every explicitly named DeepSeek A/M/U lane claim were expanded; no numeric theorem range remains."
    }
  ],
  "validationOutput": [
    "Reports full-read evidence: 7 files, 190843 bytes, concatenated SHA-256 af03d0ba9efb56cf5d9d9a4c14feb9f0085662c6d09255a4a944059d3c574ad7.",
    "Source coverage evidence: full bundle 337 files, 26255300 bytes, aggregate SHA-256 53ae2c74d72ba306f80ca569cda93e19deeef3600a151bce2ee34fffe856f2de.",
    "Verdict vocabulary is limited to ACCEPT, RECHECK-PASS, RECHECK-FAIL, OPEN, and INEQUIVALENT."
  ],
  "residualRisks": [
    "Missing W4/W5/Fable/Kimi source dependencies prevent provenance attestation for conditional rows.",
    "RECHECK verdicts integrate reruns recorded by the xhigh auditors; this read-only integrator did not execute every peer checker.",
    "Namespace overlap between KimiK3 and DeepSeek is preserved as dual provenance, not merged authorship."
  ],
  "noStagedFiles": true,
  "diffSummary": "Created the requested read-only Wave 7 xhigh peer matrix artifact only.",
  "reviewFindings": [
    "critical: 31_KIMI_W6_PACKAGE.md:361-385 - adaptive demands can leak X through S, invalidating W6-AGRD-DTV adaptive invariance.",
    "high: 61_QWEN_W6_PACKAGE.md:530-565 - last-pass logic misses the negative n=6,m=10..14 barrier interval.",
    "high: 21_SOLPRO_W6_THEORY.txt:2937-2969 - cited independent W6_THEORY_CHECKS.cpp is absent.",
    "high: 54_GROK_W6_03_PROOFS.md:113-151 - crude universal obstruction is mislabeled exact first crossover.",
    "high: peers/DEEPSEEK_W6/ec_master_verification.py:514,519 - undefined rho_kill_kimi breaks the master runner."
  ],
  "manualNotes": "No source file was edited. Only the authoritative output artifact was written."
}
```
