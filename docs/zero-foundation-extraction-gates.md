# Zero-Foundation Extraction Gates

Bead: `tokenzero-9s32.3` (parent epic `tokenzero-9s32`).
Evidence: `docs/zero-foundation-rfc.md` (ownership/release models, recommendation-only) and
`docs/zero-foundation-inventory.md` (exact-SHA duplication inventory, candidate seams, deletion ranges).
This document is the enforceable pre-code acceptance gate for every future seam migration. It is
docs-only: it writes no code, sets no numeric performance thresholds, and cannot be satisfied by prose.

## Scope and precedence

- Applies to every extraction of a seam named in `docs/zero-foundation-inventory.md` (ZeroRef/CAS,
  1TP atoms and ACK/2, QuickJS CodeMode host, telemetry/accounting, error/result envelopes) and to any
  future seam added by the same process.
- The RFC decides ownership/release/dependency model. This file does not select a model. Gate G0
  requires that decision before any extraction implementation starts.
- "Migration" below means one seam moved from an engine into a hub crate, with engine adapters
  retained locally.
- A gate fails closed: missing evidence fails the migration. No gate may be waived by the implementer;
  waivers require the owner (see Waiver rules).

## Gate G0 -- Owner-approved RFC model

| Check | Required artifact |
|---|---|
| G0.1 | Owner decision recorded on `docs/zero-foundation-rfc.md` (status `APPROVED:<model>` or `REJECTED`, owner name, date) |
| G0.2 | The chosen model is named in the migration plan; the plan cites the decision line |
| G0.3 | No extraction code, crate creation, dependency line, or engine edit lands before G0.1 passes |

Evidence-only beads `tokenzero-9s32.2` (inventory) and this spec are explicitly permitted to refine
the decision record without selecting a model, per the RFC's own scope note.

## Gate G1 -- Behavior and conformance

| Check | Required artifact |
|---|---|
| G1.1 | Byte-for-byte/golden behavior where applicable: every golden fixture that exercises the seam passes unchanged after migration (byte-identical output, not re-derived) |
| G1.2 | Cross-engine ZeroRef/gauge conformance: the shared conformance suite (ZeroRef v1 vectors, gauge conformance) passes from the hub crate, consumed by all three engines |
| G1.3 | Engine-unique authority tests still pass locally: TokenZero tokenizer identity/provider verification, stable-prefix geometry, Decision Views; FSZero byte/state durability; GraphZero graph semantics. See inventory section "Logic that must not move" |
| G1.4 | Mutation check: breaking the migrated behavior turns the corresponding gate red (no vacuous tests) |

## Gate G2 -- Targeted verification discipline

| Check | Required artifact |
|---|---|
| G2.1 | Verification runs only the changed crates (`cargo test -p <crate> <filter> -- --test-threads=1`) plus the shared conformance crate |
| G2.2 | Formatting checks exact rustfmt paths only (`rustfmt --check --edition <ed> <file1> <file2> ...`), never `cargo fmt` on the workspace |
| G2.3 | Clippy is targeted: `CARGO_TARGET_DIR=<repo-shared-target> cargo clippy -p <crate> --lib -- -D warnings` (or the crate's committed strict lane), never `--workspace --all-targets` churn |
| G2.4 | The verification report names every command with its exact working directory and pinned env (`CARGO_TARGET_DIR`, toolchain) |

Template:

```sh
CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo test -p <crate> <filter> -- --test-threads=1
rustfmt --check --edition <ed> crates/<crate>/src/<file>.rs crates/<crate>/tests/<file>.rs
CARGO_TARGET_DIR=/tmp/rch_target_tokenzero cargo clippy -p <crate> --lib -- -D warnings
```

## Gate G3 -- Performance receipts with pre-declared budgets

| Check | Required artifact |
|---|---|
| G3.1 | Numeric budgets (latency, peak RSS, binary size) declared in the migration plan and owner-approved BEFORE code starts. This spec intentionally sets no thresholds; each migration declares them |
| G3.2 | Baseline receipt and candidate receipt measured under identical pinned inputs: engine commit SHA, hub commit SHA, toolchain, profile (`release-perf` for claim paths), hardware/runner identity, and the same corpus |
| G3.3 | Every receipt names the measurement surface and unit, is raw (not smoothed), and is reproducible by the listed commands |
| G3.4 | Fail-closed: a migration with any missing receipt or undeclared budget is blocked, even if the candidate looks faster |
| G3.5 | No benchmark claim is published unless the approved performance-harness authority (`.bench-history` contract family, `tokenzero-gnt-perf-keep-gate-91mg`) exists; until then receipts are internal evidence only |

## Gate G4 -- Readability

| Check | Required artifact |
|---|---|
| G4.1 | `rustfmt --check` clean on every touched file |
| G4.2 | No line golfing; no multiple control-flow statements compressed onto one physical line |
| G4.3 | No new complexity/hotspot above the committed baseline p95 for the touched files (measured by the repository's committed complexity tooling if any; otherwise documented per-file baseline) |
| G4.4 | Touched-file scc complexity density must not rise without an explicit owner waiver |
| G4.5 | LOC is an outcome metric only: a migration may never trade readability for a lower LOC number |

## Gate G5 -- LOC accounting (outcome, not target)

| Check | Required artifact |
|---|---|
| G5.1 | Tracked exact-SHA Tokei/scc report per repository and per crate, measured from `git archive` of the recorded commits (never worktree bytes; generated/fixture/test mass excluded), following the inventory measurement script |
| G5.2 | Per-engine formula, net deletion positive: `duplicate_production_LOC_deleted - adapter_LOC_added - allocated_hub_production_LOC_added > 0`. Allocation is explicit and conserved: the shares across migrated engines must sum to `hub_seam_production_LOC_added`; zero or double allocation fails |
| G5.3 | Aggregate formula across engines for the same seam: `sum(deleted_duplicate) > sum(adapters) + hub_seam_production_LOC_added` |
| G5.4 | Formulas use Tokei `code` (or scc) columns on production files only; fixtures, goldens, tests, and generated code are reported separately and never counted as deletion credit |
| G5.5 | The report is auditable: rerunnable commands included, exact SHAs recorded, and the pre/post trees retained or reconstructable |

## Gate G6 -- Rollback and fail-loud reporting

| Check | Required artifact |
|---|---|
| G6.1 | Rollback boundary per the RFC model is stated in the migration plan (Model A: engine pin line plus `Cargo.lock`; Model C: vendored snapshot plus sync skip; Model B requires explicit owner acceptance of coordinated repin) |
| G6.2 | Rollback is actually exercised on the pre-release candidate (pin-only revert rebuilds and targeted tests pass) |
| G6.3 | Fail-loud report schema is stable and versioned: `schema_version`, `migration_id`, `seam`, `model_decision`, per-gate status, `blocked_reasons[]`, `budgets` (declared vs measured), `loc` (G5 rows), `rollback_boundary`, `commands[]` |
| G6.4 | Any failed gate yields a non-success exit from the migration gate runner and a report naming the exact missing artifact |

## Pass/fail checklist (one row per gate)

| Gate | Pass condition | Status |
|---|---|---|
| G0 | Owner model decision recorded; no code before it | |
| G1 | Golden byte-identical, conformance green, engine authority tests green, mutation red | |
| G2 | Targeted crate tests + exact-path rustfmt + targeted clippy only | |
| G3 | Budgets pre-declared; baseline/candidate receipts identical env; fail-closed | |
| G4 | rustfmt clean; no golfing; no p95/complexity rise without waiver | |
| G5 | Per-engine and aggregate LOC formulas positive on production-only counts | |
| G6 | Rollback exercised; fail-loud report schema emitted | |

A migration ships only when every row is PASS and the fail-loud report is attached.

## Required artifacts (per migration)

1. Migration plan (seam, model decision line, budgets, rollback boundary)
2. Golden/conformance/authority test evidence (G1)
3. Targeted verification log (G2)
4. Baseline and candidate performance receipts (G3)
5. Readability check output (G4)
6. Exact-SHA LOC report with formulas (G5)
7. Rollback exercise log + fail-loud report (G6)

## Waiver rules

- Only the owner may waive a gate. Waivers are recorded in the fail-loud report with the owner's
  name, date, the gate waived, the reason, and a concrete re-check date.
- G0 and G3.1 (pre-declared budgets) are never waivable: they are preconditions, not evidence.
- A waiver for G4.4 (complexity density rise) must name the touched files and the accepted delta;
  the migration still cannot ship unreadable code (G4.5 is unconditional).
- No waiver may convert a missing receipt into a passed gate; it may only record a temporary,
  dated exception.

## Out of scope (this spec)

- Setting numeric thresholds (G3.1 requires the migration to declare them).
- Selecting an RFC model (G0).
- Writing extraction code, creating crates, or editing engine/hub manifests.
- Running Cargo, rustc, clippy, rustfmt, or benchmark tooling.
