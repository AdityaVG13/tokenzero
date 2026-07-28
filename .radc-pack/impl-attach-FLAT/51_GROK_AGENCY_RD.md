# RADC Wave-5 Residue Package (Grok deep research)

**Model/run:** Grok Build (residue lane; post-workflow completion of formal deliverables)  
**Date:** 2026-07-27  
**Mode:** prove-first residue over Wave-4 substrate + peer claim inventory (Fable W5, Kimi W5)  
**Scope:** agency rate-distortion formalization, dual-weighted expand sketch, tropical multi-expand sketch, peer conflict matrix for Sol Pro. No production ship claim. No rewrite of peer proof bodies.

**Proof-status tags:** PI | DR | EC | BE | SB

**Peers read (claims, not axioms):**  
- `wave5-returns/FABLE/RACC_WAVE5_SPLICE_PACKAGE.md`  
- `wave5-returns/KIMI/RADC_WAVE5_PACKAGE.md` (+ companions)  
- `sources/wave4/WAVE4_SOLPRO_PACKAGE_FULL.txt` (substrate)

---

## 0. Executive verdict

1. **Agency RD object is formalized** as the residue primary (W5-GROK-AGENCY-DEF, W5-GROK-AGENCY-ZERO-SLICE). At \(D_{\mathrm{dec}}=0\), recovery-aware rate is identified with Wave-4 exact-recovery zero decision-distortion accounting under locked gauges (DR splice of W4-AFF / W4-DA-RATE), **not** a new independent proof of W4 hull dominance.

2. **Strict operational inequality** “\(R_{\mathrm{ag}}(D)\) lies strictly below no-recovery prefix-code rate on a nonempty open set of \(D>0\)” remains **SB / open** as a full theorem. What is proved here is: (i) definitions and gauge lock; (ii) zero-slice recovery of W4 story; (iii) a **hybrid construction class** (lossy visible + exact expand) with a derived sufficient condition for \(R_{\mathrm{ag}}\) improvement when decision TV is controlled by expand of a sufficient statistic (W5-GROK-AGENCY-SUFF, DR).

3. **Peer conflict (load-bearing for Sol Pro):** Fable and Kimi both claim large W5 families with overlapping names, but **two-demand MDC constructions are not the same object**. Fable: sequential \(\pi_{\mathrm{EDC}}^2\) with collision mass \(p_c=\sum\theta_i^2\), critical \(n_{\mathrm{crit}}^{(2)}=5\). Kimi: PARITY-DUAL batch/seq with “second demand free” at batch ledger \((5,0,4)\). Sol Pro must dual-track these as **MDC-FABLE** vs **MDC-KIMI**, not merge by label.

4. **High-agreement islands (still re-check before freeze):** antipodal one-bit optimality all-\(n\) (Fable W5-ANTI-OPT / Kimi W5-LPP-OPT); ledger uniqueness of \((5,0,4)\) not full path uniqueness (both DLU); AOT 0-bit pre / 1-bit post visible ledger shape; SMC corridor inverses as floor inverses.

5. **EC this run:** Fable `w5c_onebit.py` / `w5f_final_checks.py` executed (see 23). Kimi `drive.py` requires compiling `w5dp.cpp` with non-portable `#include <bits/stdc++.h>` — **failed on this macOS host**; treat Kimi 74/74 claim as **author EC**, not re-verified here.

6. **Residue secondaries:** dual-weighted expand (definition + DR dominance criterion under a linear decision functional); tropical multi-expand (min,+ envelope of expand chains) as formal objects ready for Sol Pro / future EC.

---

## 1. Effort budget log

| bucket | share | notes |
|--------|-------|-------|
| Affirmative invention (agency RD defs, zero-slice, sufficient hybrid, dual-weighted, tropical) | ~55% | primary residue |
| Peer inventory + conflict matrix | ~25% | Sol Pro merge fuel |
| EC replications | ~15% | Fable checkers run; Kimi compile blocked |
| Instrumental obstruction | ~5% | open \(R_{\mathrm{ag}}(D)\) curve; MDC non-identification |

---

## 2. Statement lock

### 2.1 Inherit Wave-4 (substrate)

As in Sol Pro W4 merge: source \(X\sim\mathrm{Unif}\{0,1\}^n\); demand \(S\sim\theta\) independent; formal tokenizer; linked slice \(\lambda=\rho/2\); gauge \((\rho,\lambda)=(40,20)\) where declared; candidate EDC with opaque handle; baseline = no-recovery pre-demand prefix codes; dominance in \((M,D,L)\).

### 2.2 Peer claims are not axioms

Fable/Kimi **PROVED** labels are **claims** for Sol Pro re-certification. Overlapping IDs with different constructions are **not** identified.

### 2.3 Agency task (new lock for residue)

- Full history \(H = (X,\text{prior tool state})\).  
- Capsule random variable \(Z\) (visible transcript before action).  
- Agent policies: \(\pi_{\mathrm{full}}(\cdot|H)\), \(\pi_{\mathrm{cap}}(\cdot|Z)\) on a finite action set \(\mathcal{A}\).  
- Decision distortion:
  \[
  D_{\mathrm{dec}} := \mathbb{E}\big[d\big(\pi_{\mathrm{full}}(\cdot|H),\pi_{\mathrm{cap}}(\cdot|Z)\big)\big],
  \]
  with \(d\in\{\mathrm{TV},\mathbf{1}\{\mathrm{top\text{-}action mismatch}\}\}\) declared per theorem.  
- Rate \(R\): expected recovery-adjusted token cost under the **same** carried-multiplier ledger as W4/W5 peers for the chosen protocol (single-demand default).

---

## 3. Peer claim inventory (IDs only)

### 3.1 Fable W5 (from their §4)

W5-MDC-0..5, W5-ANTI-OPT, W5-BP1 (OPEN general \(n\ge5\)), W5-AOT, W5-DLU, W5-RACE, W5-SMC, W5-Q3U, W5-Q5-SW.

### 3.2 Kimi W5 (from their §4)

W5-DLU-{1,0,STRUCT,RADIUS}, W5-LPP-{ANTI,SYM,OPT,KILL,CERT,PRODUCT}, W5-AOT-1..6, W5-MDC-{FLOOR,MONO,RATE,BATCH,SEQ,INTERACTION,NECESSITY}, W5-SMC-1..4, W5-RACE-1..2.

### 3.3 Wave-4 survivors used as PI/DR inputs

W4-DP/FLOOR/PHASE, W4-AFF-Q4-40, W4-DA-RATE, W4-OPAQUE-CAS-ALIAS, W4-DIRECT-HASH-KILL, W4-CORRIDOR / PHASE-Q4-H, W4-GEO (as claimed in W4 package).

---

## 4. New formal objects

**O-G1 (Agency RD functional).** For a compression protocol \(\Pi\) inducing \(Z\) and optional expands,
\[
R_{\mathrm{ag}}(D;\Pi,\mathcal{T}) := \inf\big\{ R(\Pi') : \Pi'\in\mathcal{T},\ D_{\mathrm{dec}}(\Pi')\le D \big\}.
\]
\(\mathcal{T}\) = declared protocol class (no-recovery prefix / EDC exact-recovery / hybrid lossy+expand).

**O-G2 (Zero-slice identification).** The set \(\{ \Pi\in\mathcal{T}_{\mathrm{ER}} : D_{\mathrm{dec}}=0\}\) of exact-recovery decision-faithful protocols under a task where correct action is a function of \((S,X_S)\) only.

**O-G3 (Hybrid sufficient expand).** Visible lossy sketch \(U=f(X)\) plus post-demand expand of a coordinate set \(J(S)\) with \(X_{J(S)}\) a sufficient statistic for the optimal action.

**O-G4 (Dual-weighted expand score).** For decision functional \(J\) and coordinate \(i\), influence \(I_i := \mathbb{E}[|\partial_i J|]\) in the formal finite setting (flip-bit influence). Expand policy ranks \(i\) by \(I_i\).

**O-G5 (Tropical expand path).** For demand sequence \(S_{1:m}\), expand cost vector \(c\in\mathbb{R}_+^m\); series composition under \((\min,+)\) for alternative expand schedules.

---

## 5. Theorem index (W5-GROK-*)

| ID | Status | Statement (short) | Tag |
|----|--------|-------------------|-----|
| W5-GROK-AGENCY-DEF | PROVED (definition) | O-G1–O-G3 are well-defined under finite \(\mathcal{A}\), finite \(X\), declared ledger | DR |
| W5-GROK-AGENCY-ZERO-SLICE | PROVED (DR) | If the optimal action is \(a^*=\phi(S,X_S)\) and \(\Pi\) is exact-recovery EDC with correct expand of \(X_S\), then \(D_{\mathrm{dec}}=0\) for any \(\pi\) that plays \(\phi\) from \((S,X_S)\); rate equals EDC recovery-aware ledger of W4 under the same \((h,q,c)\) | DR |
| W5-GROK-AGENCY-LOSS-TRANSFER | PROVED (DR) | If per-step losses \(\ell\in[0,1]\) and \(d=\mathrm{TV}\), then \(\mathbb{E}[\ell(\pi_{\mathrm{cap}})-\ell(\pi_{\mathrm{full}})]\le D_{\mathrm{dec}}\) | DR (standard TV coupling; omega TheoremCard shape) |
| W5-GROK-AGENCY-SUFF | PROVED (DR, conditional) | Under O-G3 with \(J(S)\) sufficient for \(\phi\), \(D_{\mathrm{dec}}=0\) and \(R\) equals opaque-handle + \(|J|\)-expand accounting; if \(|J|<n\) vs no-recovery full \(n\)-bit prefix at zero error, rate gap is the standard DA-rate gap (splice W4-DA-RATE) | DR |
| W5-GROK-AGENCY-CURVE | OPEN / SB | Strict inequality \(R_{\mathrm{ag}}(D) < R_{\mathrm{nr}}(D)\) on a nonempty open \(D\)-interval for hybrid class vs no-recovery | SB |
| W5-GROK-MDC-DUALTRACK | PROVED (meta) | Fable MDC and Kimi MDC are distinct formal objects; identification is forbidden without a reduction proof | DR |
| W5-GROK-DUAL-EXPAND-DEF | PROVED (definition) | O-G4 well-defined on finite boolean cube tasks | DR |
| W5-GROK-DUAL-EXPAND-DOM | PROVED (DR, restricted) | For linear \(\phi=\mathrm{XOR}_{i\in T} X_i\) (parity on set \(T\)), expand-all-of-\(T\) after demand of which parity is needed (single demand of subset) minimizes expand count among zero-error policies; any expand outside \(T\) is waste | DR |
| W5-GROK-TROPICAL-EXPAND | PROVED (DR) | Expand schedules form a \((\min,+)\) semiring under series composition of additive latency; critical path = max mean cycle only after a Markov demand matrix is declared — statement of the algebra, not a numeric certificate | DR |
| W5-GROK-BP1-STATUS | PROVED (meta) | Fable leaves BP1 general-\(n\) OPEN; Kimi LPP closes all-\(n\) antipodal optimum but does not automatically close Fable’s amortized-tangent equivalence for all floors — Sol Pro must not equate the two | DR |
| W5-GROK-EC-FABLE-ONEBIT | EC | Fable `w5c`/`w5f` scripts run on this host; Δ formula and mod-8 tie law checks report True in script footer; uniqueness “all n unique” is False precisely when ties exist (8\|n) | EC |
| W5-GROK-EC-KIMI-HARNESS | BE / blocked | Kimi `drive.py` harness not re-run: `w5dp.cpp` needs `bits/stdc++.h` (absent on Apple clang) | BE |

---

## 6. Proofs (affirmative first)

### 6.1 W5-GROK-AGENCY-DEF

Finite probability spaces: \(X\in\{0,1\}^n\), \(S\in[n]\), actions \(\mathcal{A}\) finite. Capsule \(Z\) is a deterministic or randomized function of protocol messages. Policies are kernels to \(\Delta(\mathcal{A})\). Expectations exist. Rate is a finite linear combination of expected token counts with declared multipliers. ∎

### 6.2 W5-GROK-AGENCY-ZERO-SLICE

Assume \(a^*=\phi(S,X_S)\). EDC stores \(X\), emits opaque handle independent of \(X\) (W4 opacity claim as PI/DR from substrate), expands \(X_S\) after \(S\), plays \(\phi(S,X_S)\). Then \(\pi_{\mathrm{cap}}\) and \(\pi_{\mathrm{full}}\) both play \(a^*\) a.s. ⇒ \(D_{\mathrm{dec}}=0\) for TV and top-action mismatch. Rate accounting matches W4 EDC ledger by definition of the same multipliers. ∎  
**Scope:** does not re-prove hull dominance over no-recovery baselines (that is W4-AFF as peer/substrate claim).

### 6.3 W5-GROK-AGENCY-LOSS-TRANSFER

Let \(\mu=\pi_{\mathrm{full}}(\cdot|H)\), \(\nu=\pi_{\mathrm{cap}}(\cdot|Z)\). For \(\ell\in[0,1]^{\mathcal{A}}\),
\[
\big|\mathbb{E}_\mu\ell-\mathbb{E}_\nu\ell\big|\le \|\mu-\nu\|_{\mathrm{TV}}.
\]
Take expectation over \((H,Z)\). ∎

### 6.4 W5-GROK-AGENCY-SUFF

If \(X_{J(S)}\) determines \(\phi(S,X_S)\), expand only \(J(S)\). Zero decision error as in 6.2. Token rate: handle + expand cost proportional to \(|J|\) under unit-bit tokenizer. No-recovery zero-error requires conveying enough information about \(X\) for all \(s\), standardly \(n\) bits in the formal bit-model (W4-DA-RATE class). Gap is DR from that rate theorem’s hypotheses; if those hypotheses fail, status drops to SB. ∎

### 6.5 W5-GROK-AGENCY-CURVE (open)

To prove a strict interior-\(D\) advantage one needs a continuous family of lossy visible maps with controlled TV and intermediate rates. Construction candidates exist (partial bit reveal pre-demand) but **no EC floor family** was computed in this residue run. Status: OPEN/SB.

### 6.6 W5-GROK-MDC-DUALTRACK

**Fable object (claim):** sequential two-demand timeline with carried multipliers; candidate ledger \(M=9-p_c\), \(p_c=\sum_i\theta_i^2\); ZE dominance iff \(p_c\ge(9-2n)/3\); \(n_{\mathrm{crit}}=5\) on \(\Theta_n^\downarrow\).

**Kimi object (claim):** PARITY-DUAL policies; batch ledger \((5,0,4)\) equal to single-demand; “second demand free”; necessity that \(\ge2\) expands fail \(L\)-dominance.

These differ in: timeline multipliers, candidate family (EDC² dedup vs parity dual), and critical-dimension statements. Without a formal reduction,  
\[
\text{W5-MDC}^{\mathrm{Fable}}\ \not\equiv\ \text{W5-MDC}^{\mathrm{Kimi}}.
\]
∎

### 6.7 W5-GROK-DUAL-EXPAND-DOM

Task: after demand \(T\subseteq[n]\) revealed, output \(\bigoplus_{i\in T}X_i\). Zero-error requires all bits in \(T\) (or an equivalent sufficient set). Minimal expand set is \(T\). Expanding \(j\notin T\) cannot reduce \(D\) below zero-error already achieved and strictly increases expand tokens under positive per-bit expand cost. ∎  
**Note:** this is a toy decision functional illustrating dual-weighting; not TokenZero production.

### 6.8 W5-GROK-TROPICAL-EXPAND

Let schedules be vectors of nonnegative expand latencies. Serial composition of independent segments adds costs (usual \(+\)). Alternative schedules take componentwise min for latency objectives that are pure delay. That is the \((\min,+)\) structure on \(\mathbb{R}_+\cup\{\infty\}\). Critical-path / max-mean-cycle claims require an explicit demand Markov matrix and are **not** asserted numerically here. ∎

### 6.9 W5-GROK-BP1-STATUS

Fable §4: W5-BP1 OPEN for general \(n\ge5\). Kimi W5-LPP-OPT closes antipodal one-bit optimum all-\(n\) (related but not identical to Fable’s amortized tangent equivalence for the full floor’s first breakpoint). Equating them is a category error unless Sol Pro proves implication either way. ∎

---

## 7. EC logs summary

See `23_GROK_EC_LOGS.md`.

- Fable `w5f_final_checks.py`: Delta formula exact; tie law; MDC integer certificates print True.  
- Fable `w5c_onebit.py`: e_anti matches W4 for n=3..8; uniqueness-all-n false when ties (expected under 8|n).  
- Kimi harness: compile blocked (`bits/stdc++.h`).

---

## 8. Conflict resolutions used

| Topic | Resolution for Sol Pro |
|-------|-------------------------|
| MDC | Dual-track; prove reductions or keep separate theorem IDs |
| One-bit antipodal | Same claim shape; prefer independent EC (Fable scripts ran) |
| DLU | Agree on ledger uniqueness ≠ path uniqueness |
| AOT / SMC / RACE | Align shapes; re-derive constants before freeze |
| BP1 vs LPP | Do not identify OPEN BP1 with LPP-OPT |

---

## 9. Obstruction map

| Route | Missing global input | Assessment |
|-------|----------------------|------------|
| Full \(R_{\mathrm{ag}}(D)\) curve | Family of lossy protocols + EC floors in \(D\) | OPEN |
| Identify Fable/Kimi MDC | Reduction between \(p_c\)-EDC² and PARITY-DUAL | Not available; dual-track |
| Re-verify Kimi 74/74 on this host | Portable C++ harness | Blocked (headers) |
| BP1 all-n | Amortized inequality beyond antipodal | OPEN (Fable); not closed by LPP alone |

---

## 10. Timestamp + model identity

- **Timestamp:** 2026-07-27 (operator host)  
- **Producer:** Grok Build residue lane filling `GROK_DEEP_RESEARCH` after incomplete workflow `deep-research-3`  
- **Status:** DONE_WITH_CONCERNS (agency curve open; Kimi EC not re-run; MDC dual-track mandatory)
