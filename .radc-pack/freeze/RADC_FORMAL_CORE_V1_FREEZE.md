# RADC Formal Core v1 — FREEZE inventory (not new proofs)

**Date:** 2026-07-27  
**Purpose:** Shared freeze list for (A) Wave-6 theory and (B) implementation bead distillation.  
**Rule:** This document inventories **what the campaign claims is frozen**. Peer/Sol Pro labels are **claims** until independent EC re-run. Do not invent new theorems here.

---

## 0. Dual-track split (non-negotiable)

| Track | Goal | Success metric |
|-------|------|----------------|
| **A — Theory Wave 6** | Advance formal RADC beyond freeze | New PROVED+EC theorems; obstruction maps for failed moonshots |
| **B — Implementation beads** | Turn freeze into codeable work | Bead graph with acceptance tests; no open math research inside impl beads |

Track B must **not** re-open dual-track MDC merge or full North Star. Track A must **not** claim production TokenZero wins.

---

## 1. Normative product substrate (already in repo docs)

| ID | Source | Role |
|----|--------|------|
| RACC-PUBLIC | `docs/racc-public.md` | Visible capsule, exact refs, recovery-adjusted objective, never-wrong-bytes |
| RACC-DISTILL | `docs/RACC_RESEARCH_DISTILL.md` | Research distill (non-shipping notes) |

---

## 2. Wave-4 Sol Pro substrate (formal base)

**Source:** `sources/wave4/WAVE4_SOLPRO_PACKAGE_FULL.txt` (also in Wave-5 merge zip as file 10)

**Freeze as working base (re-check before code promotion):**

| Family | Content (claim shape) |
|--------|------------------------|
| W4-DP / W4-FLOOR | Exact subset-tree DP; piecewise \(F_4(t)\) |
| W4-PHASE | Linked-slice phase thresholds at registered gauges |
| W4-AFF-Q4-40 | EDC-style candidate dominates no-recovery hull at \((\rho,\lambda)=(40,20)\) with stated margins |
| W4-Qn | Lower-capped lift / n≥3 style extensions |
| W4-DA-RATE | Opaque exact-ref rate vs full n-bit no-recovery |
| W4-OPAQUE-CAS-ALIAS + DIRECT-HASH-KILL | Visible hash ≠ opaque; two-level alias→CAS |
| W4-CORRIDOR | Handle/tokenizer/selector/latency parameters \((h,q,c)\) |
| W4-NEG-NR / NO-PENALTY-ROBUST | Negatives that constrain overclaim |

---

## 3. Wave-5 peer islands (do not merge by name)

### 3.1 Fable W5

**Source:** `wave5-returns/FABLE/`

Claim families: MDC (\(p_c\), \(n_{\mathrm{crit}}=5\)), ANTI-OPT, BP1 (OPEN general-n), AOT, DLU, RACE, SMC, checkers `w5a`–`w5f`.

### 3.2 Kimi W5

**Source:** `wave5-returns/KIMI/`

Claim families: DLU-*, LPP-*, AOT-*, MDC-PARITY-DUAL (“second demand free”), SMC-*, RACE-*, companions + checkers.

### 3.3 Grok residue

**Source:** `wave5-returns/GROK_DEEP_RESEARCH/`

Agency RD defs; **MDC dual-track meta**; conflict matrix; partial EC notes.

**Freeze rule:** `MDC-FABLE` and `MDC-KIMI` remain **distinct IDs** until a reduction is PROVED+EC.

---

## 4. Sol Pro W5 + continuations (strongest finite closures)

**Source:** `wave5-returns/SOLPRO/`

| Package | What to treat as freeze *candidates* |
|---------|--------------------------------------|
| Main theory PDF/txt | ISC class; phase formulas; agency RD on ISC; OARC-style opacity×corridor; multi-demand structure; Q4 multi-demand phase **started** |
| Continuation 1 | \(R_{\mathrm{ag},\theta}(D)=1-H_2(D)\); NR water-filling; no-message Q4 face \(m\le18\) / \(m\ge19\) (not full prefix hull) |
| Continuation 2 | **Full Q4 sequential prefix hull:** parity dominates complete randomized variable-length no-recovery class on \(\Theta_4^\downarrow\) and \(\Theta_4^{\mathrm{cap}}\) at (40,20) **iff** \(1\le m\le 18\); fails for \(m\ge 19\); coverage-leaf + prefix spectrum + portable C++/Python EC |

**Highest-confidence engineering-facing freeze (after re-run of Cont-2 checkers):**

> **W5-SOL-MDC-Q4-FULL-18-19 (Cont-2):** sequential parity ledger vs full no-recovery prefix hull on registered Q4 polytopes/gauge; critical demand count \(m_{\mathrm{crit}}=18\).

---

## 5. Explicitly NOT frozen (do not bead as PROVED)

- Production TokenZero global Pareto dominance  
- Real tokenizer \(h_\tau\) without measurement  
- “99.9% compression always”  
- Identification of Fable MDC with Kimi MDC  
- Full \(R_{\mathrm{ag}}(D)\) on arbitrary real agent policies (only formal ISC/binary models)  
- BP1 general-\(n\) (Fable OPEN)  
- Arbitrary-\(n\) Cont-2 generalization  

---

## 6. Product KPI for Track B (return)

Primary metric for code work:

\[
\text{RATC / recovery-adjusted tokens per successful task}
=
\text{visible} + \text{expand} + \text{weighted fails/retries}
\]

Secondary: task success, anchor recall, dangling-ref rate, expand rate.

**Not** primary: first-message visible length alone.

---

## 7. File map for attach packs

See:

- Track A: `wave6-attach-FLAT/` + `WAVE6_THEORY_PROMPT.md`  
- Track B: `impl-attach-FLAT/` + `IMPL_BEADS_DISTILL_PROMPT.md`  

Operator: `DUAL_TRACK_OPERATOR.md`
