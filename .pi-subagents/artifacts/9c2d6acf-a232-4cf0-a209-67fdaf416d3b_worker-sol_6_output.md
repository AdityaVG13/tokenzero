# Wave 7 xhigh QWEN_W6 read-only audit

## Executive verdict

**Do not freeze the package as a complete P1/P5 phase result.** The core coverage, spectrum, no-message, obstruction, and n=3 anomaly arguments survive. The claimed general phase certificate has a material hole at (n=6,m=10,ldots,14): the exact barrier used by the proof is negative there, while the embedded checker defines (m_{m crit}) as the last passing point and does not test interval contiguity. P2 is only conditional ledger arithmetic, P3/P4 are not delivered, and P5 is only a partial linked-slice table.

No source files were edited. This report is the only written artifact.

## 1. Scope, file-read ledger, duplicates, checksums

Source root: `/Users/aditya/Downloads/RADC_WAVE7_THEORY_BUNDLE_EXTRACTED/wave7-attach-FLAT`.

| File | Lines | Bytes | SHA-256 | Complete-read attestation |
|---|---:|---:|---|---|
| `60_QWEN_W6_PROVENANCE.txt` | 10 | 372 | `699cfaaa0074d0c75a91019b1dba1174038d5138db682094200e3b95c0d1d1e7` | Read completely. |
| `61_QWEN_W6_PACKAGE.md` | 745 | 40,511 | `5bbdc7eeb48b1607aca67ba1170916707405a45aee070f88b83590c9151f25e1` | Read completely, including both fenced Python programs and lines 680-745 after full-value expansion. |
| `peers/QWEN_W6/00_PROVENANCE.txt` | 10 | 372 | `699cfaaa0074d0c75a91019b1dba1174038d5138db682094200e3b95c0d1d1e7` | Complete by direct read/hash and byte equality with file 60. |
| `peers/QWEN_W6/RADC_WAVE6_PACKAGE.md` | 745 | 40,511 | `5bbdc7eeb48b1607aca67ba1170916707405a45aee070f88b83590c9151f25e1` | Complete by direct hash and byte equality with file 61. |
| `peers/QWEN_W6/SHA256SUMS.txt` | 1 | 144 | `7eb4426eab25fb455701ed7af51bc3f2c8c59ca687767a5b4b2ffbd7db14224c` | Read completely. |
| `peers/QWEN_W6/radc-wave6-qwen.md` | 745 | 40,511 | `5bbdc7eeb48b1607aca67ba1170916707405a45aee070f88b83590c9151f25e1` | Complete by direct hash and byte equality with file 61. |
| `00_WAVE7_OPERATOR_MANIFEST.md` | 140 | 4,771 | `139b78a450f7cad37ef4cdf57306601196a3e86fb33a89f7ba8669bdfd8017a6` | Read completely as scope/control evidence. |
| `02_WAVE7_THEORY_CAMPAIGN.md` | 125 | 4,349 | `e90f01170c89ea1ae5e1d92088a1f782ddaf1ac7322f7db3d10087e0cedf3644` | Read completely for P1-P5 definitions. |

Duplicate groups are byte-for-byte exact (`cmp` exit 0):

1. `60_QWEN_W6_PROVENANCE.txt` = `peers/QWEN_W6/00_PROVENANCE.txt`.
2. `61_QWEN_W6_PACKAGE.md` = `peers/QWEN_W6/RADC_WAVE6_PACKAGE.md` = `peers/QWEN_W6/radc-wave6-qwen.md`.

The sole checksum entry in `peers/QWEN_W6/SHA256SUMS.txt:1` records `5bbdc7...25e1`, which matches all three package copies. Its filename is an absolute original-host path, so the manifest is not portable for direct `shasum -c`; the digest itself is valid. No checksum entries cover provenance, the alias copy, or the checksum file.

## 2. Concrete review findings

### HIGH -- P1/P5 phase certificate has an uncovered (n=6) interval

Paths: `61_QWEN_W6_PACKAGE.md:173,270,334-363,530-565` and both byte-identical peer copies.

The theorem index says the barrier is EC-positive for every small-(r) tree at every (mle m_{m crit}(n)). Exact re-execution gives the barrier predicate's passing sets:

- (n=3): (m=4,ldots,16)
- (n=4): (m=7,ldots,18)
- (n=5): (m=10,ldots,18)
- (n=6): (m=15,ldots,19)

For (n=6,r=2), the claimed lower bound is negative at every missing point:

| (m) | exact (min_r B_r(6,40,m)) | decimal |
|---:|---|---:|
| 10 | (-140251787537/7688671875) | -18.241354 |
| 11 | (-329112151843/25628906250) | -12.841444 |
| 12 | (-14427192718753/1729951171875) | -8.339653 |
| 13 | (-238800770453203/51898535156250) | -4.601301 |
| 14 | (-196518913781419/129746337890625) | -1.514639 |

The inherited small-(m) argument is stated only for (1le mle9) at `61_QWEN_W6_PACKAGE.md:363-366`; therefore (m=10,ldots,14) is uncovered. The code at lines 530-534 merely assigns `m_crit = m` whenever a point passes, so `assert ph6["m_crit"] == 19` at line 565 succeeds despite the hole. This invalidates the asserted EC positivity and leaves full-prefix dominance unproved on that interval. It is not by itself a counterexample to actual dominance.

### MEDIUM -- exact floor and infinity rows are over-attested

Paths: `61_QWEN_W6_PACKAGE.md:354-379`; dependency contrast `18_WAVE4_SOLPRO_PACKAGE_FULL.txt:1691,1698-1699`.

The package says frozen Wave-4 floors give exact (F_{n,downarrow}(40)=2+2n) for all (nge4), then labels (F_5=12), (F_6=14), and the whole master table EC. The bundled `w6_floors.py` block computes only (n=3,4). The cited Wave-4 source explicitly calls its (Phi_{n,t}) an exact **lower-bound functional** and says it is not asserted to be the exact attainable baseline floor. Thus the exact (12,14) entries lack the claimed dependency/EC. A weaker Wave-4 lower bound may still suffice for latency, so this is an attestation defect rather than a demonstrated latency counterexample.

The same EC table reports (m_{m crit}(infty)=19), while `61_QWEN_W6_PACKAGE.md:702-704,735` says arbitrary (n), a closed form for (m_{m crit}(n)), and every (nge7) count remain open/SB. Only (m_{m obstr}	o19) is proved.

### MEDIUM -- MDC theorems are conditional on absent/miscited ledgers

Paths: `61_QWEN_W6_PACKAGE.md:113-119,179-180,406-454`.

The algebra
[
Delta M=1-p_c,qquad Delta L=	frac32(1-p_c)
]
is correct if the imported Fable and Kimi ledgers are on one accounting lock. But the package cites “file 43” for Fable and “file 42” for Kimi. In this extracted flat tree, file 42 is `42_DEEPSEEK_W6_PACKAGE.md` and file 43 is `43_DEEPSEEK_W6_NOTES.md`; neither is the cited W5 ledger source. No file outside the Qwen duplicates contains `FABLE-MDC`. The EC block only substitutes the already-assumed formulas. Verdict: conditional arithmetic, not an independently attested construction or P2 rank-stratification theorem.

### MEDIUM -- claimed EC/recheck artifacts are not shipped as files

Paths: `61_QWEN_W6_PACKAGE.md:129-158,462-652,744`.

Both fenced programs execute successfully when extracted in memory, but `w6_qwen_checks.py` and `w6_floors.py` are not standalone files in the extraction. `{SCRATCH}/core_recheck.log`, the cited `21_SOLPRO_CONT1_CHECKS.py`, and its output are absent. The available renamed `12_SOLPRO_W5_CONT2_CHECKS.py` independently passes. Therefore the new Qwen arithmetic is reproducible from Markdown, but the claimed captured recheck trail is absent and Cont-1 cannot be re-attested from the stated checker.

### LOW -- bundle metadata still says Qwen is absent

Paths: `00_WAVE7_OPERATOR_MANIFEST.md:40,139`, `02_WAVE7_THEORY_CAMPAIGN.md:35`.

Those control files say `NOT_IN_TREE` / placeholder or `NOT_IN_ZIP unless present`. Qwen is present here as flat 60-61 plus four peer files. The flags are stale, not evidence of missing primary content.

## 3. Complete theorem inventory

Canonical path for every row: `61_QWEN_W6_PACKAGE.md:171-180`. The same rows occur verbatim in the two peer package copies.

Locked sequential gauge unless a row says otherwise: (Xsimmathrm{Unif}({0,1}^n)), (S_{1:m}stackrel{iid}{sim}	heta), (Theta_n^downarrow={	heta_ige4/(5n)}), heavy weights ((n+4,4,ldots,4)) over (5n), ((ho,lambda)=(40,20)), candidate ((M,L,D)=(3m+2,4,0)), (nin{3,4,5,6}). MDC additionally locks ((h,q,c_0,c_1)=(1,0,1/2,1/2)).

| Theorem ID / source status | Claim and principal integers | Dependencies | Own verdict |
|---|---|---|---|
| **W6-QWEN-COVERAGE-N** -- PROVED, DR | (P_Tle1-p_{m cov}(1-r/2^n)); (p_{m cov}ge1-[4(n-1)/(5n)]^m-(n-1)[(5n-4)/(5n)]^m). Reduces to (3/5,4/5) at (n=4). | Statement lock, complete-demand event, union bound, heavy-vertex majorization. | **ACCEPT.** Proof is self-contained. The floor may be negative for low (m), but remains a valid lower bound. |
| **W6-QWEN-SPECTRUM** -- PROVED, DR+EC | Exact (C_8,C_{16},C_{32},C_{64}); small-(r) sets (2..4,2..6,2..10,2..16). | Binary prefix-tree recurrence; embedded block 1. | **ACCEPT for (n=3..6).** Rerun exit 0; recurrence matches the model. No separate checker file. |
| **W6-QWEN-BARRIER-N** -- PROVED, DR+EC | (B_r=(m+1)C_{2^n}(r)/2^n-(2m+1)+40p_m(n)(1-r/2^n)); claimed positive through each (m_{m crit}). | COVERAGE-N + SPECTRUM. | **PARTIAL / HIGH defect.** Formula is valid. The unqualified EC-positivity statement is false; notably (n=6,m=10..14) is negative. |
| **W6-QWEN-NOMSG-N** -- PROVED, DR+EC | (P_{0,m}=2^{-n}sum_{Bsubseteq[n]}	heta(B)^m); (gamma_{0,m}=39-2m-40P_{0,m}); heavy vertex maximizes; decreasing for (mge10). | Cont-1 no-message identity, convexity/Schur-convexity, exact integer sum. | **ACCEPT.** Formula, heavy-vertex reduction, and reported exact margins rerun correctly. |
| **W6-QWEN-OBSTR** -- PROVED, DR+EC | (m_{m obstr}(n,ho)=lfloor(ho(1-2^{-n})-1)/2floor); at (ho=40): (17,18,18,19). | Fixed-prototype no-message baseline; candidate memory ledger. | **ACCEPT as a sufficient universal obstruction onset.** General proof is DR; EC only covers the registered (ho=40) rows. It is not the exact law-dependent critical point, as the package itself shows at (n=3). |
| **W6-QWEN-PHASE-N** -- PROVED, DR+EC | Reports (m_{m crit}(3,4,5,6)=(16,18,18,19)). | BARRIER-N, NOMSG-N, OBSTR, one-demand latency floor, inherited (mle9) reduction. | **PARTIAL / do not freeze as full phase.** The last-passing-point numbers rerun, but (n=6,m=10..14) is not certified. Rows (n=3..5) have no analogous gap after combining the inherited strip. |
| **W6-QWEN-N3-SHARP** -- PROVED, DR+EC | (m_{m crit}(3)=16<17=m_{m obstr}(3)); (gamma_{0,16}=845049722020265693/437893890380859375>0); (gamma_{0,17}=-22519522704133297/437893890380859375<0). | NOMSG-N + exact phase engine. | **ACCEPT under the lock.** Exact fractions and sign change rerun. |
| **W6-QWEN-MASTER** -- PROVED, DR+EC | Table gives (F=(8,10,12,14)), (m_{m obstr}=(17,18,18,19)), (m_{m crit}=(16,18,18,19)), and infinity row 19. | PHASE-N, OBSTR, W4 floor claims. | **PARTIAL / do not freeze.** Obstruction rows survive; (n=6) phase has a gap, exact (F_5,F_6) are unattested, and (m_{m crit}(infty)) is SB mislabeled EC. |
| **W6-QWEN-MDC-SEP** -- PROVED, DR+EC | Fable seq ((M,L,D)=(9-p_c,11/2-3p_c/2,0)), Kimi seq ((8,4,0)); equality iff (p_c=1); uniform (n=5) gap ((4/5,6/5)). | Imported Fable/Kimi W5 ledgers and accounting convention. | **CONDITIONAL.** Algebra verified; imported constructions and common lock are not attested by the cited files in this extraction. This is not P2 rank stratification. |
| **W6-QWEN-MDC-MECHANISM** -- PROVED, DR | Fable saving is collision mass (p_c); Kimi second demand is free by residual rank 1; no law-independent relabeling. | Same absent imported policy definitions; MDC-SEP. | **NOT ATTESTED.** Narrative follows the assumed ledgers, but no self-contained policy/rank proof or independent EC is present. |

## 4. Exact integer/rational ledger

### Prefix spectra (embedded EC rerun)

`C_8` = `[0,8,10,13,16,20,22,24]`.

`C_16` = `[0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64]`.

`C_32` = `[0,32,34,37,40,44,48,52,56,61,66,71,76,81,86,91,96,102,108,114,120,124,128,132,136,141,146,149,152,156,158,160]`.

`C_64` = `[0,64,66,69,72,76,80,84,88,93,98,103,108,113,118,123,128,134,140,146,152,158,164,170,176,182,188,194,200,206,212,218,224,231,238,245,252,259,266,273,280,285,290,295,300,305,310,315,320,326,332,338,344,348,352,356,360,365,370,373,376,380,382,384]`.

### Registered phase arithmetic

| (n) | (m_{m obstr}) | reported last passing (m) | exact (gamma_{0,m}^{downarrow}) | exact (min_r B_r) at that (m) |
|---:|---:|---:|---|---|
| 3 | 17 | 16 | (845049722020265693/437893890380859375) | (4518767325098636071/350315112304687500) |
| 4 | 18 | 18 | (277615146191/762939453125) | (11524149748246/762939453125) |
| 5 | 18 | 18 | (887975035189461090631639/582076609134674072265625) | (3023250229608618705048711/232830643653869628906250) |
| 6 | 19 | 19 | (2975301311635846283/19705225067138671875) | (4111757685339858180773/591156752014160156250) |

Additional reproduced Q4/Cont-2 values: (p_{10}=6560848/9765625); the five (m=18) small-tree barriers are (10769686/1953125), (97023471/15625000), (252888283/31250000), (38966203/3906250), (20384017/1562500); down endpoint margin (277615146191/762939453125); cap endpoint margin (20074685943080277/50000000000000000). The available Cont-2 checker also reconfirmed monotonicity at (m=10..17), tree gaps at (m=10..18), and obstruction onset at (m=19).

One-demand embedded EC proves only (F_{3,downarrow}(40)=8) and (F_{4,downarrow}(40)=10), plus (20m^m<(m+1)^{m+1}) for (m=10..25). It does not compute (F_5) or (F_6).

MDC arithmetic under the assumed ledgers: for uniform (n=(2,3,4,5,8)), (p_c=(1/2,1/3,1/4,1/5,1/8)), Fable (M=(17/2,26/3,35/4,44/5,71/8)), Kimi (M=8), (Delta M=(1/2,2/3,3/4,4/5,7/8)), and (Delta L=(3/4,1,9/8,6/5,21/16)).

## 5. P1-P5 disposition

| Target | Qwen coverage | Audit disposition |
|---|---|---|
| **P1 -- general-(n) sequential full-prefix phase** | COVERAGE-N, SPECTRUM, BARRIER-N, NOMSG-N, OBSTR, PHASE-N, N3-SHARP. | **Partial.** Strong reusable lemmas and valid (n=3) anomaly, but no complete (n=6) phase because (m=10..14) is uncovered. No arbitrary-(n) result. |
| **P2 -- MDC rank stratification** | MDC-SEP and MDC-MECHANISM compare two imported ledgers. | **Not delivered as requested.** Conditional inequivalence arithmetic only; no residual-rank master stratification, leaf-floor coincidence, or MDS triviality certificate. |
| **P3 -- BP1** | No theorem ID. Obstruction map calls it open/SB. | **Not delivered; no EC.** Honest non-claim. |
| **P4 -- agency RD / decision-TV** | No theorem ID. Restates PI (1-H_2(D)) and names a hybrid as missing. | **Not delivered; no EC.** Honest non-claim. |
| **P5 -- master phase surface** | OBSTR gives a general sufficient formula; MASTER gives one ((ho,lambda)=(40,20)) slice for (n=3..6). | **Partial.** No (ho^star(n,s,Theta)), off-slice (lambda), batch/sequential unified surface, or arbitrary-(n) (m_{m crit}); table contains the defects above. |

## 6. EC evidence and absence

Evidence obtained:

1. Both embedded Python blocks were extracted from `61_QWEN_W6_PACKAGE.md` in memory and run under Python 3. Block 1: exit 0, 17 stdout lines, final `ALL ASSERTIONS PASSED (exit 0)`. Block 2: exit 0, 5 stdout lines, final `ALL AUX ASSERTIONS PASSED (exit 0)`.
2. Independent audit calls into block 1 exposed the exact truth sets and negative (n=6) barriers above. This is evidence that the checker passes by using a last-passing-point definition, not interval certification.
3. `python3 12_SOLPRO_W5_CONT2_CHECKS.py` exits 0 and reproduces the displayed Q4 certificates.
4. Hashes, sizes, line counts, and duplicate `cmp` checks all pass.

Absent or insufficient:

- Standalone `w6_qwen_checks.py`, `w6_floors.py`, `{SCRATCH}/core_recheck.log`, and `21_SOLPRO_CONT1_CHECKS.py`.
- Intended Fable/Kimi W5 source files identified as Qwen “file 43/file 42”.
- EC for exact (F_5=12), (F_6=14), arbitrary (n), or (m_{m crit}(infty)=19).
- Any P3/P4 EC or a full P2/P5 checker.

## 7. NOT_IN_ZIP

Interpreted against the complete extracted source tree:

- **Primary QWEN_W6 is IN the extraction.** The NOT_IN_TREE/NOT_IN_ZIP metadata is stale.
- **NOT_IN_ZIP:** standalone `w6_qwen_checks.py`; standalone `w6_floors.py`; `core_recheck.log`; `21_SOLPRO_CONT1_CHECKS.py`; its claimed rerun output; the original `/Users/aditya/AI/TokenZero/docs/radc-wave6-qwen.md`; and the intended Fable/Kimi W5 ledger files cited as “file 43/file 42”.
- Present substitutes are the two fenced programs, the three byte-identical package copies, the available renamed Cont-2 checker, and high-level W5 summaries in the frozen/core material. These substitutes do not supply the missing MDC construction proof or Cont-1 recheck log.

## 8. Residual risks

- Negative (B_r) values show failure of the stated certificate, not failure of actual dominance; resolving (n=6,m=10..14) requires a sharper bound, exact policy optimization, or a counterexample.
- Exact (F_5,F_6) may happen to equal 12 and 14, but that equality is not proved or computed by the supplied Qwen evidence.
- MDC conclusions remain convention-sensitive until the original Fable/Kimi policies and ledger accounting are available.
- Duplicate equality eliminates copy drift but does not repair the mathematical/provenance gaps.