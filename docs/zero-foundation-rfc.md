# Zero-Foundation RFC: Ownership, Releases, and Dependency Model

Bead: `tokenzero-9s32.1` (parent epic `tokenzero-9s32`: zero-foundation shared crates for the
three engines, static-link, ship-separate).

Status: **DRAFT -- RECOMMENDATION ONLY. No decision is made by this document.**
Implementation is gated on explicit owner approval of one model (see Decision checklist).
This RFC writes no code: no extraction, no crate creation, no engine edits, no bead
state changes.

## Context and law constraints

The four-repo law (ZeroStack hub; FSZero bytes/state; GraphZero structure; TokenZero
model-facing surface) says:

- Engines never import each other. The hub composes them.
- Depend only on hub contract crates. Never import FSZero/GraphZero.
- Pin hub by pushed `origin/main` rev (immutable git pin). TokenZero today pins
  `zero-abi`, `zero-gate`, `zero-gauge`, `zero-ledger`, `zero-ref`, and
  `zerostack-machine-permit` to one pushed rev of `AdityaVG13/zerostack`
  (rev `3eca1c6` per `Cargo.toml`).
- Hub defects found here become hub beads, never drive-by hub edits from engine checkouts.
- Benchmarks/telemetry: no unlabeled %; receipts generate claims; Q99 labeled.
- Per-repo `Cargo.toml` + `Cargo.lock` are the reproducibility anchors; TokenZero keeps
  `deny.toml` for supply-chain policy and a nightly `rust-toolchain.toml`.

The parent epic frames the target as "shared crates for the three engines
(static-link, ship-separate)": foundation crates compile into each engine binary
(static-link), while each engine ships on its own cadence (ship-separate).

This RFC compares exactly three ownership/release/dependency models for those
foundation crates. No hybrid is evaluated here.

## The three models

### Model A -- Infrastructure-only ZeroStack foundation crates (git/lockfile pin now, crates.io later)

- New infrastructure-only crates (no model-facing behavior: storage layouts, refs,
  ledgers, gates, gauges, digest plumbing) live in the ZeroStack hub under hub
  ownership and hub ABI-digest discipline.
- Engines consume them exactly as they consume `zero-abi` today: `git = "...zerostack",
  rev = <pushed origin/main rev>`, locked in each engine's `Cargo.lock`.
- After a stability milestone, the same crates may be released to crates.io as
  versioned artifacts. The git rev pin remains the canonical reproducibility anchor;
  a crates.io version tag is documented to correspond to a specific pushed rev.

### Model B -- TokenZero-owned crates consumed by FSZero/GraphZero

- The foundation crates are owned and released from the TokenZero repo; FSZero and
  GraphZero depend on TokenZero crates (git or crates.io).
- Dependency direction becomes engine-to-engine: peers consume TokenZero's model-facing
  repository as a library source.

### Model C -- Vendored snapshots with automated upstream sync

- Each engine vendors a snapshot of foundation crate sources into its own tree.
- An automated sync tooling run (upstream rev tracking, diff, bump) refreshes the
  vendored copies on a schedule or per engine release.

## Comparison

| Dimension | A (hub git pin -> crates.io) | B (TokenZero-owned) | C (vendored + sync) |
| --- | --- | --- | --- |
| Public/private source | Single public hub repo; all three engines read the same pinned source; uniform access | TokenZero repo is public; peers depend on an engine repo as library source | Copies live in each engine tree, public or private with that repo; private source must never leak through sync; provenance is labeled per copy |
| crates.io vs git | Git rev pin now (matches existing law); crates.io release optional after stability; each release tag maps to a pushed rev and lockfile checksum | Publishing adds a registry commitment on an engine repo; git consumption still reverses the intended dependency direction | Registry irrelevant; sync tooling replaces registry semantics |
| Offline builds | Deterministic after first fetch: pinned rev + per-engine `Cargo.lock`; hub rev must exist in local cargo git cache | Same mechanics, but wrong dependency direction | Fully offline by construction (sources in-tree); sync tooling itself needs network |
| Semver / MSRV | Hub crates today declare no `rust-version`; engines run nightly per-repo toolchain; crates.io release must declare MSRV and semver, ABI digest bumps on semantic change | Foundation crates inherit TokenZero's nightly surface; peers absorb TokenZero's toolchain pressure | MSRV drifts per vendored copy unless sync enforces an upstream-declared floor |
| Rollback | One-line repin to prior pushed rev per engine; engine binaries built on the older rev keep running (static-link); crates.io yank policy documented | Coordinated repin across three repos; partial rollback leaves mixed engine states; highest blast radius | Restore prior vendored snapshot; sync tooling must not re-apply the reverted change on next run |
| Supply-chain / reproducibility | Single trust source (hub pushed rev) + `Cargo.lock` + `deny.toml`; digest-verified; smallest surface | Adds a second engine as a library trust source; enlarges surface without adding capability | Vendoring freezes code but can hide provenance; unlabeled copies violate the no-unlabeled-% telemetry spirit; every snapshot must record upstream rev + digest |
| Independent engine releases | Engines release on their own cadence against their own pins; hub crate releases never force an engine release | FSZero/GraphZero release timing couples to TokenZero crate releases | Full decoupling; each engine release must run its own sync step |
| Contribution ownership | Hub owns foundation crates; engine defects filed as hub beads; matches "hub defects found here -> hub beads" | TokenZero owns code that peers compile; peer changes to TokenZero-owned foundation code blur the model-facing authority boundary | Each engine owns its copy; upstream fixes must be re-synced by every engine; three-way ownership of one codebase |
| Lagging-engine behavior | A lagging engine keeps its older pin and compiles against it; hub ABI-digest discipline prevents silent breakage; lag is visible as a pin diff | A lagging engine lags TokenZero crate versions; TokenZero's model-facing surface moves fast and can drag peers into compile-time breakage | A lagging engine holds a stale snapshot; sync automation may overwrite local fixes unless lag detection is explicit |

## Concrete release scenarios

- **A, release:** Hub merges foundation crate change on `origin/main`, pushes, bumps
  ABI digest if semantics change. Each engine updates its pin line and `Cargo.lock` on
  its own schedule. Later, hub tags crates.io versions; engines may switch to registry
  deps with the version-to-rev mapping documented, or keep git pins.
- **B, release:** TokenZero cuts a crate version; both peers must consume it; a TokenZero
  strict-mode or surface change that touches a foundation crate becomes a compile-time
  blocker for FSZero/GraphZero even when their behavior is unchanged.
- **C, release:** Each engine runs sync before its release; any upstream change lands in
  each tree independently; a change that breaks engine A may still land in B and C
  unless the sync log records a per-engine gate.

## Concrete failure scenarios

- **A fails:** A hub rev that was pinned gets rewritten or garbage-collected (forbidden
  by law: pin only pushed `origin/main` revs; never local or un-pushed refs). A crates.io
  release gets yanked before engines migrate; mitigation is the documented rev-as-canonical
  rule plus a yank policy note in the release record.
- **B fails:** A TokenZero crate rename/restructure breaks both peers at once; dependency
  cycles or peer-import coupling reintroduces the exact "engines never import each other"
  violation the law forbids; TokenZero's authority (model-facing surface) starts to gate
  peer compilation.
- **C fails:** Sync automation applies an upstream change that breaks the engine build;
  a snapshot drifts from upstream and "same" crate becomes three different codebases;
  missing upstream rev/digest labels produce unlabeled divergence that violates the
  no-unlabeled-% honesty rule.

## Recommendation (not a decision)

**Recommend Model A.** It is the only model consistent with the current hub-authority law
("depend only on hub contract crates", "pin hub by pushed origin/main rev") and with the
epic's static-link/ship-separate framing: foundation crates static-link into each engine
binary from the hub, while each engine ships separately against its own immutable pin.
Model B reverses the dependency direction the law exists to enforce. Model C trades
provenance and single-ownership for offline convenience that the pinned-git + `Cargo.lock`
model already provides.

**This document does not decide.** The owner must approve a model (checklist below)
before extraction implementation starts. Evidence-only beads `tokenzero-9s32.2`
(inventory) and `tokenzero-9s32.3` (acceptance gates) can refine the decision record
without selecting or implementing a model.

## Decision checklist (owner)

1. Approve Model A, B, or C explicitly (default recommendation: A).
2. Accept the hub as sole owner of foundation crates (A), engine-to-engine ownership
   (B), or per-engine snapshot ownership plus a named sync operator (C)?
3. Accept engine-to-hub-only dependency direction as permanent (A), allow peer imports
   (B), or replace dependency edges with provenance-locked snapshots (C)?
4. Is a crates.io release of infrastructure-only foundation crates in scope? If yes,
   after which stability milestone, and is the git rev kept canonical (A)?
5. Are vendored copies acceptable under the no-unlabeled-% provenance rule, and who
   operates the sync automation and its per-engine gate (C)?
6. MSRV/toolchain policy for foundation crates: nightly per engine, or declared stable
   MSRV at crates.io release (A)?
7. Confirm the rollback boundary below is acceptable for the chosen model.
8. Record the decision in this file (status -> APPROVED:<model> / REJECTED) with the
   owner's name and date.

## Rollback boundary

- **Model A:** the boundary is the pin line in each engine's `Cargo.toml` plus its
  `Cargo.lock`. Any pin-only change is a one-line revert per engine; no migration, no
  data change; binaries built against the prior rev remain runnable because the crates
  are static-linked per engine. A foundation change that alters engine behavior (not
  just compilation) requires a separate owner-approved release note before the pin bump.
- **Model B:** the boundary is coordinated repins across three repos; a partial rollback
  leaves mixed engine states. Rejected as default for that reason, per recommendation.
- **Model C:** the boundary is the vendored snapshot directory plus its sync log; rollback
  restores the prior snapshot, and the sync tooling must honor an explicit skip so it does
  not re-apply the reverted upstream change.

## Out of scope (this RFC)

- Extraction seams, inventory, or LOC/behavior gates: beads `tokenzero-9s32.2` / `9s32.3`.
- Any edit to engine crates, hub crates, `Cargo.lock`/`Cargo.toml` dependency lines,
  fuzz lock, or bead state. This document is docs-only.
