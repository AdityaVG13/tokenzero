# Wave 7 xhigh audit: KIMI_W6

## Verdict

**DO NOT FREEZE THE PACKAGE WHOLE.** The registered-gauge sequential result m_crit(n), the scoped MDC vertex certificates, and the BP1 one-bit/small-n results are substantially supported. Two promoted claims are not:

1. **CRITICAL -- W6-AGRD-DTV is false for the advertised adaptive-demand model.** The rate conditions on S_{1:m}, so adaptive choices can carry information about X for free. The converse silently uses H(X)=n after conditioning on S, and its iid occupancy formula is unavailable for adaptive S.
2. **HIGH -- W6-RHO-SURFACE/W6-MASTER-TABLE is not an exact general phase surface.** rho_tree is explicitly only sufficient-certified, so max(rho*_NM,rho_tree,rho_L) cannot simultaneously be called the exact threshold with an iff.

Own disposition: **accept-with-scope** W6-PARITY-N-INV, LEAF-OCC, NOMSG-VERTEX/Law, TREE-BARRIER-N at rho=40, GENN-PHASE at (40,20), BATCH-PHASE, scoped MDC results, BP1-E1/RED/CRUDE. **Reject** adaptive invariance/converse in AGRD-DTV. **Downgrade** RHO-SURFACE and MASTER-TABLE to sufficient/certified brackets outside exact faces and finite rows.

## Concrete findings

### F1. Critical: adaptive DTV converse and invariance fail

Paths: 31_KIMI_W6_PACKAGE.md:26-43,361-385,503-517; 37_KIMI_W6_PROOF_DEVELOPMENT.md:181-205; peers/KIMI_W6/w6/w6_genn_checks.py:980-1060.

Claimed model permits S_t to depend on transcript, recoveries, and answers, but defines rate as I(X;Z,R_{1:m}|S_{1:m}). For adaptive S, H(X|S) need not equal n and S itself is a communication channel. The displayed converse should begin with H(X|S), not H(X)=n. Also E|Q_m| = sum_i[1-(1-theta_i)^m] assumes iid fixed theta, not an arbitrary adaptive process.

Exact D=0 counterexample, n=2, m=2:

- S1=1.
- After the first answer, choose S2=1 if X1=0 and S2=2 if X1=1.
- Send R1=X1. If S2=2 send R2=X2; if S2=1 the repeated answer is already known.
- Given S, R1 is deterministic. Conditional rate is P[S2=2]*H(X2)=1/2 bit.
- Joint error is 0 and E|Q|=(1+2)/2=3/2.

The claimed converse gives R >= E|Q|=3/2, contradicted by R=1/2. The package also says the coverage-limit rate is exact at D=0 with value n=2, contradicted under its own adaptive-rate convention. Restricting demands to exogenous iid S independent of X can salvage the displayed nonadaptive bounds; alternatively charge I(X;S,Z,R), directed information, or an explicit demand-policy channel.

The GENN checker verifies only algebraic entropy/mixture identities and float tables. It cannot certify the missing information-theoretic premise.

### F2. High: the advertised rho phase surface is only sufficient

Paths: 31_KIMI_W6_PACKAGE.md:268-280,386-409; 37_KIMI_W6_PROOF_DEVELOPMENT.md:95-103,204-210.

The package defines rho*(n,m)=max(rho*_NM,rho_tree,rho_L), while the same paragraph calls rho_tree sufficient-certified. A success upper bound produces a sufficient dominance threshold, not necessity. Therefore:

- rho*_NM=(2m+1)/(1-P_down(n,m)) is exact and sharp for the no-message face.
- rho_L is an exact/certified latency face where the underlying one-demand floor is exact/certified.
- rho_tree is a certificate upper bound on the true nontrivial-tree threshold.
- Their maximum is a sufficient full-hull surface, not a proved exact iff surface at arbitrary rho.

The rho=40 m_crit classification can still be exact: positive certificates cover all trees below the boundary and the heavy-vertex no-message witness kills the next m. Do not generalize that exact registered-gauge slice to the entire rho surface.

### F3. High: the GENN checker is not runnable as packaged

Paths: peers/KIMI_W6/w6/w6_genn_checks.py:14-20,1075-1083; 31_KIMI_W6_PACKAGE.md:410-425.

The docstring says the log is written next to the script, but imports are prefixed with '/mnt/agents/output/w6' and the final write is hard-coded to '/mnt/agents/output/w6/W6_GENN_EC_LOG.md'. That directory is NOT_IN_ZIP and absent in this extracted tree. The documented command 'python3 w6_genn_checks.py' completes the expensive calculations and then raises FileNotFoundError instead of satisfying its stated exit contract. search5b.py similarly hard-codes '/mnt/agents/output/w6/search5b_results.json'.

Use a path relative to __file__ or an explicit --output argument. Until then the shipped GENN log is evidence from a prior environment, not a clean package-local reproduction.

### F4. Medium: checksum attestation fails

Path: peers/KIMI_W6/SHA256SUMS.txt:3.

Exact command 'cd peers/KIMI_W6 && sha256sum -c SHA256SUMS.txt' exits 1. Twenty-five listed files pass; SHA256SUMS.txt itself fails. Recorded self-digest is abb3533405dc3b36d38340e3ea2b159b3b471f642500ba585ac1fcde8f1c1ecc; actual digest is d3ed29cd5456893a3c68a2ead33b803de3e3b3ce912c35fd51e4c43a6df0f4a1. A conventional manifest cannot naively attest its own final bytes. Remove the self-entry or use a detached signed manifest.

### F5. Medium: stale working proof contradicts the final package

Path: 37_KIMI_W6_PROOF_DEVELOPMENT.md.

- Lines 181-205 retain the already-admitted backwards minimum 'min(n-2D,n-H2(D))=n-2D'. Because H2(D)>=2D, the minimum is n-H2(D). The final package fixes the upper bound, but the shipped proof-development dependency remains false.
- Lines 204-210 label m_crit as 14/16/18/18/19+, contradicting the final phase law m_crit(2)=empty. Fourteen is m_nm(2), the M-side no-message cutoff, not the full phase after latency.
- Lines 95-103 still state an exact iff rho surface despite rho_tree being sufficient.

The file says it feeds the final package, so these are not harmless private notes. Mark it superseded or synchronize it.

### F6. Medium: evidence for all-kink barrier remediation is not shipped as code

Paths: peers/KIMI_W6/w6/w6_genn_checks.py:568-690; 33_KIMI_W6_VERIFICATION_LOG.md:135-150; 31_KIMI_W6_PACKAGE.md:226-241.

The checker uses the full v=k/2^n grid only through n=9, then a 256-point grid. It does not contain the verification log's later all-kink pass. The verifier says it independently checked every dyadic kink for n=10..29, but that independent program is absent. The package text promotes the result as exact.

I independently reconstructed the exact Fraction calculation at all continuous piecewise-linear candidate kinks: v=2^-j, the two nearest source-grid points to v*=1-(r-1)P, and endpoints. Results:

- 6,979 candidates for n=10..29, m=3..19: global minimum 1/2 at (n,m,v,r)=(10,3,1/8,144), all positive.
- m>=9 minimum 896425198047/976562500000 = 0.917939402800128 at (10,9,1/64,144).
- m>=10 minimum 90570163249161/97656250000000 = 0.9274384716714087 at (10,10,1/128,144).

Thus this audit supports the barrier conclusion, but the zip itself lacks the advertised reproducible certificate.

### F7. Medium: cited dependencies and commands are absent or misnamed

- 12_SOLPRO_CONT2_CHECKS.py is cited by 31_KIMI_W6_PACKAGE.md and 32_KIMI_W6_plan.md but exists neither in the KIMI zip tree nor under that name in flat. Flat contains 12_SOLPRO_W5_CONT2_CHECKS.py; the KIMI tree contains W5_FULL_PREFIX_CHECKS.py.
- The original archive '~/Downloads/Kimi_Agent_RADC Wave 6 Theory Extension.zip' is absent, so 'verbatim from zip' cannot be independently compared.
- The all-n MDS conclusion depends on the external binary-MDS triviality theorem. The zip exhaustively checks r=2,3,4 only; no source or proof of the general classification is bundled.
- GENN m=1,2 and latency depend on W4/W5 one-demand floors, Fable integer chains, and a chord lemma. These are cited IDs, not self-contained KIMI_W6 proofs.

### F8. Low: numerical and terminology drift remains

- 34_KIMI_W6_GENN_EC_LOG.md:122-128 says P_band<P_down is consistent with Schur-concavity. It is consistent with Schur-convexity, as the final package correctly says.
- 31_KIMI_W6_PACKAGE.md:243-250 quotes approximate m=1,2 floor margins 2.95 and 1.91; the shipped GENN log reports exact certified minima 3 and 3.23260443. The understatements are safe but inconsistent.
- 33_KIMI_W6_VERIFICATION_LOG.md:241-260 records two residual tail micro-nits: -7.6+28.88/n <= -7.5856 is false at n=2001, and the U-versus-exp bridge is not displayed in the package. Final constants remain safe.

## Attack results by mission target

### m_crit and sequential phase

Locked gauge: rho=40, lambda=20, h=1, q=0, c0=c1=1/2. Baseline is M_T=(m+1)(1+ell)+40e_T, L_T=1+ell+c_comp+20e_T, D_T=e_T. Parity is (3m+2,0,4), n-independent.

Two different cutoffs must remain separate:

| n | m_nm, M-side no-message | full m_crit | next exact kill | own verdict |
|---:|---:|---:|---:|---|
| 2 | 14 | empty | latency identity (6,0,3) for every m; M kill 15 | sound; proof-development table wrong |
| 3 | 16 | 16 | heavy no-message m=17; universal 18 | sound; L binds with gamma_L=0 |
| 4 | 18 | 18 | m=19 | bit-exact Cont-2 reproduction |
| 5 | 18 | 18 | m=19 | supported by inherited floor plus barrier |
| >=6 | 19 | 19 | universal m=20 | supported; all-n tail has display nits only |

Exact endpoint witnesses include n=2 gamma14=1211104419/1220703125 and gamma15=-245291159/244140625; n=3 gamma16=845049722020265693/437893890380859375 and gamma17=-22519522704133297/437893890380859375; n=4 gamma18=277615146191/762939453125; n=5 gamma18=887975035189461090631639/582076609134674072265625; n=6 gamma19=2975301311635846283/19705225067138671875; n=7 gamma19=82403021704638194497022551/177482997121587371826171875.

Corrected r*(n), n=2..14: 4,5,7,11,17,28,47,81,144,257,462,839,1537. The old uniform-r barrier really fails from n=8, reaching -18.97294307 at (n,m,r)=(24,19,2062); only the leaf-aware (v,r) split rescues the theorem. That distinction is honestly recorded in the final package.

### MDC coincidence and stratification

Own verdict: **valid only at the named vertices and candidate strata.** Do not promote to class-wide equality.

- Residual-rank strata: parity U_{1,n} has n_crit=3; opaque U_{n,n} and U_{n-1,n} have n_crit=5.
- Fable-adaptive and Kimi-prototype Omega hulls coincide at five mission vertices: Q4 uniform/down/cap and Q3 down/uniform, for alpha=2,3; Q2 down is an extra kill vertex.
- They are not leaf identities. Disagreements are 35,880/65,536, 36,120/65,536, 36,264/65,536 leaves at the three Q4 vertices and 52/256 at each Q3 vertex. Witness A={000,001,010,101}: E_adapt=17, E_proto=18.
- Q4 uniform pairs: (0,176),(16,128),(33,80),(42,56),(64,0). Q4 down: (0,272),(16,182),(32,108),(64,0). Q4 cap: (0,1096),(16,776),(32,492),(64,0). Q3 down: (0,1188),(8,738),(24,0). Q3 uniform: (0,48),(8,30),(24,0).
- Kimi thresholds: 150/17, 1200/137, 96/11, 400/41, 48/5. Fable M-side thresholds: 92/11, 143/17, 94/11, 17/2.
- At n=3, F2_3(40)=12>8 and F2_2(40)=8, so gamma=(4,0). At n=2, F2_2(40)=6<8.
- Intermediate U_{r,n} linear realizability is correctly reduced to binary MDS. General all-r closure remains an external theorem dependency.

### BP1

Own verdict: **partial, correctly open beyond certified range.** The all-n one-bit optimum is proved: M_n=2^{n-1}E|n-2K| and e1=1/2-E|n-2K|/(2n), attained by majority balls. M_n for n=2..12 is 2,6,12,30,60,140,280,630,1260,2772,5544. t1(3)=8, t1(4)=32/3 and t1 asymptotic 2*sqrt(2*pi*n).

BP1-uniform itself is exact only for n=2,3,4 through V_c(Omega)=n2^{n-1}; the n=4 DP uses 21,457,825 split comparisons. The quartet A={0000,0001,1111,1110} gives 2Canc=12>3 and kills greedy induction. The n=5,6,7 runs are BE searches, about 1,460 trees each plus 40k/40k/25k annealing steps, not proofs. Weighted heavy exact first breakpoints are 20/3,8,80/9 for n=2,3,4; the all-class first-breakpoint floor >=4 is a valid crude theorem.

### DTV/hybrid

Parity-noise n-H2(D) does dominate the parity-erasure hybrid n-2D on [0,1/2]. The algebra and the nD distance between the two displayed envelopes are correct for exogenous demands. The false part is extending the converse and invariance to adaptive endogenous S while conditioning rate on S. The exact-rate 'open sliver' is therefore not even a valid bracket for the advertised adaptive model.

### Phases and gauges

- Registered sequential slice (40,20): supported as above.
- Batch parity (5,0,4), n>=3, all m: supported conditional on inherited one-demand floor; n=3 has gamma_L=0 and strict M.
- Exact no-message face: supported.
- General linked/unlinked rho surface: sufficient certificate only, not exact.
- Master table combines exact ISC/W4 rows, certified brackets, and a sufficient sequential tree face. Label each cell by exact versus bracket versus sufficient.

## ID and status inventory

| ID | package status | own status |
|---|---|---|
| W6-PARITY-N-INV | DR | accept in locked model |
| W6-LEAF-OCC | DR | accept; counting proof sound |
| W6-NOMSG-VERTEX | DR | accept; Schur-convex heavy/band reduction |
| W6-NOMSG-LAW | DR+EC | accept as M-side law; not full n=2 phase |
| W6-TREE-BARRIER-N | DR+EC | accept at rho=40 after exact kink audit; shipped reproducer incomplete |
| W6-GENN-PHASE | DR+EC | accept at (40,20), conditional on inherited floors |
| W6-BATCH-PHASE | DR+EC | accept, same dependency |
| W6-RHO-SURFACE | DR+EC | downgrade to sufficient except exact faces/rows |
| W6-MDC-STRAT | DR+EC | accept only scoped parity/opaque strata |
| W6-MDC-LEAFCOIN | EC | accept at five named vertices only |
| W6-MDC-MDS | DR+EC | mathematically plausible/correct; external all-r theorem unbundled |
| W6-BP1-E1-UNIFORM | DR | accept all n |
| W6-BP1-UNIFORM-RED | DR+EC+BE | accept reduction; exact n<=4, open n>=5 |
| W6-BP1-CRUDE | DR | accept all n/classes |
| W6-AGRD-DTV | DR+BE | reject adaptive converse/invariance; retain exogenous achievability |
| W6-MASTER-TABLE | DR+EC | partial; mixed exact/bracket/sufficient cells |

Cited inherited IDs, all dependency-only rather than reproved in this zip: W5-SOL-MDC-Q4-FULL-18-19; W5-SOL-COVERAGE-LEAF; W5-SOL-Q4-LENGTH-SPECTRUM; W5-SOL-OCCUPANCY-SCHUR; W5-SOL-AGRD-THETA; W5-SOL-RANK-AREA; W5-SOL-RCM; W5-SOL-LCC; W5-SOL-DBL; W5-SOL-OPAQUE-NCRIT; W5-SOL-MDC-ZE-M; W5-SOL-MDC-BATCH40; W5-SOL-ISC-PHASE; W4-FLOOR-Q3-DOWN; W4-FLOOR-Q4-DOWN/CAP/UNIFORM; W4-Qn-3PLUS; W4-Qn-SEPARABLE; W4-PHASE-MASTER; Fable W5-MDC-3/4/5; Kimi W5-MDC-FLOOR/BATCH/SEQ/NECESSITY; W5-BP1; W5-ANTI-OPT; W5-LPP-KILL/CERT.

Proof tags are PI=published input, DR=derived result, EC=exact computation, BE=bounded experiment, SB=speculative bridge. No theorem in the final index is tagged SB. The package's 78% affirmative,16% EC,6% disproof figures are self-reported effort estimates, not auditable measurements.

## Path and NOT_IN_ZIP inventory

### Flat duplication map

All comparisons passed byte-for-byte:

| flat path | bytes | SHA-256 | peer counterpart |
|---|---:|---|---|
| 30_KIMI_W6_PROVENANCE.txt | 372 | b6237f909c27f92c80a0f53a7a0261ad465a825ba3a2cced9841d017929d65ea | peers/KIMI_W6/00_PROVENANCE.txt |
| 31_KIMI_W6_PACKAGE.md | 41,597 | e122dd3c53d4a81214df3dc149520714aa7dd5dab6097791ec5e5734a361e40b | root and w6/RADC_WAVE6_PACKAGE.md |
| 32_KIMI_W6_plan.md | 1,286 | 8a497eef96e22557fed170344cdb34028e95b441712ff714f7ff92fc81ffc49b | peers/KIMI_W6/plan.md |
| 33_KIMI_W6_VERIFICATION_LOG.md | 18,332 | 9255c1b2c78ee854bbf2c884eaa3e742049fd7e0548a38eb42337f5ab33641c5 | w6/W6_VERIFICATION_LOG.md |
| 34_KIMI_W6_GENN_EC_LOG.md | 31,015 | 1168854917cba66525b72ff4901381ced74be9ea7d2cdee6eb64cc84054a9d13 | w6/W6_GENN_EC_LOG.md |
| 35_KIMI_W6_MDC_EC_LOG.md | 11,978 | 9404af2315aff04b14b3b44373968beb1e9822039ea65e0b64e522f90e77181f | w6/W6_MDC_EC_LOG.md |
| 36_KIMI_W6_BP1_EC_LOG.md | 13,692 | fa3c5716238863f34e7547ea2d189676840281da216ad0b4e6c1308c2c0c8b83 | w6/W6_BP1_EC_LOG.md |
| 37_KIMI_W6_PROOF_DEVELOPMENT.md | 15,896 | 6bc51ed12d5df8cb6ad1ccad835b3cfc1c523d0f5411757866ee1e12657b30f4 | w6/W6_PROOF_DEVELOPMENT.md |

### Manifest-listed w6 files read completely

Each path below was byte-read. Size and manifest digest are shown. The checksum command confirms every row except the manifest's self-entry.

| path under peers/KIMI_W6 | bytes | SHA-256 |
|---|---:|---|
| RADC_WAVE6_PACKAGE.md | 41,597 | e122dd3c...e40b |
| plan.md | 1,286 | 8a497eef...49b |
| SHA256SUMS.txt | 2,270 | actual d3ed29cd...0f4a1; recorded abb35334...c1ecc FAIL |
| w6/.bp1_verify.out | 13,708 | 30a93d47...7640 |
| w6/canc8.py | 2,191 | 6f34e604...4f87 |
| w6/heavy7.py | 5,795 | 902a5530...e40 |
| w6/heavy7b.py | 2,764 | 852cc15c...977e |
| w6/heavy7c.py | 1,790 | 9ae880bf...8565 |
| w6/RADC_WAVE6_PACKAGE.md | 41,597 | e122dd3c...e40b |
| w6/search5.py | 5,282 | 41030204...79b |
| w6/search5b_results.json | 440 | 68670304...8f1 |
| w6/search5b.log | 371 | a1c22890...473 |
| w6/search5b.py | 3,548 | 7ac197d9...1ac |
| w6/sol_m_demand_grid.cpp | 2,625 | 1ffc1243...8e6 |
| w6/w5_full_prefix_check.cpp | 5,288 | 4db03381...14c |
| w6/W5_FULL_PREFIX_CHECKS.py | 4,409 | d3b0c08e...337 |
| w6/w6_bp1_checks.py | 38,864 | 4a9fcb6d...08f |
| w6/W6_BP1_EC_LOG.md | 13,692 | fa3c5716...b83 |
| w6/w6_genn_checks.py | 50,103 | 875cec89...5f5 |
| w6/W6_GENN_EC_LOG.md | 31,015 | 11688549...d13 |
| w6/w6_lib.py | 10,344 | e67ccf5a...f09 |
| w6/w6_mdc_checks.out | 9,892 | 6761e287...b09 |
| w6/w6_mdc_checks.py | 27,090 | ede6683f...b9ba |
| w6/W6_MDC_EC_LOG.md | 11,978 | 9404af23...81f |
| w6/W6_PROOF_DEVELOPMENT.md | 15,896 | 6bc51ed1...30f4 |
| w6/W6_VERIFICATION_LOG.md | 18,332 | 9255c1b2...41c5 |

### NOT_IN_ZIP / extraction-runtime files

The SHA manifest is the available zip-membership ledger. The following are not listed:

- peers/KIMI_W6/00_PROVENANCE.txt, 372 bytes. Wave-7 wrapper, byte-identical to flat 30.
- peers/KIMI_W6/w6/__pycache__/w6_lib.cpython-312.pyc, binary snapshot, 13,691 bytes. Its embedded source path points to this extracted directory. It is generated cache, not evidence.
- peers/KIMI_W6/.tokenzero/**: gc.last (0), ledger.jsonl (989 at read), ledger.jsonl.rotation.lock (0), maintenance.last (0), maintenance.lock (0), recovery-cache.json (314), recovery-cache.json.journal (41,504 at read), recovery-cache.json.lock (6), tool-metrics.json (371), pulse/events.jsonl (1,352). These are runtime telemetry, were byte-read, and are excluded from theory evidence.
- Flat 30-37 are Wave-7 flattening copies, not entries under the KIMI manifest.
- The original zip itself is NOT_IN_ZIP/extracted root, so archive-level provenance cannot be re-attested.

## Exact EC commands and results

1. Integrity, **failed exactly once**:

    cd peers/KIMI_W6 && sha256sum -c SHA256SUMS.txt

Result: 25 OK, SHA256SUMS.txt FAILED, exit 1.

2. Frozen Cont-2 Python, **passed**:

    cd peers/KIMI_W6/w6 && PYTHONDONTWRITEBYTECODE=1 python3 W5_FULL_PREFIX_CHECKS.py

Result: C_16 spectrum, p10=6560848/9765625, B2..B6, down/cap m17/18 fractions, monotonicity, all nontrivial trees, and m>=19 obstruction all PASS.

3. Independent C++, **passed**:

    c++ -std=c++20 -O2 peers/KIMI_W6/w6/w5_full_prefix_check.cpp -o /tmp/kimiw6-audit/w5_full_prefix_check
    /tmp/kimiw6-audit/w5_full_prefix_check

Result: 'PASS independent C++ exact certificate'; m=19 obstruction -3/2.

4. Syntax/build sweep, **passed**:

    PYTHONDONTWRITEBYTECODE=1 python3 -c "import ast,pathlib; [ast.parse(p.read_text(),filename=str(p)) for p in pathlib.Path('peers/KIMI_W6/w6').glob('*.py')]"
    c++ -std=c++20 -O2 peers/KIMI_W6/w6/sol_m_demand_grid.cpp -o /tmp/kimiw6-audit/sol_m_demand_grid

All 11 Python files parsed; both C++ files compiled.

5. Shipped GENN command, **not rerun to completion because its output contract is broken**:

    cd peers/KIMI_W6/w6 && python3 w6_genn_checks.py

Static result: hard-coded final write to missing /mnt/agents/output/w6/W6_GENN_EC_LOG.md. Shipped log reports 193 PASS,0 FAIL,3 documented anomalies in 74.6s. A wrapper attempt redirected only that open to /tmp but the execution host terminated the long cell before completion; no pass is claimed from that attempt.

6. Shipped MDC command, log-attested but not rerun due 594.8s cost:

    cd peers/KIMI_W6/w6 && PYTHONDONTWRITEBYTECODE=1 python3 w6_mdc_checks.py
    # serial fallback: python3 w6_mdc_checks.py --serial

w6_mdc_checks.out contains 66 PASS,0 FAIL. W6_MDC_EC_LOG.md reports 66/66.

7. Shipped BP1 command, log-attested but not rerun due 196s cost:

    cd peers/KIMI_W6/w6 && PYTHONDONTWRITEBYTECODE=1 python3 w6_bp1_checks.py > /tmp/kimiw6-audit/W6_BP1_EC_LOG.md

The bundled log reports all PASS and 196s; .bp1_verify.out is a separate 197-line run with runtime-only differences.

8. Exact barrier candidate audit, **passed**. The audit used Fraction formulas imported from w6_lib.py, enumerated n=10..29 and m=3..19, evaluated endpoints, every v=2^-j, and floor/ceil grid neighbors of v*=1-(r-1)P at r=r*(n). Output: candidate_count=6979; global_min=1/2 at (10,3,128,144); positive=True. A second exact pass produced the m>=9 and m>=10 minima quoted in F6.

9. Duplicate validation, **passed**:

    cmp -s 30_KIMI_W6_PROVENANCE.txt peers/KIMI_W6/00_PROVENANCE.txt && ... && cmp -s 37_KIMI_W6_PROOF_DEVELOPMENT.md peers/KIMI_W6/w6/W6_PROOF_DEVELOPMENT.md

Result: DUPLICATES_BYTE_IDENTICAL for all flat 30-37 mappings.

## Residual risks

- The exact all-r binary-MDS dependency is not bundled.
- MDC equality is vertex-only; the polytope-uniform problem remains open.
- BP1 exact all-n and weighted sharp BP1 remain open; searches are BE.
- GENN tree-barrier all-kink support is independently reconstructed here, but the package should ship that checker path.
- The current extracted tree contains runtime metadata not in the source manifest; archive provenance is unavailable.