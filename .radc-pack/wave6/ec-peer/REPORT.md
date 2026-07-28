# EC Peer Re-Run Report -- RADC Wave 6 (Fable W5 / Kimi W5 / Grok W6 checkers)

Date: 2026-07-27. Host: macOS arm64, Apple clang via `g++`, python3.
Scratch: `/Users/aditya/AI/TokenZero/.radc-pack/wave6/ec-peer/`.
Checker sources NOT modified; copies used. Full stdout preserved:
`fable/w5abcdf.out`, `fable/w5e.out`, `kimi/drive.out`, `grok/grok.out`.

## 0. Manifest verification

- FABLE `SHA256SUMS.txt`: `shasum -a 256 -c` -> all 7 entries OK (6 checkers + splice package md).
- KIMI `SHA256SUMS.txt`: all 16 entries OK (checkers, prebuilt binaries, docs).
- GROK_W6: no SHA256SUMS manifest present in package.

## 1. Fable W5 (python3, no args, cwd = copy of checkers dir)

| checker | run | verdict |
|---|---|---|
| w5a_single.py | OK, 26.1s | no PASS/FAIL lines; all claims reproduced |
| w5b_twodemand.py | OK, 43.1s | no PASS/FAIL lines; floors reproduced |
| w5c_onebit.py | OK, 0.04s | 8 lines `***FAIL***` (see flag F1); summary `UNIQUE ... : False` |
| w5d_q5.py | OK, 0.42s | no PASS/FAIL lines; all kills reproduced |
| w5e_rest.py | OK, 5.3s | no PASS/FAIL lines; all certs True |
| w5f_final_checks.py | OK, 0.03s | all `True` |

Key integers/fractions:

- w5a: Q4 split-comparison count per run `21457825` (matches W4). Q4 cap G at t=40 `128` -> F=14/5.
  Envelopes (supported pairs (Ltot,Etot), breakpoints):
  - Q4 cap: `[(0,80),(16,48),(32,28),(64,0)]`, bps `10, 16, 160/7`; F(40)=10; least t F>=8: `40/3`.
  - Q4 down: `[(0,40),(16,22),(32,12),(64,0)]`, bps `80/9, 16, 80/3`; F(40)=10; least t F>=8: `160/11`.
  - Q4 unif: `[(0,32),(16,20),(32,12),(42,8),(64,0)]`, bps `32/3, 16, 20, 22`; F(40)=10; least t F>=8: `64/5`.
  - Q3 down: `[(0,60),(8,30),(15,16),(24,0)]`, bps `8, 15, 135/8`; F(40)=8.
  - Q3 unif: `[(0,12),(8,6),(24,0)]`, bps `8, 16`; F(40)=8.
- w5b (two-demand sequential): Q4 unif pairs `[(0,176),(16,128),(33,80),(42,56),(64,0)]` (alpha=3 bps `92/11`, `80/11` thresholds);
  Q4 down pairs `[(0,272),(16,182),(32,108),(64,0)]`; Q3 down `[(0,1188),(8,738),(24,0)]`; Q3 unif `[(0,48),(8,30),(24,0)]`.
  Candidate M2/L2 at vertices: Q4unif `p_c=1/4 M2=35/4 L2=41/8`; Q4down `p_c=7/25 M2=218/25 L2=127/25`;
  Q3down `p_c=9/25 M2=216/25 L2=124/25`; Q3unif `p_c=1/3 M2=26/3 L2=5`.
- w5c: e_anti values n=3..8: `1/4, 11/40, 121/400, 5/16, 145/448, 43/128` (all match W4).
  g_anti == g_best for every n=2..60 (antipodal always A class optimum).
- w5d: Q5 1-bit optimum `E=242, e1=121/400`, pair `(0,31)` antipodal; 16 optimal pairs (all complement pairs).
  Kill line hits 8 at `t=1600/121=13.2231`. 2-bit optimum `E=160, e2=1/5`, quad `(0,1,14,15)`; kill extends to `t=10`.
  Mixed (1+2+2) values at t in {1600/121, 27/2, 14, 29/2, 15}: `16233/1936, 13527/1600, 429/50, 13929/1600, 1413/160` all >= 8.
- w5e: caterpillar (1,2,3,3) at Q5: t=1600/121 -> `8263/968=8.53616>=8 (no kill)`; t=14 -> `3477/400=8.69250>=8`.
  rho_cert(5) numeric `17.577411`; Psi5(17.55)=7.99420 < 8, Psi5(17.6)=8.00477 > 8, Psi5(18)=8.08838.
  Big-int certs all True: `7^25 > 2^69` (1341068619663964900807 > 590295810358705651712), `3^25 > 2^39`,
  `71*11^4=1039511 < 1048576`, `257*17^3=1262641 < 2^21=2097152`, `129^2*9^8=716340484161 < 824633720832`, `3^5 < 2^8`,
  `463^4=45954068161 <= 2*400^4=51200000000`, `63^3*256=64012032 > 64000000=400^3`.
  Split-gain density audit: max split-gain density at |A|=2, NOT the full set, for all 5 vertices; s1 values `2, 9/8, 3/4, 15/4, 3/4`
  do not equal the max density (bound not tight).
- w5f: Delta formula exact all n 3..40; tie law: final-step tie iff `8|n`, never a third tied class; b=0 strictly worse.
  Certs: `65^2*463^10 <= 8*64^2*400^10`, `2075^2*309^12 <= 32*2048^2*256^12`, `27^7>=2^33`, `53^7>=2^40`,
  `125<=128`, `17^11=34271896307633 <= 2^45=35184372088832`, n>=8 tail `20/11+28/3 = 11.1515... >= 9`,
  `16641*43046721=716340484161 < 3*16384*16777216=824633720832`, `243<256`.
  p_c vs (9-2n)/3: n=3 `9/25 < 1` False; n=4 `7/25 < 1/3` False; n=5 `29/125 >= -1/3` True; n=6 `1/5 >= -1` True.

### Flag F1 (for audit) -- w5c "***FAIL***" lines
w5c prints `***FAIL***` at n = 2, 8, 16, 24, 32, 40, 48, 56, each with `gap_to_runnerup = 0` and TWO argmin classes,
e.g. `n=8: [(1,6),(1,7)] g_anti=g_best=3440, e1=43/128`. Final line:
`antipodal (b,m)=(1,n-1) is the UNIQUE class optimum for all n in 2..60: False`.
In every FAIL row g_anti == g_best, so the antipodal VALUE is always optimal; only UNIQUENESS fails.
This is consistent with w5f's own tie law (tie iff 8|n for n 3..40; n=2 is an extra small-case tie) and with
Kimi's pairs.cpp at n=8: `cntMin=1024 cntAntipodalType=128 cntNonComplementTie=896` (896 non-complement ties).
So the "***FAIL***" markers are the checker correctly reporting a tie law, not an error in the run.

## 2. Kimi W5

Toolchain note: macOS clang has no libstdc++ `bits/stdc++.h`; compiled unmodified sources with a scratch aggregate
shim header (`ec-peer/include/bits/stdc++.h`, standard includes only) via `-I`. No source edits.
`/tmp/w5/` pre-existed from an earlier session (drive.py hardcodes `/tmp/w5/{w5dp,pairs,results.txt}`); freshly
compiled binaries were placed there.

| checker | compile/run | verdict |
|---|---|---|
| w5dp.cpp | compiled OK (~0.9s) | exercised via drive.py; all outputs match |
| pairs.cpp | compiled OK (~0.7s) | exercised via drive.py A4/A5; counts OK |
| mdc_dp.cpp | compiled OK (~0.7s), ran standalone | see below |
| drive.py | ran OK, 5.6s | ALL PASS; subtotals A2 PASS=33 FAIL=0; A3 PASS=6 FAIL=0; A4/A5 PASS=19 FAIL=0; Cross-check PASS=10 FAIL=0; B PASS=4 FAIL=0. Total 72 PASS, 0 FAIL. |

drive.py key results:
- A1: split comparisons `21457825` == closed form, PASS.
- A2 envelopes (D,E) + breakpoints exactly as claimed for Theta4cap, Theta4down, Q4unif, Theta3down (see w5a list above;
  identical numbers). Variable-length witnesses certified: Q4unif `D=42 -> ell=21/8`, Theta3down `D=15 -> ell=15/8`.
  F-checks: cap `F(10)=7, F(16)=44/5, F(40/3)=8, F(120/7)=9, F(160/7)=10, F(40)=10`; down `F(40)=10`;
  unif `F(20)=39/4, F(40)=10`; 3down `F(40)=8`. Envelope completeness at every breakpoint PASS.
- A3: e_anti n=3..8 match table (1/4, 11/40, 121/400, 5/16, 145/448, 43/128); n=9..20 computed
  (n=20: `1222699/3276800 = 0.373138`).
- A4/A5 (pairs.cpp): n=5: `Nstrict=496 Emin=242 Eanti=242` (A4 PASS). All n=3..8: ANTIPODAL EXACT OPTIMUM,
  counts OK, table PASS. n=8: `cntMin=1024, cntAntipodalType=128, cntNonComplementTie=896` (ties; see flag F1).
- A6: `257*17^3 = 1262641 < 2^21 = 2097152` PASS.
- Cross-check py_dp vs C++: 10/10 PASS incl. Theta4cap t=40/3 `G*3=288 F=8` splits `21457825`,
  two-demand Theta4down t=40 `F2_batch=10`.
- B7: F2_batch@Theta4down pairs `[(0,272),(16,182),(32,108),(64,0)]` bps `80/9, 400/37, 400/27`, `F2_batch(40)=10` PASS;
  F2_batch@Theta4cap pairs `[(0,1096),(16,776),(32,492),(64,0)]` bps `10, 800/71, 1600/123`, `F2_batch(40)=10` PASS.
- B8: G2@Theta4down bps `40/3, 600/37, 200/9`, `G2(40)=15` PASS; H2 `H2(40)=10` PASS.

mdc_dp.cpp standalone (args: vertex k tn td):
- `0 2 40 1` -> `val=3200 ellnum=64 e2num=0 S=25 base=2`  (floor = 2 + 3200/(16*25) = 10; matches Kimi F2(40)=10)
- `1 2 40 1` -> `val=12800 ellnum=64 e2num=0 S=100 base=2` (floor = 2 + 12800/1600 = 10)
- `0 3 40 1` -> `val=4800 ellnum=64 e2num=0 S=25 base=3`  (floor = 3 + 4800/400 = 15; matches G2(40)=15)
- `1 3 40 1` -> `val=19200 ... base=3` (floor = 3+12 = 15)
- `0 2 10 1` -> `val=2620 ellnum=16 e2num=182 S=25 base=2` (mixed witness: ell=1, e2=182/400=91/200; matches the ell=1 line)

## 3. Grok W6

| checker | run | vs stored ec_out |
|---|---|---|
| w6_cont2_generalize.py | OK, 0.03s, exit 0 | `diff -B` vs `ec_out/ec_cont2_gen.out`: IDENTICAL |
| w6_mdc_separation.py | OK, 0.03s, exit 0 | `diff -B` vs `ec_out/ec_mdc.out`: IDENTICAL |
| w6_bp1_agency_phase.py | OK, 0.02s, exit 0 | `diff -B` vs `ec_out/ec_bp1_agency.out`: IDENTICAL |

22 PASS lines, 0 FAIL. Key integers:
- cont2: C8 `[0,8,10,13,16,20,22,24]`; C16 `[0,16,18,21,24,28,32,36,40,45,50,53,56,60,62,64]`; C32[-1]=160.
  m_fail(n,40) = {2:15,3:18,4:19,5:19,6:20,7:20,8:20}. p10 = 6560848/9765625.
  m=18 margins: down `277615146191/762939453125`, cap `20074685943080277/50000000000000000`; m=19 obstruction `-3/2`.
  n=3 vertex: m=17 gap `-22519522704133297/437893890380859375` (<0); exact positive-margin max m=16.
- mdc: Fable (M,L) uniform n=4 `(35/4, 41/8)` vs Kimi batch `(5,4)` seq `(8,4)`; vertex p_c=`7/25`, Fable L=`127/25`>5.
  n_crit=5. Expands: unif4 `7/4 > 1`, vert4 `43/25 > 1`. `16641*43046721 < 3*16384*16777216`. Margins (5,0,1)/(7,0,1).
- bp1: e_anti n=2..15 (n=13 `95467/266240`, n=15 `148887/409600`); t1_conj n=4 `80/9`, n=5 `800/79`.
  phase table rows [(3,18,9,35),(4,19,9,38),(5,19,10,39),(6,20,10,39),(8,20,10,40)].
  rho* samples Q3u=16, Q3d=135/8, Q4u=64/5, Q4d=160/11; Kimi batch rho* down=150/17, cap=1200/137.

Note: `run_all.sh` also invokes `wave6-attach-FLAT/12_SOLPRO_CONT2_CHECKS.py`, which is outside the three assigned
checkers; its expected output is stored as `ec_out/cont2_checks.out` (not re-run here, not part of the task list).

## 4. Mismatches and notes

- No hash mismatches (Fable 7/7, Kimi 16/16 manifest entries OK).
- No output mismatches vs Grok stored `ec_out` (all three identical mod trailing blank lines).
- Fable w5c/w5a/w5b/w5d/w5e print no machine PASS/FAIL tokens; verdicts above are from reproducing the printed numbers.
- Only FAIL-tokens anywhere: w5c's 8 designed `***FAIL***` tie rows + `UNIQUE...: False` summary (flag F1), and
  drive.py's five `FAIL=0` subtotal lines.
- Wall time: Fable ~75s total (w5a 26.1 + w5b 43.1 + w5c 0.04 + w5d 0.42 + w5e 5.3 + w5f 0.03);
  Kimi compile ~2.3s + drive 5.6s + mdc_dp <1s; Grok <0.1s. Total ~85s compute.
