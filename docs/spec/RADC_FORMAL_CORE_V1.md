# RADC Formal Core v1 — Freeze Import + Statement Lock

**Date:** 2026-07-27 (freeze); imported into `docs/spec/` per bead radc-e0-lprp.1.
**Source of truth:** `.radc-pack/impl-attach-FLAT/00_RADC_FORMAL_CORE_V1_FREEZE.md`.
**Status:** PRODUCT doc citing DR/EC theorem IDs. All Peer/Sol Pro labels are
**claims until independent EC re-run**, per the freeze rule. This document
invents no new theorems.

---

## 1. Frozen IDs (claim-until-EC-rerun)

### Wave-4 Sol Pro substrate (formal base)

| ID | Content (claim shape) |
|----|------------------------|
| W4-DP / W4-FLOOR | Exact subset-tree DP; piecewise F4(t) |
| W4-PHASE | Linked-slice phase thresholds at registered gauges |
| W4-AFF-Q4-40 | EDC-style candidate dominates no-recovery hull at (rho, lambda) = (40, 20) with stated margins |
| W4-DA-RATE | Opaque exact-ref rate vs full n-bit no-recovery |
| W4-OPAQUE-CAS-ALIAS | Visible hash is not the opaque handle; two-level alias -> CAS |
| W4-DIRECT-HASH-KILL | Companion to OPAQUE-CAS-ALIAS: raw content hash must not be the handle |
| W4-CORRIDOR-Q4 | Handle/tokenizer/selector/latency corridor parameters (h, q, c) on Q4 |

### Wave-5 Sol Pro + continuations (strongest finite closures)

| ID | Content (claim shape) |
|----|------------------------|
| W5-SOL-AGRD-THETA | R_ag,theta(D) = 1 - H2(D); NR water-filling; no-message Q4 face m <= 18 / m >= 19 (not full prefix hull) |
| W5-SOL-MDC-Q4-FULL-18-19 (Cont-2) | Sequential parity ledger dominates the complete randomized variable-length no-recovery prefix class on Theta_4-down and Theta_4-cap at (40, 20) iff 1 <= m <= 18; fails for m >= 19; coverage-leaf + prefix spectrum + portable C++/Python EC. Critical demand count m_crit = 18. |

### Peer islands kept distinct (do NOT merge by name)

- `MDC-FABLE` and `MDC-KIMI` remain **distinct IDs** until a reduction is PROVED+EC.
- Fable W5: MDC (p_c, n_crit = 5), ANTI-OPT, BP1 (OPEN general-n), AOT, DLU, RACE, SMC.
- Kimi W5: DLU-*, LPP-*, AOT-*, MDC-PARITY-DUAL, SMC-*, RACE-*.

---

## 2. Explicitly NOT frozen (do not cite as PROVED)

Copied verbatim from the freeze inventory, section 5:

- Production TokenZero global Pareto dominance
- Real tokenizer h_tau without measurement
- "99.9% compression always"
- Identification of Fable MDC with Kimi MDC
- Full R_ag(D) on arbitrary real agent policies (only formal ISC/binary models)
- BP1 general-n (Fable OPEN)
- Arbitrary-n Cont-2 generalization

---

## 3. Statement lock: formal symbol -> concrete code counter

Every formal symbol below is locked to the exact product counter that
instantiates it. Field names reviewed against
`crates/tokenzero-engine/src/ledger.rs` (`tokenzero.ledger.v1`,
`LEDGER_SCHEMA`, `LedgerRecord`, `TokenMass`) and
`crates/tokenzero-recovery/src/telemetry.rs` (`CrossEngineTelemetry`).
If a code rename diverges from this table, the table is stale and the
claim loses its product anchor.

| Formal symbol | Meaning in the theory | Locked code counter |
|---------------|------------------------|---------------------|
| **M** (visible / message cost) | Tokens the agent actually sees per served response | `tokenzero.ledger.v1` `LedgerRecord.token_mass.visible_tokens` (`TokenMass.visible_tokens`) |
| Raw baseline (no-recovery hull size) | What the response would have cost uncompressed | `TokenMass.raw_tokens` |
| Recovery/expand cost term | Cost of re-expanding a ref back to exact bytes | Counted into `LedgerRecord.cumulative_session_cost_tokens` on expand; per-store materialization visible in `CrossEngineTelemetry.payload_bytes_materialized` |
| **L** (latency cost) | Extra round-trips / handle-resolution overhead | Proxied by round-trip structure: `CrossEngineTelemetry.refs_received` / `refs_sent` / `ref_transfers` count handle resolutions vs inline transfers |
| **D** (distortion) | Loss vs exact bytes | Product side is exactness-first: a lossy capsule must present as expandable, never as a committed result. `TokenMass.saved_bytes` and `prevented_tokens` record dedup/diff savings only, never a prevented-read estimate |
| **m** (demand count) | Number of served demands in a session | One `LedgerRecord` JSONL line per served response, keyed by `LedgerRecord.session_id`; per-tool identity in `LedgerRecord.tool` |
| Gauge (rho, lambda) = (40, 20) | Registered weighting of visible vs latency cost | Not a runtime knob; frozen evaluation point for W4-AFF-Q4-40 and W5-SOL-MDC-Q4-FULL-18-19. Product claims must not extrapolate to other gauges |
| Opaque handle (W4-OPAQUE-CAS-ALIAS) | Visible hash != opaque handle; alias -> CAS | Two-level store: session alias -> CAS blob in `tokenzero-recovery` shared_cas; blob identity is `tz://blob/<digest>` |
| RATC (Track-B KPI) | Recovery-adjusted tokens per successful task = visible + expand + weighted fails/retries | Aggregated from `visible_tokens` + expand-charged `cumulative_session_cost_tokens` deltas across a `session_id` |

---

## 4. Dual-track split (non-negotiable)

| Track | Goal | Success metric |
|-------|------|----------------|
| A — Theory Wave 6 | Advance formal RADC beyond freeze | New PROVED+EC theorems; obstruction maps for failed moonshots |
| B — Implementation beads | Turn freeze into codeable work | Bead graph with acceptance tests; no open math research inside impl beads |

Track B must not re-open dual-track MDC merge or full North Star. Track A
must not claim production TokenZero wins.

---

## 5. Product KPI for Track B

Primary: RATC (recovery-adjusted tokens per successful task) = visible +
expand + weighted fails/retries.

Secondary: task success, anchor recall, dangling-ref rate, expand rate.

Not primary: first-message visible length alone.

---

*Reference only: `docs/racc.md` carries the normative product substrate
(visible capsule, exact refs, recovery-adjusted objective,
never-wrong-bytes). This file adds no obligations beyond the freeze
inventory it imports.*
