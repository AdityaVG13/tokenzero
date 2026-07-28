# KIMI_W6 (Max) read-only peer audit

**Independent verdict proposal: ERRORS-FOUND / MAJOR REVISION.** The sequential general-`n`, MDC/PARITY-DUAL, and proved BP1 pieces are substantially supported. `W6-AGRD-DTV` is false as stated for adaptive demands under its own conditional-rate definition. The all-`n` no-message tail also has a displayed proof gap for `n > 2000`. Source files were not edited.

## 1. Concrete review findings

1. **HIGH -- adaptive-agency converse and “adaptivity invariance” are false.** `31_KIMI_W6_PACKAGE.md:363-387` and identical `peers/KIMI_W6/RADC_WAVE6_PACKAGE.md` / `peers/KIMI_W6/w6/RADC_WAVE6_PACKAGE.md` define adaptive `S_t` but charge `I(X;Z,R_{1:m}|S_{1:m})`. Once future demands depend on past answers, `S` is `X`-dependent and becomes an uncharged side channel. Concrete `n=2,m=2,D=0` counterexample: `S1=1`, `R1=X1`, `S2=1` if `R1=0` else `2`, `R2=X_{S2}`. Every answer is correct; `S2` reveals `R1`, so `I(X;R1,R2|S)=1/2`, while `E|Q|=3/2`; the claimed lower bound at `D=0` is violated by exactly `1`. Repair: require exogenous demands `S ⟂ X` (including no answer-dependent selection), or charge `I(X;S,Z,R)`. The parity-noise achievability remains valid.
2. **MEDIUM -- the `n>2000` tail chain does not bound the stated majorant.** `31_KIMI_W6_PACKAGE.md:421-430` defines `M(n)` with `U=(1-s+s²/2)^16 >= e^{-t}`, then bounds the smaller `(1+e^{-t})^{n-1}` term and calls the result a bound on `M(n)`. This implication is invalid. `33_KIMI_W6_VERIFICATION_LOG.md:final closure pass` itself records the missing bridge `U <= exp(-t+t²/16)` and a slightly changed constant, but that lemma is absent from the package. Thus `m_crit(n)=19` for every `n>2000` is true-looking but not proved by the displayed package. Add the `U` upper bound and redo the exponent, or certify a direct monotonic bound.
3. **MEDIUM -- stale proof-development artifact contradicts the delivered agency theorem.** `37_KIMI_W6_PROOF_DEVELOPMENT.md:180-201` and its peer copy still assert `min(n-2D,n-H2(D))=n-2D`; `34_KIMI_W6_GENN_EC_LOG.md:480-490` correctly says this is backwards and the package uses `n-H2(D)`. The file is labeled working notes, but the return contains two incompatible claim versions.
4. **MEDIUM -- advertised GEN-N reproduction command is not relocatable in this archive.** `peers/KIMI_W6/w6/w6_genn_checks.py:17,1074` hard-code `/mnt/agents/output/w6`; that directory is absent here. `python3 w6_genn_checks.py` therefore cannot satisfy the log's promise to rewrite the local log and exit 0 without recreating the original environment. `search5b.py:100` has the same issue for its JSON output.
5. **LOW -- `W6-RHO-SURFACE` / master surface is not a sharp full phase surface away from the registered gauge.** `31_KIMI_W6_PACKAGE.md:271-282,388-409` calls `rho_tree` “sufficient-certified”; only the no-message face is sharp. The `rho=40` iff phase is supported, but the general `rho*(n,m)` should be labeled a certified sufficient envelope unless tree necessity is proved.
6. **LOW -- integrity manifest fails its own check.** `peers/KIMI_W6/SHA256SUMS.txt` includes a stale/self-referential digest for itself. `sha256sum -c SHA256SUMS.txt` reports `./SHA256SUMS.txt: FAILED`; the other 25 listed entries pass. `00_PROVENANCE.txt` is not listed.

## 2. Locks, gauges, and key integers

Locked model (`31_KIMI_W6_PACKAGE.md:77-114`): `X~Unif({0,1}^n)`, iid exogenous demand in the main RADC phase, `Theta_down_n: theta_i>=4/(5n)`, heavy vertex `((n+4),4,...,4)/(5n)`, linked `(rho,lambda)=(40,20)`, costs `(h,q,c0,c1)=(1,0,1/2,1/2)`, and dominance weak in all `(M,D,L)` coordinates with at least one strict. Sequential baseline is `M_T=(m+1)(1+ell)+40e_T`, `L_T=1+ell+c_comp+20e_T`, `D_T=e_T`; parity is `(3m+2,0,4)`.

Key phase integers: `m_nm(n)=14,16,18,18,19` for `n=2,3,4,5,>=6`; sequential `m_crit(n)=empty,16,18,18,19`; kills `15,17(vertex)/18(universal),19,19,20`; `r*(n)=4,5,7,11,17,28,47,81,144,257,462,839,1537` for `n=2..14`; BP1 `t1(3)=8`, `t1(4)=32/3`; MDC critical dimensions parity `3`, opaque `5`; MDS exhaustive maxima `(r,max n)=(2,3),(3,4),(4,5)`.

## 3. New theorem / proposition inventory

All rows are stated in `31_KIMI_W6_PACKAGE.md:132-152`; byte-identical statements occur at `peers/KIMI_W6/RADC_WAVE6_PACKAGE.md:132-152` and `peers/KIMI_W6/w6/RADC_WAVE6_PACKAGE.md:132-152`. Development proofs are in `37_KIMI_W6_PROOF_DEVELOPMENT.md` and `peers/KIMI_W6/w6/W6_PROOF_DEVELOPMENT.md`.

| ID | One-line claim shape | Status | Gauge / dependencies / key integers | Audit verdict |
|---|---|---|---|---|
| `W6-PARITY-N-INV` | Sequential parity ledger is `(3m+2,0,4)` for every `n`. | DR | Registered costs; `W5-SOL-RANK-AREA`; rank `r_K(Q)=1`. | **Accept.** Algebra and `D=0` fiber argument are direct. |
| `W6-LEAF-OCC` | `P_T <= E[min(1,r 2^{-|Q|})] <= r P_0m`, with `E[2^{-|Q|}]=P_0m`. | DR | Any deterministic prefix tree; randomized hull by averaging. | **Accept.** Inclusion-exclusion identity and counting proof are sound. |
| `W6-NOMSG-VERTEX` | Schur reduction gives heavy/band closed forms and gap monotonicity for `m>=10`. | DR | `gamma=39-2m-40P`; integer certificate `20m^m<(m+1)^{m+1}`. | **Accept with tail dependency.** Finite cells exact; all-`n` tail inherits Finding 2. |
| `W6-NOMSG-LAW` | Exact no-message law `14/16/18/18/19` and sharp `rho*_NM=(2m+1)/(1-P_down)`. | DR+EC | `(40,20)`; endpoint fractions; tail majorant. | **Accept through n=2000; conditional beyond** pending displayed repair. |
| `W6-TREE-BARRIER-N` | Every nontrivial tree has `Gamma_T>0` for `3<=m<=19`, all `n`. | DR+EC | `W6-LEAF-OCC`; corrected `C_N(r)=N+E_{r-1}`; exact `(v,r)` split; analytic cover `n>=15`. | **Accept.** Exact form covers `n=3..24`, analytic form covers `n>=15`; overlap closes all `n`. |
| `W6-GENN-PHASE` | Parity dominates the complete sequential prefix hull iff `m<=m_crit(n)`. | DR+EC | `m_crit=empty,16,18,18,19`; floor reductions for `m=1..9`; no-message + tree barriers. | **Accept except formal n>2000 tail gap.** n=2 latency kill `(6,0,3)` is sound. |
| `W6-BATCH-PHASE` | Batch parity `(5,0,4)` dominates full batch hull for all `m>=1,n>=3`. | DR+EC | `F_down_n(40)>=8`; n=3 has `gamma_L=0`, `gamma_M=3`. | **Accept.** Strictness is supplied by M at n=3. |
| `W6-RHO-SURFACE` | Linked threshold is max of no-message, tree, latency surfaces; unlinked M/L region. | DR+EC | `rho_L=135/8,160/11`; m=2 thresholds `150/17,1200/137,96/11,400/41,48/5`. | **Accept only as certified envelope plus exact rho=40 slice**, not globally sharp. |
| `W6-MDC-STRAT` | Two-demand critical dimension is `3` for parity `U1`, `5` for opaque `Un`. | DR+EC | `MDC-KIMI/PARITY-DUAL` vs `MDC-FABLE`; rank-area; `(40,20)`. | **Accept.** Candidate classes are explicitly disjoint; separation is real, ledger reduction common. |
| `W6-MDC-LEAFCOIN` | Adaptive-Fable and prototype-Kimi floors coincide at five computed vertices. | EC | Q4 uniform/down/cap, Q3 down/uniform; alpha `2,3`. | **Accept in stated finite scope only.** Leaf laws differ: witness errors `17<18`; no general identity. |
| `W6-MDC-MDS` | Binary-linear `U_{r,n}` realizable iff `r in {1,n-1,n}`. | DR+EC | Binary MDS triviality; exhaustive `r=2,3,4` gives max `n=3,4,5`. | **Accept.** Standard binary MDS classification plus finite checks. |
| `W6-BP1-E1-UNIFORM` | Majority ball is one-bit optimal and `e1=1/2-E|n-2K|/(2n)`. | DR | `M_n=2^{n-1}E|n-2K|`; `t1~2sqrt(2pi n)`. | **Accept.** Max-swap proof is exact. |
| `W6-BP1-UNIFORM-RED` | BP1-uniform iff `V_c*(Omega)=n2^{n-1}`; true for `n<=4`. | DR+EC+BE | `21,457,825` n=4 splits; quartet `2Canc=12>3`; search n=5..7. | **Accept reduction and n<=4 only.** Explicitly OPEN for `n>=5`; BE is not proof. |
| `W6-BP1-CRUDE` | Every class/all `n` has first breakpoint at least `4`. | DR | Weighted cancellation induction; `F(t)=2+t/2` on `[0,4]`. | **Accept.** The coordinatewise inequality closes the induction. |
| `W6-AGRD-DTV` | Finite joint decision-TV bounds, m-invariance, and adaptivity invariance. | DR+BE | `n(1-D)-H2(D) <= R <= n-H2(D)` in coverage limit; gap `nD`. | **Reject as stated.** Finding 1 refutes the converse/adaptive clause. Exogenous-demand version remains plausible. |
| `W6-MASTER-TABLE` | Combines ISC, one-demand prefix, and sequential-m phase references. | DR+EC | ISC limit cubic; exact n<=4 values `16,135/8,64/5,40/3,160/11`; sequential rho=40 phase. | **Accept as reference table**, with Findings 2 and 5 qualifications. |

### Other labeled objects and scoped candidate IDs

- `D1` occupancy law, `D2` leaf-occupancy bound, `D3` length spectrum, `D4` residual-rank stratification, `D5` agency model, `D6` band vertex: `31_KIMI_W6_PACKAGE.md:92-106`; statuses inherit their theorem rows.
- `PARITY-DUAL` / `MDC-KIMI-*`: rank-one `U_{1,n}` candidate, `(8,0,4)` at `m=2`, critical dimension `3`; `31_KIMI_W6_PACKAGE.md:283-315`. **Accept** in the locked `(40,20)` model.
- `MDC-FABLE-*`: opaque `U_{n,n}` coordinate-expansion candidate, critical dimension `5`; same path. **Accept only in opaque/residual-rank>=2 scope.**
- `MDC-NECESSITY`: opaque-class latency necessity (`L>=11/2`); same path. **Accepted as inherited**, not independently reproved in this return.
- `BP1` / `W6-BP1` umbrella: exact one-bit law, finite-DP reduction, crude floor; all-`n` sharp conjecture remains open.

### Referenced predecessor IDs

These are dependencies, not new results. Their full statements/proofs are not present in the audited KIMI return, so the shapes below are only the uses explicitly disclosed by `31_KIMI_W6_PACKAGE.md:153-159` and proof citations: `W5-SOL-MDC-Q4-FULL-18-19` (frozen Q4 full-prefix m=18 success/m=19 kill); `W5-SOL-COVERAGE-LEAF` (coverage leaf success bound); `W5-SOL-Q4-LENGTH-SPECTRUM` (Q4 tree spectrum); `W5-SOL-OCCUPANCY-SCHUR` (occupancy Schur extremum); `W5-SOL-AGRD-THETA` (agency demand substrate); `W5-SOL-RANK-AREA` (linear-handle ledger); `W5-SOL-RCM`, `W5-SOL-LCC` (residual-rank/closure machinery); `W5-SOL-DBL` (decision-bound substrate); `W5-SOL-OPAQUE-NCRIT` (opaque critical dimension); `W5-SOL-MDC-ZE-M` and `W5-SOL-MDC-BATCH40` (MDC zero-error/batch ledgers); `W5-SOL-ISC-PHASE` (ISC phase); `W4-FLOOR-Q3-DOWN`, `W4-FLOOR-Q4-DOWN`, `W4-FLOOR-Q4-CAP`, `W4-FLOOR-Q4-UNIFORM` (finite floor values); `W4-Qn-3PLUS`, `W4-Qn-SEPARABLE`, `W4-PHASE-MASTER` (general-n obstruction/separability/master phase); `W5-MDC-3`, `W5-MDC-4`, `W5-MDC-5`, `W5-MDC-FLOOR`, `W5-MDC-BATCH`, `W5-MDC-SEQ`, `W5-MDC-NECESSITY` (dual-track MDC substrate); `W5-BP1` (prior breakpoint conjecture); `W5-ANTI-OPT` (anti-coordinate optimum); `W5-LPP-KILL`, `W5-LPP-CERT` (finite-prefix kill/certificate); and `W4-DA-RATE` (agency rate landmark, cited only in stale proof notes). **Audit status: dependency-only / NOT independently assessable from this return.**

No additional formally labeled theorem, lemma, or proposition IDs were found.

## 4. Exact EC commands and recorded outputs

Commands are quoted from the artifacts. They were not rerun because GEN-N rewrites a log and the task is read-only; long-run results below are archive-recorded, then cross-checked against raw output files.

| Command | Exact recorded output / status | Audit note |
|---|---|---|
| `python3 W5_FULL_PREFIX_CHECKS.py` | PASS all 7 groups; exit `0`. | Frozen Cont-2 baseline (`33_KIMI_W6_VERIFICATION_LOG.md:11-18`). |
| `g++ -std=c++20 -O2 w5_full_prefix_check.cpp` then `./w5_full_prefix_check` | `independent C++ exact certificate`; identical fractions; exit `0`. | Compiler flags are exact as recorded; output binary name follows source/checker naming. |
| `python3 w6_genn_checks.py` | `193 PASS, 0 FAIL, 3 anomalies`; runtime `74.6s`; exit `0`. | `34_KIMI_W6_GENN_EC_LOG.md:480-490`. In this extraction, hard-coded `/mnt/...` prevents faithful local rerun. |
| `python3 w6_mdc_checks.py` | raw `SUMMARY: 66 checks, 66 passed, 0 failed (wall time 594.8s)`; exit `0`. | `peers/KIMI_W6/w6/w6_mdc_checks.out:175-179`; verification rerun reports `1324s`. `--serial` is documented fallback. |
| `python3 w6_bp1_checks.py` | packaged log `Total runtime: 196s`; verification capture `.bp1_verify.out` says `Total runtime: 461s`, `BP1_EXIT=0`; all sections PASS. | Both outputs are preserved and differ only by rerun runtime. |

GEN-N's three self-reported anomalies were corrections, not failed assertions: false naive spectrum (`C_7(4)=12`, not `14`), original Task-5 barrier failure for `n>=8` repaired by `(v,r)` split, and Task-8 min-direction correction. MDC reports no anomalies. BP1 explicitly distinguishes exact results from seeded BE searches.

## 5. Master-phase and focused verdicts

- **General-n `m_crit`:** finite endpoint fractions, monotonicity, no-message kills, corrected tree spectrum, occupancy identity, and exact/analytic tree coverage align. Proposed acceptance through `n=2000`; all-`n` wording needs the missing `U` upper-bound line.
- **MDC-KIMI / PARITY-DUAL:** accepted. It lowers the critical dimension from opaque `5` to parity `3`; this is class separation, not contradiction. `LEAFCOIN` is only a five-vertex Omega-frontier coincidence.
- **BP1:** exact one-bit law, n<=4 DP, and crude all-n floor accepted. Sharp BP1 for n>=5 remains OPEN and should not be promoted from BE.
- **Agency hybrid `D>0`:** parity-noise upper bound accepted; adaptive converse/invariance rejected. The stale `n-2D` development bound is also wrong; corrected exogenous upper bound is `n-H2(D)`.
- **Master phases:** useful reference at frozen gauges; do not call the whole rho surface sharp while `rho_tree` is merely sufficient-certified.

## 6. Every file read

Complete-byte read attestation covered **35 files, 496,707 bytes**. Python files also passed in-memory `compile`; JSON parsed. Flat duplicates are byte-identical to peer copies.

### Flat 30--37
1. `30_KIMI_W6_PROVENANCE.txt`
2. `31_KIMI_W6_PACKAGE.md`
3. `32_KIMI_W6_plan.md`
4. `33_KIMI_W6_VERIFICATION_LOG.md`
5. `34_KIMI_W6_GENN_EC_LOG.md`
6. `35_KIMI_W6_MDC_EC_LOG.md`
7. `36_KIMI_W6_BP1_EC_LOG.md`
8. `37_KIMI_W6_PROOF_DEVELOPMENT.md`

### `peers/KIMI_W6/**` recursively
1. `peers/KIMI_W6/00_PROVENANCE.txt`
2. `peers/KIMI_W6/RADC_WAVE6_PACKAGE.md`
3. `peers/KIMI_W6/SHA256SUMS.txt`
4. `peers/KIMI_W6/plan.md`
5. `peers/KIMI_W6/w6/.bp1_verify.out`
6. `peers/KIMI_W6/w6/RADC_WAVE6_PACKAGE.md`
7. `peers/KIMI_W6/w6/W5_FULL_PREFIX_CHECKS.py`
8. `peers/KIMI_W6/w6/W6_BP1_EC_LOG.md`
9. `peers/KIMI_W6/w6/W6_GENN_EC_LOG.md`
10. `peers/KIMI_W6/w6/W6_MDC_EC_LOG.md`
11. `peers/KIMI_W6/w6/W6_PROOF_DEVELOPMENT.md`
12. `peers/KIMI_W6/w6/W6_VERIFICATION_LOG.md`
13. `peers/KIMI_W6/w6/canc8.py`
14. `peers/KIMI_W6/w6/heavy7.py`
15. `peers/KIMI_W6/w6/heavy7b.py`
16. `peers/KIMI_W6/w6/heavy7c.py`
17. `peers/KIMI_W6/w6/search5.py`
18. `peers/KIMI_W6/w6/search5b.log`
19. `peers/KIMI_W6/w6/search5b.py`
20. `peers/KIMI_W6/w6/search5b_results.json`
21. `peers/KIMI_W6/w6/sol_m_demand_grid.cpp`
22. `peers/KIMI_W6/w6/w5_full_prefix_check.cpp`
23. `peers/KIMI_W6/w6/w6_bp1_checks.py`
24. `peers/KIMI_W6/w6/w6_genn_checks.py`
25. `peers/KIMI_W6/w6/w6_lib.py`
26. `peers/KIMI_W6/w6/w6_mdc_checks.out`
27. `peers/KIMI_W6/w6/w6_mdc_checks.py`

### Missing / `NOT_IN_ZIP`

- No `NOT_IN_ZIP` marker occurs in any of the 35 files.
- No flat file in requested range 30--37 is missing.
- Every path named by `SHA256SUMS.txt` exists; the manifest's self-digest fails as noted.
- Optional n=5 down MDC computation is explicitly “not computed (2^32 subsets), as authorized,” not `NOT_IN_ZIP`.

## 7. Validation evidence and residual risks

- Complete-byte inventory: `COMPLETE_READ_OK files=35 bytes=496707`; `NOT_IN_ZIP hits=0`; `DUPLICATES_IDENTICAL=True`.
- Adaptive counterexample check: `D=0 R=1/2 E|Q|=3/2 claimed_lower=3/2 violation=1`.
- Manifest: `./SHA256SUMS.txt: FAILED`, exit `1`; all non-self entries shown as OK.
- Residual risk: main EC jobs were not rerun in this read-only audit; recorded outputs are internally consistent except the documented runtimes, stale proof note, and GEN-N path portability.

```acceptance-report
{
  "criteriaSatisfied": [
    {
      "id": "criterion-1",
      "status": "satisfied",
      "evidence": "Six path-specific severity findings, complete theorem inventory, all 35 files listed, EC command/output table, and residual risks are recorded above."
    }
  ],
  "changedFiles": [
    "/Users/aditya/AI/TokenZero/.pi-subagents/artifacts/outputs/ff585402-4ca2-4804-9371-e125323a92ed/analysis/20_kimi_w6.md"
  ],
  "testsAddedOrUpdated": [],
  "commandsRun": [
    {
      "command": "complete-byte Python inventory/read + in-memory compile/JSON parse",
      "result": "passed",
      "summary": "35 files, 496707 bytes; zero NOT_IN_ZIP hits; flat/peer duplicates identical"
    },
    {
      "command": "adaptive-demand n=2,m=2 exact counterexample enumeration",
      "result": "passed",
      "summary": "D=0, conditional rate 1/2 versus claimed lower bound 3/2"
    },
    {
      "command": "cd peers/KIMI_W6 && sha256sum -c SHA256SUMS.txt",
      "result": "failed",
      "summary": "Only SHA256SUMS.txt self-entry failed; command exit 1"
    },
    {
      "command": "python3 w6_genn_checks.py / python3 w6_mdc_checks.py / python3 w6_bp1_checks.py",
      "result": "not-run",
      "summary": "Read-only audit; GEN-N rewrites a hard-coded /mnt log and long-run outputs already exist in the return"
    }
  ],
  "validationOutput": [
    "COMPLETE_READ_OK files=35 bytes=496707",
    "NOT_IN_ZIP hits=0",
    "DUPLICATES_IDENTICAL=True",
    "D=0 R=1/2 E|Q|=3/2 claimed_lower=3/2 violation=1",
    "./SHA256SUMS.txt: FAILED (exit 1)"
  ],
  "residualRisks": [
    "W6-AGRD-DTV adaptive-demand converse is false unless demands are exogenous or demand information is charged.",
    "General-n m_crit for n>2000 needs the omitted U upper-bound bridge.",
    "Full EC suites were assessed from preserved logs/raw outputs rather than rerun."
  ],
  "noStagedFiles": true,
  "diffSummary": "Added only the required read-only audit artifact; no source-root files changed.",
  "reviewFindings": [
    "high: 31_KIMI_W6_PACKAGE.md:363-387 - adaptive S is an uncharged X-dependent side channel, refuting W6-AGRD-DTV converse/invariance",
    "medium: 31_KIMI_W6_PACKAGE.md:421-430 - n>2000 tail bounds e^-t while M uses larger U",
    "medium: 37_KIMI_W6_PROOF_DEVELOPMENT.md:180-201 - stale backwards n-2D agency bound",
    "medium: peers/KIMI_W6/w6/w6_genn_checks.py:17,1074 - hard-coded absent /mnt path breaks advertised reproduction",
    "low: peers/KIMI_W6/SHA256SUMS.txt - self-check fails"
  ],
  "manualNotes": "Independent proposal: major revision for agency claim; accept the remaining headline theory with the stated tail/surface qualifications."
}
```
