# Wave 7 xhigh read-only audit: DEEPSEEK_W6

**Source:** /Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT  
**Scope read completely:** flat 40, 42, 43 plus every non-tooling file under peers/DEEPSEEK_W6, including lanes, checkers, outputs, and peer reruns.  
**Read attestation:** 104 files, 812,718 bytes. Every byte was read; the single binary (ec-peer-reruns/kimi/mdc_dp, 33,800 bytes) was byte-read and SHA-256 checked. Generated .fszero metadata is excluded.  
**Overall verdict:** **MIXED / NOT FREEZE-READY AS A UNIT.** Exact EC is substantial and tier scripts reproduce, but the canonical package promotes draft/SB statements, omits corrected lane results, depends on missing inputs, and ships a broken E10 master path.

## P1. General-n Cont-2

- **Confirmed in scope:** the coverage-leaf inequality and exact prefix spectra for n=3,4,5. Tier-2 scripts rerun with exit 0 and byte-identical captures.
- **Q4 re-verification:** credible at registered gauge (rho,lambda)=(40,20), Q4-down/Q4-cap, sequential parity path, m=1..18; m>=19 obstruction. Original Sol Pro source/checkers are NOT_IN_ZIP, so this is an independent reimplementation, not source-chain verification.
- **General-n closure remains open.** The owned canonical package correctly does not claim arbitrary-n closure.
- **High status/gauge drift:** swarm_lanes/G1_G12_CONT2_GENERALIZATION.md:547 defines Theta_3-down by theta_i>=1/4 and leaves G7 OPEN, while checkers/tier2/g7_n3_phase.py:2-6 uses the statement-locked theta_i>=4/15, vertex (7,4,4)/15, and certifies m_crit(3)=16. Likewise the lane's G8 uses theta_i>=1/6 and p_min=1-(1/6)^m-4(5/6)^m, while the checker uses theta_i>=4/25, vertex (9,4,4,4,4)/25, and 1-(16/25)^m-4(21/25)^m. Do not merge these as one theorem without restating the class.
- **Medium incomplete table:** 42_DEEPSEEK_W6_PACKAGE.md:239 still says n=5,m=20 is “to be computed” although W6-DS-PCOV-TABLE is marked DR+EC. Independent exact value: 1596172874824372311085379769 / 1818989403545856475830078125 = 0.8775053179050215.

### Key certified integers

- C8=[0,8,10,13,16,20,22,24].
- C16=[0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64].
- C32=[0,32,34,37,40,44,48,52,56,61,66,71,76,81,86,91,96,102,108,114,120,124,128,132,136,141,146,149,152,156,158,160].
- Q4 p10=6560848/9765625; B2..B6=10769686/1953125, 97023471/15625000, 252888283/31250000, 38966203/3906250, 20384017/1562500.
- Q4 m=18 margins: down 277615146191/762939453125; cap 20074685943080277/50000000000000000. m=19 witness <=-3/2.
- Corrected G7 checker: m=16 margin 845049722020265693/437893890380859375; m=17 witness -22519522704133297/437893890380859375.

## P2. MDC separation/unification

- **Accepted:** MDC-FABLE, MDC-KIMI, and MDC-SOLPRO are different registered paths with different timelines and ledgers. Keeping separate IDs is sound.
- **High overclaim:** W6-DS-MDC-TRIAD says “no reduction exists” as DR, but swarm_lanes/M1_M10_MDC_RESOLUTION.md:215-306 labels both reduction attempts BE|SB and FAIL, and M10:501 calls permanent separation a DR|EC|SB recommendation. EC shows differing expand-count distributions/ledger parameterizations only under assumed invariant preservation. No reduction category or preservation axioms are formalized.
- **High internal conflict:** swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md:U7.2 says the paths are “Unified via recovery-closure matroid [DR]”; U8 says they are extremal points of a unified matroid. The canonical package instead says that matroid is promising and not a reduction. Canonical separation is safer; U7.2 must not be cited as proved.
- **Own verdict:** namespace separation **ACCEPT**; “permanent/no reduction exists” **DOWNGRADE TO SB/RECOMMENDATION**.

| Path | Timeline / object | Gauge | Declared status | Key integers | Own verdict |
|---|---|---|---|---|---|
| MDC-FABLE | 4-turn sequential pi_EDC^2; M=9-p_c, L=11/2-(3/2)p_c | ZE gauge-free; full lossy at (40,20) | PI/DR, EC support | p_c=1/4 or 7/25; ledgers (35/4,41/8), (218/25,127/25); n_crit=5 | Conditional on missing Fable source; arithmetic reproduced |
| MDC-KIMI | 3-turn batch PARITY-DUAL; residual rank 1; M=5,L=4 (seq M=8) | examples at (40,20) | PI/DR, EC support | margins (5,0,1)/(7,0,1); F2=10,G2=15 | Conditional on missing Kimi source; arithmetic reproduced |
| MDC-SOLPRO | (m+2)-turn sequential parity; M=3m+2,L=4 | Q4, (40,20), m<=18 | inherited PROVED+EC | m_crit=18 | Accept within locked class |

## P3. BP1 and rho-kill

- **W6-DS-BP1-STATUS:** **ACCEPT WITH WORDING FIX.** The three-way BP1 equivalence is algebraic; n<=4 five shipped classes are exact-DP certified; general n>=5 is OPEN. The density-1/2 antipodal family kills a per-split greedy proof, not every possible induction or global proof.
- **n=5:** only optimal-root, subcube, depth-2, and bounded cell families are certified. B4d leaves suboptimal-root cases open. No BP1(5) theorem.
- **High rho-kill scope issue:** B1_B8_BP1_RESOLUTION.md:242 calls rho_kill a witness lower bound, “not necessarily the exact phase threshold”; TIER5_REPORT later calls max(12,4/e_anti) the full law; the canonical package calls it exact. Branch arithmetic reproduces, but global exactness needs a definition that rho_kill is precisely the maximum of these registered witnesses.
- Values: rho_kill(3..7)=16,160/11,1600/121,64/5,1792/145; registered witness=12 for n>=8; e_anti(8)=43/128 and 4/e=512/43<12. BP1 t1 includes n=4 80/9, n=5 800/79 (not 800/159), n=8 256/21.

## P4. Agency RD / hybrid / decision-TV

- **High canonical mismatch:** 42_DEEPSEEK_W6_PACKAGE.md:302 gives R_hybrid(D)=min_D0 {n[1-H2(D0)]+1-H2(D/D0)} and calls it coin-gated repair. The A4 lane derives n[1-H2(e)]+(1-D/e)H2(e), optionally with repair overhead. These are different constructions. The canonical formula is not established by the lane checker.
- **W6-DS-HYBRID-LOSSY:** rate-only, ISC singleton-demand, amortized construction. It does not certify M- and L-dominance. **DOWNGRADE canonical DR+SB to SB until one construction and cost model are locked.**
- **A4 correction:** its “lower bound” on D-dagger is corrected in-place to an upper bound. Cite only the final correction.
- **W6-DS-AGENCY-DECTV:** retain SB. Conditional decision-TV against point-mass binary truth equals 0-1 error, so 1-H2(D) is valid only in that restricted model. Marginal-TV is degenerate. A generic K-ary uniform-source RD claim would be false.
- Tier-4's eight scripts all rerun, exit 0, and match outputs, but use floats for RD curves. They are numeric audits, not exact proofs.

## P5. Master phase table and runners

- **W6-DS-PHASE-MASTER is a draft registry, not one theorem.** U1 mixes exact ISC formulas, W4 inverses, an “approx” lower-capped condition, asymptotics, and an independent Cont-2 m-axis. **DOWNGRADE DR to mixed PI/DR/EC/SB by row**.
- G9 lane contains sign-chasing and contradictory rho narratives at lines 700-745. The corrected tier-2 G9 output is usable; the prose phase sketch is not.
- **High broken master:** peers/DEEPSEEK_W6/ec_master_verification.py:514/519 calls undefined rho_kill_kimi. E1-E9 pass 328/328; E11 passes 21/21; E12 passes 76/76 when called separately. No E1-E12 grand total is produced.
- **Portability defect:** ec_master_verification.py:665 hardcodes an output outside the bundle. Main would overwrite an external E1_E12_EC_WORKERS.md before crashing at E10.
- **Stored report mismatch:** swarm_lanes/E1_E12_EC_WORKERS.md has only E1-E9 sections. “99/99” is E1 alone, not the current master total.
- **Tier scripts pass independently:** tier2 6/6, tier3 7/7, tier4 8/8, tier5 4 Python + 4 C all exit 0. Corresponding captures match; two helper C programs lack captures. O3 raw capture matches; O3_AUDIT_LOG is enriched manual output.
- Tier5 Python scripts have no assert statements; exit 0 plus identical stdout is reproducibility evidence, not theorem proof.

## Canonical owned theorem inventory

| ID | Path / gauge | Package status | Dependencies | Integers/certificate | Own verdict |
|---|---|---|---|---|---|
| W6-DS-C2N-COMPUTE | Equiprobable binary prefix trees, n=3,4,5; gauge-free | EC [S] | G2, E12, DP recurrence | full C8/C16/C32 above | **ACCEPT EC**, finite n only |
| W6-DS-PCOV-TABLE | Theta_n-down, theta_i>=4/(5n); gauge-free | DR+EC [S] | G1/G7/G8, union bound, heavy vertex | six n=3..5,m=10/12 fractions; m=20 placeholder | **PARTIAL**; fill or remove m=20 row |
| W6-DS-CONT2-REVERIFY | Q4 down/cap, sequential parity vs no-recovery prefix hull, (40,20) | DR+EC [S] | missing Sol Pro 10/12/13/14/15/17 files; owned E1 reimplementation | p10, B2..B6, m18 margins, m19 witness | **ACCEPT CONDITIONALLY** on locked imported theorem |
| W6-DS-MDC-TRIAD | Fable/Kimi/SolPro paths; examples mainly (40,20) | DR [F] | M1-M10; missing peer packages | p_c, expand distributions, ledgers | **PARTIAL**: distinct yes; no-reduction no |
| W6-DS-RHOKILL-RESOLVED | Theta_n-down, linked gauge; one-bit + zero-message witnesses | DR+EC [S] | B6/E10; E10 master broken | values n=3..8 above | **CONDITIONAL** obstruction law, not proven global threshold |
| W6-DS-BP1-STATUS | J_t prefix floor on five n<=4 classes and Theta_n-down | DR [F] | B1-B8; foreign Fable rerun | t1 table, exact frontier DP | **ACCEPT STATUS**; scope obstruction to greedy route |
| W6-DS-HYBRID-LOSSY | ISC, uniform singleton demand, rate-only D>0 | DR+SB [M] | A4/A9; formula conflict | float roots/chord checks | **DOWNGRADE SB** |
| W6-DS-PHASE-MASTER | ISC + W4 floors + Q4 Cont-2; mixed linked gauges | DR [F] | U1-U2, missing W4/Core inputs | rho formulas, seams, mcrit18 | **DOWNGRADE mixed-row registry** |
| W6-DS-COVERAGE-LEAF-GEN | Uniform X, deterministic no-recovery prefix tree, arbitrary theta,m,n; gauge-free | DR [F] | G1 proof | P<=1-pcov(1-r/2^n) | **ACCEPT DR** in stated class |
| W6-DS-AGENCY-DECTV | Binary point-mass truth, finite action container, singleton demand | SB [M] | A5/A10, data processing/Fano | no integer cert | **KEEP SB / scope-lock** |

## Explicit W6-DS candidate and object registry

These IDs occur in owned checker/output/package material but are not all promoted by the canonical ten-row index.

| IDs | Declared path/status | Dependencies / integers | Own verdict |
|---|---|---|---|
| W6-DS-A1, A2, A3 | binary singleton agency RD; water-fill; corridor; DR/EC-numeric | tier4 a1-a3; float grids; D-star | Accept as scoped audits, not new exact EC proofs |
| W6-DS-A4a, A4b, A4c | chord/latency/Model-H hybrid candidates; DR or DR+EC | tier4 a4; H2 float roots | Candidate only; keep separate from conflicting canonical hybrid |
| W6-DS-A5a, A5b | binary truth with finite actions / conditional decision-TV; DR | tier4 a5; marginal-TV counterexample | Accept only with binary-truth scope |
| W6-DS-A6, A7, A8 | opacity audit, rho map, five class thresholds | tier4 a6-a8; landmarks s={1/2,2,5/2,3}, T=8 | Scoped audits; A8 retains Q3d PI gap |
| W6-DS-A9, A10 | lossy interval / obstruction map | A lane; m<=18 D=0 strip | A9 partial; A10 PI/SB map |
| W6-DS-B1, B2, B3 | e_anti bridge, support functional, BP1 equivalence | tier5; n<=30 EC, exact algebra | Accept in model lock |
| W6-DS-B4a, B4b, B4c | deep-leaf lemma; n<=4 universal DP; root-split sufficient condition | tier5 B4; exact C programs | Accept stated fragments only |
| W6-DS-B4d | n=5 optimal-root/subcube/depth-2 families | 41,416 small sets; cell/subcube EC | Bounded fragment, not BP1(5) |
| W6-DS-B5 | density-1/2 antipodal obstruction | tier5 b5; six classes | Accept route obstruction |
| W6-DS-B6 | rho-kill reconciliation | tier5 b6; 1/384 crossing margin | Conditional on rho-kill definition |
| W6-DS-B7, B8 | five-class second segment; t1 target table n=2..15 | exact DP / fractions | Accept EC tables, not all-n BP1 |
| W6-DS-G7 | corrected n=3 full phase candidate | O3 + g7 checker; (4/15),(40,20), mcrit16 | Promising, but **conflicted statement/status**; restate before freeze |
| W6-DS-C2N(r), W6-DS-PCOV(n,m) | object IDs, not theorem IDs | canonical statement lock | Registry objects only |

## Other owned theorem/claim IDs

- A-lane index: **Cont-1** (four overloaded rows), **A4-HYBRID, A4-HYBRID-BOUND, A5-TV-FANO, A5-TV-AGENCY, A5-K-ARY, A6-OPACITY, A6-MULTI, A7-CORRIDOR, A8-ISC-PHASE, A9-INTERVAL, A10-OBSTRUCT**. They inherit P4 scope. A5-K-ARY and A10-OBSTRUCT remain SB; A4-HYBRID-BOUND is an upper bound after the in-text correction.
- M-lane theorem IDs/aliases: **M1-ZE (MDC-2), M1-CRIT (MDC-3), M1-HULL (MDC-4), M2-NEC (MDC-NECESSITY), M6-DIFF**. M1 claims depend on missing Fable source but integer checks reproduce; M2-NEC is ledger-model-specific and does not prove policy uniqueness; M6-DIFF proves differing parameter dependence, not universal non-reducibility.
- U-lane claim IDs: **U2-1..U2-4, U3-1..U3-3, U4-1..U4-2, U5-1..U5-3, U6-1..U6-3**. U2-U4 are synthesis claims tied to stated rows; U5-U6 are optional SB/toy algebra. None upgrades W6-DS-PHASE-MASTER to a single exact theorem.
- Lane coordinates G1-G12, M1-M10, B1-B8, A1-A10, U1-U8, E1-E12 are worker/module IDs unless an explicit theorem/claim ID above is present.

## Integer certificate inventory

All direct comparisons evaluate true in owned E11/tier scripts: 20*m^m<(m+1)^(m+1) for m=10..17; 3^5=243<256=2^8; 257*17^3=1,262,641<2,097,152=2^21; 129^2*9^8<3*128^2*8^8; 65^2*463^10<=8*64^2*400^10; 2075^2*309^12<=32*2048^2*256^12; 125<=128; 17^11<=2^45; 7^25>2^69; 71*11^4<2^20; 63^3*256>400^3; 27^7>=2^33; 53^7>=2^40; 16641*43046721<3*16384*16777216. Arithmetic truth does not prove a surrounding theorem when its bridge premise is missing.

## Foreign material and duplicates

### Byte-identical duplicate groups

- SHA 6972da96809f31c7: 40_DEEPSEEK_W6_PROVENANCE.txt = peers/DEEPSEEK_W6/00_PROVENANCE.txt.
- SHA ce08a5da2d9eb018: 42_DEEPSEEK_W6_PACKAGE.md = peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE.md = peers/DEEPSEEK_W6/swarm_lanes/RADC_WAVE6_PACKAGE.md.
- SHA 0299dabde79d4aac: 43_DEEPSEEK_W6_NOTES.md = peers/DEEPSEEK_W6/deepseekwave6.md. These are prompt/notes, not evidence.
- SHA 47bed5062746e218: ec_out/O3_AUDIT_LOG.md = ec_out/o3_spotcheck.out. Raw rerun o3_spotcheck.capture.out is shorter.

### Foreign-by-provenance

- peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md is a distinct **KimiK3-Thinking** return, not the DeepSeek-swarm canonical package. It contains 47 W6-DS strings, including later G/M/A/B/U promotions; namespace overlap is not authorship evidence. It was read but excluded from the owned theorem count.
- peers/DEEPSEEK_W6/ec-peer-reruns/fable/**, grok/**, and kimi/** are foreign checker copies/captures. They corroborate imported values but do not transfer theorem ownership.
- Fable w5c has eight ***FAIL*** markers for uniqueness ties (n=2 and multiples of 8); optimum value still matches. Grok has 22 PASS/0 FAIL. Kimi drive has 72 PASS/0 FAIL. These are foreign, not owned tier totals.

## NOT_IN_ZIP

Referenced by owned prompt/package/lanes/master but absent anywhere in the extracted Wave-7 flat root:

1. 00_RADC_FORMAL_CORE_V1_FREEZE.md; 01_README_OPEN_FIRST.txt; RADC_FORMAL_CORE_V1_1_FREEZE.md.
2. 10_SOLPRO_CONT2.md; 12_SOLPRO_CONT2_CHECKS.py; 13_SOLPRO_CONT2_CHECKS.cpp; 14_SOLPRO_CONT2_GRID.cpp; 15_SOLPRO_CONT2_CHECKS.out; 17_SOLPRO_CONT2_SHA256.txt.
3. 20_SOLPRO_CONT1.md; 21_SOLPRO_CONT1_CHECKS.py; 22_SOLPRO_CONT1_CHECKS.out.
4. 30_SOLPRO_W5_THEORY_FULL.txt; 40_FABLE_THEOREM_INDEX_EXTRACT.txt; 41_GROK_CONFLICT_MATRIX.md; 42_KIMI_W5_PACKAGE.md; 43_FABLE_W5_PACKAGE.md.
5. 51_OMEGA_FRANKENSIM_MATH_TRANSFER.md; W5_COMP_CERTIFICATES.md.

Consequence: imported PI statements, source manifests, and advertised original 12/21 checker reruns cannot be provenance-verified from this ZIP. Included peer-rerun copies are partial substitutes, not the cited originals.

## File-read ledger

Every listed payload was byte-read. Per-row READ is the attestation; aggregate byte count and duplicate SHA evidence are above.

| # | Path | Classification | Status |
|---:|---|---|---|
| 1 | 40_DEEPSEEK_W6_PROVENANCE.txt | flat duplicate/entry | READ |
| 2 | 42_DEEPSEEK_W6_PACKAGE.md | flat duplicate/entry | READ |
| 3 | 43_DEEPSEEK_W6_NOTES.md | flat duplicate/entry | READ |
| 4 | peers/DEEPSEEK_W6/00_PROVENANCE.txt | provenance duplicate | READ |
| 5 | peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE.md | owned canonical package | READ |
| 6 | peers/DEEPSEEK_W6/RADC_WAVE6_PACKAGE_KIMIK3_THINKING.md | FOREIGN KimiK3 package | READ |
| 7 | peers/DEEPSEEK_W6/checkers/o3/o3_spotcheck.py | owned checker/report | READ |
| 8 | peers/DEEPSEEK_W6/checkers/tier2/g10_lambda.py | owned checker/report | READ |
| 9 | peers/DEEPSEEK_W6/checkers/tier2/g2_spectra.py | owned checker/report | READ |
| 10 | peers/DEEPSEEK_W6/checkers/tier2/g456_q4_verify.py | owned checker/report | READ |
| 11 | peers/DEEPSEEK_W6/checkers/tier2/g7_n3_phase.py | owned checker/report | READ |
| 12 | peers/DEEPSEEK_W6/checkers/tier2/g8_n5_partial.py | owned checker/report | READ |
| 13 | peers/DEEPSEEK_W6/checkers/tier2/g9_rho_surface.py | owned checker/report | READ |
| 14 | peers/DEEPSEEK_W6/checkers/tier3/m10_certificates.py | owned checker/report | READ |
| 15 | peers/DEEPSEEK_W6/checkers/tier3/m3_separating_example.py | owned checker/report | READ |
| 16 | peers/DEEPSEEK_W6/checkers/tier3/m4_reduction_fable_to_kimi.py | owned checker/report | READ |
| 17 | peers/DEEPSEEK_W6/checkers/tier3/m5_reduction_kimi_to_fable.py | owned checker/report | READ |
| 18 | peers/DEEPSEEK_W6/checkers/tier3/m6_interaction.py | owned checker/report | READ |
| 19 | peers/DEEPSEEK_W6/checkers/tier3/m7_accounting.py | owned checker/report | READ |
| 20 | peers/DEEPSEEK_W6/checkers/tier3/m9_phase_table.py | owned checker/report | READ |
| 21 | peers/DEEPSEEK_W6/checkers/tier4/TIER4_REPORT.md | owned checker/report | READ |
| 22 | peers/DEEPSEEK_W6/checkers/tier4/a1_fano_ec.py | owned checker/report | READ |
| 23 | peers/DEEPSEEK_W6/checkers/tier4/a2_waterfill_ec.py | owned checker/report | READ |
| 24 | peers/DEEPSEEK_W6/checkers/tier4/a3_corridor_ec.py | owned checker/report | READ |
| 25 | peers/DEEPSEEK_W6/checkers/tier4/a4_hybrid_ec.py | owned checker/report | READ |
| 26 | peers/DEEPSEEK_W6/checkers/tier4/a5_decision_tv_ec.py | owned checker/report | READ |
| 27 | peers/DEEPSEEK_W6/checkers/tier4/a6_opacity_ec.py | owned checker/report | READ |
| 28 | peers/DEEPSEEK_W6/checkers/tier4/a7_rho_star_ec.py | owned checker/report | READ |
| 29 | peers/DEEPSEEK_W6/checkers/tier4/a8_phase_ec.py | owned checker/report | READ |
| 30 | peers/DEEPSEEK_W6/checkers/tier5/TIER5_REPORT.md | owned checker/report | READ |
| 31 | peers/DEEPSEEK_W6/checkers/tier5/b1_b2_antiopt_audit.py | owned checker/report | READ |
| 32 | peers/DEEPSEEK_W6/checkers/tier5/b4_frontier.c | owned checker/report | READ |
| 33 | peers/DEEPSEEK_W6/checkers/tier5/b4_n5_cells.c | owned checker/report | READ |
| 34 | peers/DEEPSEEK_W6/checkers/tier5/b4_n5_subcube.py | owned checker/report | READ |
| 35 | peers/DEEPSEEK_W6/checkers/tier5/b5_density.py | owned checker/report | READ |
| 36 | peers/DEEPSEEK_W6/checkers/tier5/b6_rhokill.py | owned checker/report | READ |
| 37 | peers/DEEPSEEK_W6/checkers/tier5/n5_all16.c | owned checker/report | READ |
| 38 | peers/DEEPSEEK_W6/checkers/tier5/n5_optclass.c | owned checker/report | READ |
| 39 | peers/DEEPSEEK_W6/deepseekwave6.md | prompt duplicate | READ |
| 40 | peers/DEEPSEEK_W6/ec-peer-reruns/REPORT.md | FOREIGN peer rerun | READ |
| 41 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5a_single.py | FOREIGN peer rerun | READ |
| 42 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5b_twodemand.py | FOREIGN peer rerun | READ |
| 43 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5c_onebit.py | FOREIGN peer rerun | READ |
| 44 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5d_q5.py | FOREIGN peer rerun | READ |
| 45 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5e_rest.py | FOREIGN peer rerun | READ |
| 46 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/checkers/w5f_final_checks.py | FOREIGN peer rerun | READ |
| 47 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/w5abcdf.out | FOREIGN peer rerun | READ |
| 48 | peers/DEEPSEEK_W6/ec-peer-reruns/fable/w5e.out | FOREIGN peer rerun | READ |
| 49 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/bp1.mine | FOREIGN peer rerun | READ |
| 50 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/checkers/run_all.sh | FOREIGN peer rerun | READ |
| 51 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/checkers/w6_bp1_agency_phase.py | FOREIGN peer rerun | READ |
| 52 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/checkers/w6_cont2_generalize.py | FOREIGN peer rerun | READ |
| 53 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/checkers/w6_mdc_separation.py | FOREIGN peer rerun | READ |
| 54 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/cont2.mine | FOREIGN peer rerun | READ |
| 55 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/grok.out | FOREIGN peer rerun | READ |
| 56 | peers/DEEPSEEK_W6/ec-peer-reruns/grok/mdc.mine | FOREIGN peer rerun | READ |
| 57 | peers/DEEPSEEK_W6/ec-peer-reruns/include/bits/stdc++.h | FOREIGN peer rerun | READ |
| 58 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/checkers/drive.py | FOREIGN peer rerun | READ |
| 59 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/checkers/mdc_dp.cpp | FOREIGN peer rerun | READ |
| 60 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/checkers/pairs.cpp | FOREIGN peer rerun | READ |
| 61 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/checkers/w5dp.cpp | FOREIGN peer rerun | READ |
| 62 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/drive.out | FOREIGN peer rerun | READ |
| 63 | peers/DEEPSEEK_W6/ec-peer-reruns/kimi/mdc_dp | FOREIGN peer rerun | READ |
| 64 | peers/DEEPSEEK_W6/ec_master_verification.py | owned broken master | READ |
| 65 | peers/DEEPSEEK_W6/ec_out/O3_AUDIT_LOG.md | owned captured output | READ |
| 66 | peers/DEEPSEEK_W6/ec_out/a1_fano_ec.out | owned captured output | READ |
| 67 | peers/DEEPSEEK_W6/ec_out/a2_waterfill_ec.out | owned captured output | READ |
| 68 | peers/DEEPSEEK_W6/ec_out/a3_corridor_ec.out | owned captured output | READ |
| 69 | peers/DEEPSEEK_W6/ec_out/a4_hybrid_ec.out | owned captured output | READ |
| 70 | peers/DEEPSEEK_W6/ec_out/a5_decision_tv_ec.out | owned captured output | READ |
| 71 | peers/DEEPSEEK_W6/ec_out/a6_opacity_ec.out | owned captured output | READ |
| 72 | peers/DEEPSEEK_W6/ec_out/a7_rho_star_ec.out | owned captured output | READ |
| 73 | peers/DEEPSEEK_W6/ec_out/a8_phase_ec.out | owned captured output | READ |
| 74 | peers/DEEPSEEK_W6/ec_out/b1_b2_antiopt_audit.out | owned captured output | READ |
| 75 | peers/DEEPSEEK_W6/ec_out/b4_frontier.out | owned captured output | READ |
| 76 | peers/DEEPSEEK_W6/ec_out/b4_n5_cells.out | owned captured output | READ |
| 77 | peers/DEEPSEEK_W6/ec_out/b4_n5_subcube.out | owned captured output | READ |
| 78 | peers/DEEPSEEK_W6/ec_out/b5_density.out | owned captured output | READ |
| 79 | peers/DEEPSEEK_W6/ec_out/b6_rhokill.out | owned captured output | READ |
| 80 | peers/DEEPSEEK_W6/ec_out/g10_lambda.out | owned captured output | READ |
| 81 | peers/DEEPSEEK_W6/ec_out/g2_spectra.out | owned captured output | READ |
| 82 | peers/DEEPSEEK_W6/ec_out/g456_q4_verify.out | owned captured output | READ |
| 83 | peers/DEEPSEEK_W6/ec_out/g7_n3_phase.out | owned captured output | READ |
| 84 | peers/DEEPSEEK_W6/ec_out/g8_n5_partial.out | owned captured output | READ |
| 85 | peers/DEEPSEEK_W6/ec_out/g9_rho_surface.out | owned captured output | READ |
| 86 | peers/DEEPSEEK_W6/ec_out/m10_certificates.out | owned captured output | READ |
| 87 | peers/DEEPSEEK_W6/ec_out/m3_separating_example.out | owned captured output | READ |
| 88 | peers/DEEPSEEK_W6/ec_out/m4_reduction_fable_to_kimi.out | owned captured output | READ |
| 89 | peers/DEEPSEEK_W6/ec_out/m5_reduction_kimi_to_fable.out | owned captured output | READ |
| 90 | peers/DEEPSEEK_W6/ec_out/m6_interaction.out | owned captured output | READ |
| 91 | peers/DEEPSEEK_W6/ec_out/m7_accounting.out | owned captured output | READ |
| 92 | peers/DEEPSEEK_W6/ec_out/m9_phase_table.out | owned captured output | READ |
| 93 | peers/DEEPSEEK_W6/ec_out/o3_spotcheck.capture.out | owned captured output | READ |
| 94 | peers/DEEPSEEK_W6/ec_out/o3_spotcheck.out | owned captured output | READ |
| 95 | peers/DEEPSEEK_W6/swarm_lanes/A1_A10_AGENCY_RD.md | owned lane/package duplicate | READ |
| 96 | peers/DEEPSEEK_W6/swarm_lanes/B1_B8_BP1_RESOLUTION.md | owned lane/package duplicate | READ |
| 97 | peers/DEEPSEEK_W6/swarm_lanes/E1_E12_EC_WORKERS.md | owned lane/package duplicate | READ |
| 98 | peers/DEEPSEEK_W6/swarm_lanes/G1_G12_CONT2_GENERALIZATION.md | owned lane/package duplicate | READ |
| 99 | peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md | owned lane/package duplicate | READ |
| 100 | peers/DEEPSEEK_W6/swarm_lanes/O1_O4_ORCHESTRATION_AUDIT.md | owned lane/package duplicate | READ |
| 101 | peers/DEEPSEEK_W6/swarm_lanes/RADC_WAVE6_PACKAGE.md | owned lane/package duplicate | READ |
| 102 | peers/DEEPSEEK_W6/swarm_lanes/README_DELIVERY.md | owned lane/package duplicate | READ |
| 103 | peers/DEEPSEEK_W6/swarm_lanes/S1_S8_SUBSTRATE_LOCK.md | owned lane/package duplicate | READ |
| 104 | peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md | owned lane/package duplicate | READ |

## Residual risks

1. Computational scripts test formulas and finite regimes; they do not prove bridge assumptions or global policy reductions.
2. Missing PI/Core/SolPro packages prevent source-level dependency verification.
3. KimiK3 and DeepSeek materials reuse W6-DS names; provenance must stay path-qualified.
4. Tier5 Python success is output reproduction without assertions.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Concrete severity-tagged findings cite canonical, lane, checker, output, runner, and rerun paths; canonical and candidate theorem registries include path/gauge/status/dependencies/integers and independent verdicts."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/9c2d6acf-a232-4cf0-a209-67fdaf416d3b/analysis-xhigh/40_deepseek_w6.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "byte-read and SHA-256 inventory of flat 40/42/43 plus peers/DEEPSEEK_W6 excluding .fszero",
      "result": "passed",
      "summary": "104 files and 812,718 bytes read; duplicate groups and one binary identified."
    },
    {
      "command": "invoke ec_master_verification.py run_E1 through run_E12 without main/output write",
      "result": "failed",
      "summary": "E1-E9 passed 328/328, E10 raised NameError: rho_kill_kimi undefined, E11 passed 21/21, E12 passed 76/76."
    },
    {
      "command": "run owned tier2/tier3/tier4/tier5 Python scripts with PYTHONDONTWRITEBYTECODE=1 and compare stdout to ec_out",
      "result": "passed",
      "summary": "6+7+8+4 scripts exited 0; every corresponding stored capture was byte-identical."
    },
    {
      "command": "compile all four tier5 C sources to a temporary directory and run",
      "result": "passed",
      "summary": "4/4 compiled and exited 0; two primary outputs matched captures; two helpers have no stored capture."
    },
    {
      "command": "test cited dependency basenames across the extracted root",
      "result": "passed",
      "summary": "19 cited Core/SolPro/W5/Omega inputs confirmed NOT_IN_ZIP."
    }
  ],
  "validationOutput": [
    "Owned tier captures reproduce independently while the current master runner breaks specifically at E10.",
    "Canonical package has 10 promoted W6-DS theorem rows; all are inventoried with independent verdicts.",
    "File-read ledger contains all 104 scoped files."
  ],
  "residualRisks": [
    "Missing cited PI/Core/SolPro files prevent provenance-level theorem verification.",
    "Finite EC does not establish unformalized reduction impossibility or global phase laws.",
    "Foreign KimiK3 material overlaps the W6-DS namespace and must remain path-qualified."
  ],
  "noStagedFiles": true,
  "diffSummary": "Read-only source audit; wrote only the requested markdown artifact.",
  "reviewFindings": [
    "high: peers/DEEPSEEK_W6/ec_master_verification.py:514 - E10 calls undefined rho_kill_kimi, so the master cannot complete.",
    "high: peers/DEEPSEEK_W6/swarm_lanes/M1_M10_MDC_RESOLUTION.md:215 - BE/SB failed reduction attempts are promoted to a DR no-reduction theorem in the canonical package.",
    "high: peers/DEEPSEEK_W6/swarm_lanes/U1_U8_MASTER_PHASE_TABLE.md:U7.2 - claimed DR matroid unification conflicts with canonical permanent separation.",
    "high: 42_DEEPSEEK_W6_PACKAGE.md:302 - canonical hybrid formula does not match the owned A4 construction and has no M/L certificate.",
    "high: peers/DEEPSEEK_W6/swarm_lanes/G1_G12_CONT2_GENERALIZATION.md:547 - stale Theta_3/Theta_5 gauges conflict with statement-locked checker gauges.",
    "medium: 42_DEEPSEEK_W6_PACKAGE.md:239 - PCOV table is marked DR+EC but contains an uncomputed n=5,m=20 cell.",
    "medium: peers/DEEPSEEK_W6/swarm_lanes/E1_E12_EC_WORKERS.md - file contains only E1-E9; 99/99 is E1, not a current E1-E12 total."
  ],
  "manualNotes": "Source payload was not edited. Execution suppressed Python bytecode and placed C binaries only in temporary directories."
}
```
