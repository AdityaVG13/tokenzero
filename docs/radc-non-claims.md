# TokenZero Non-Claims

This page is the product-facing statement of what TokenZero does **not** claim,
even after a pilot win. It is the operating manual for every savings/recovery
sentence written into README, release notes, docs, or agent instructions.

## Source of truth

The non-claims below mirror the frozen RADC Formal Core v1 statement lock,
originally imported at commit `ebe6210` as
`docs/spec/RADC_FORMAL_CORE_V1.md`. That file was intentionally removed from the
public tree by commit `d6c505f` during the public-repo slim pass. Claim policy
is now owned by the ZeroStack hub as receipt-generated claim policy (Q99
discipline); TokenZero keeps this page so product docs and the claim audit
refer to one stable, auditable list. The historical freeze remains the
citation for every row below.

## Non-claims

| # | TokenZero does NOT claim | Freeze citation (ebe6210:docs/spec/RADC_FORMAL_CORE_V1.md) |
|---|---|---|
| 1 | **99.9%-compression always.** No "always" compression percentage, on any corpus or task. | Section 2 "Explicitly NOT frozen": "99.9% compression always" |
| 2 | **Global Pareto dominance.** No claim that TokenZero dominates every no-recovery or competitor pipeline across tasks. | Section 2: "Production TokenZero global Pareto dominance" |
| 3 | **Real-tokenizer h_tau/q_tau/c_tau without provider-locked measurement.** Handle, selector, and CAS-round-trip token costs are not real-tokenizer numbers until measured on the declared production tokenizer. | Section 3 (gauge `(rho, lambda) = (40, 20)` registered at `W4-AFF-Q4-40` and `W5-SOL-MDC-Q4-FULL-18-19`); corridor values remain formal-gauge until measured. |
| 4 | **Fable-MDC = Kimi-MDC identity.** The two peer ledger identities stay distinct; no reduction claim. | Section 1 peer islands `MDC-FABLE` / `MDC-KIMI`; Section 2: "Identification of Fable MDC with Kimi MDC" |
| 5 | **R_ag(D) on arbitrary real agent policies.** `R_ag,theta(D) = 1 - H2(D)` is proven only on formal ISC/binary models, not arbitrary real agent policies. | Section 2: "Full R_ag(D) on arbitrary real agent policies"; `W5-SOL-AGRD-THETA` |
| 6 | **BP1 general-n.** The first-breakpoint equivalence for general `n` remains open. | Section 2: "BP1 general-n (Fable OPEN)" |
| 7 | **Formal gauge = production measurement.** Corridor/gauge values (e.g. `(40,20)`) are frozen evaluation points, not runtime knobs and not production metrics. | Section 3 statement lock; Section 4 dual-track split: formal results must not become production TokenZero wins |

## Rules for product text

- A savings number is always paired with the measured surface, corpus, and
  tokenizer; no unlabeled percentages.
- "Compression" claims are stated as measured rows on a named suite, never as
  a universal property.
- Any cross-engine or peer comparison states the exact identity of both
  sides; no conflation of formal gauge with production measurement.
- The claim audit (`tokenzero claim-audit`) gates release-facing statements;
  this page is the reference the audit and promotion output point to.
