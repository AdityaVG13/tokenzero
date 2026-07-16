# Shared-CAS derivation provenance — vNext RFC (coordination)

Status: **frozen** as `zerostack.derivation-provenance.v1`  
Owner: TokenZero (schema authority for `zerostack.cas-gc.*` and derivation provenance)
Proposing engine: GraphZero (`graphzero-3wbh` family)
Tracking bead: `tokenzero-cas-gc-vnext-provenance-2yis`
Related: frozen `schemas/shared-cas-gc/v1/` (`tokenzero-9ap`), GraphZero opt-in slice (`graphzero-3wbh` / `.1` / `.2`), frozen bundle `schemas/derivation-provenance/v1/`

RFC 2119 terms are normative for the freeze. This document is the coordination history + decision record; the normative contract lives at `schemas/derivation-provenance/v1/SPEC.md`.

## 1. Why this RFC exists

`zerostack.cas-gc.v1` is frozen and MUST remain byte-stable. GraphZero shipped an opt-in, engine-private derivation provenance slice so agents can answer WHY a derived row exists (edges, outline spans, semantic chunks, capsules) without touching v1 GC discovery paths.

This RFC coordinated the **shared record shape** GraphZero already emits so TokenZero could publish an official freeze without silent unfreeze of cas-gc.v1.

## 2. Non-goals (this coordination slice)

- Do **not** mutate `schemas/shared-cas-gc/v1/` schemas, fixtures, or `SPEC.md`.
- Do **not** teach v1 collectors to discover provenance under `gc/roots|pins|leases`.
- Do **not** require FSZero / TokenZero producers in this bead.
- Do **not** change retain-on-uncertainty for unknown schema versions under `gc/`.

## 3. Candidate shared record shape

GraphZero field set (source of truth today: `graphzero-store` `ProvenanceRecord`):

| Field | Type | Meaning |
| --- | --- | --- |
| `schema_version` | string | freeze id below |
| `record_type` | const | `derivation-provenance` |
| `row_id` | 64-hex | stable SHA-256 identity of the derivation |
| `derived_kind` | string | e.g. `graph_edge`, `outline_span`, `semantic_chunk`, `query_capsule` |
| `derived_ref` | string | expandable evidence ref (`gz://blob/<hash>#B…`, etc.) |
| `source_blob_digest` | 64-hex | source blob digest (lowercase) |
| `byte_span` | `{start,end}` | half-open byte offsets in the source blob |
| `line_span` | optional `{start,end}` | 1-based inclusive lines |
| `producing_engine` | enum | `tokenzero` \| `fszero` \| `graphzero` |
| `producing_commit` | string | engine commit / package pin |
| `transform_id` | string | stable transform id (engine-prefixed) |
| `created_at` | date-time | RFC3339 |
| `edge_src` / `edge_dst` / `edge_kind` | optional | graph_edge endpoints |

### Freeze id candidates (decided in §8)

1. **Preferred for provenance-only freeze:** `zerostack.derivation-provenance.v1`  
   Keeps GC metadata (`zerostack.cas-gc.v1`) orthogonal; collectors that only scan `gc/` stay unchanged.
2. **Alternate if folded into CAS-GC:** `zerostack.cas-gc.v2` with additive `record_type: derivation-provenance` **and** an explicit collector discovery rule (must not place unsupported versions under `gc/` until collectors bump).

GraphZero previously emitted `schema_version: "zerostack.cas-gc.vnext-provenance"`. That string is a **proposal tag**, not a freeze. On official freeze, GraphZero MUST migrate emitters to the chosen const.

## 4. Storage and GC interaction (phased)

### Phase A — current / safe with v1 collectors (already shipped in GraphZero)

Store under engine-private paths, e.g.:

`<store-root>/<engine>/provenance/<row_id>.json`

v1 collectors discover only `gc/roots|pins|leases|reports`. Engine-private provenance MUST NOT appear there. Source-blob retention for derived rows remains the engine's responsibility (pins/roots) until Phase B.

### Phase B — optional GC retention (deferred)

If provenance becomes a GC input, collectors MUST:

- discover an explicit, versioned path grammar (proposed: `<store-root>/gc/provenance/<engine>/<project_id>/<row_id>.json`);
- treat `source_blob_digest` as a retain root while the provenance record is live;
- retain-on-uncertainty for corrupt/unsupported provenance metadata the same way as v1 roots/pins/leases;
- never require reading `<store-root>/<engine>/` private DBs.

Phase B is **out of scope** for this freeze.

## 5. Draft machine-readable schema

Historical draft: colocated `derivation-provenance.schema.json` and `fixtures/`. Normative freeze: `schemas/derivation-provenance/v1/`.

## 6. Acceptance for the TokenZero freeze bead

Close the freeze bead only when **all** are true:

1. TokenZero publishes under `schemas/shared-cas-gc/` (or a sibling `schemas/derivation-provenance/v1/`) a frozen `SPEC.md` + JSON Schema + golden fixtures; v1 cas-gc bundle remains untouched.
2. Freeze chooses and documents exactly one of the schema ids in §3; GraphZero / FSZero / TokenZero migration notes are linked.
3. SPEC states Phase A storage is conforming; Phase B GC discovery is either deferred with explicit "not yet" or fully specified with retain-on-uncertainty tests.
4. Fixture validator passes (mirror `v1/fixtures/validate_fixtures.py` pattern).
5. Sibling beads exist or are updated: GraphZero migrates `PROVENANCE_SCHEMA_VERSION`; FSZero notes N/A or adopts when it emits derivations.
6. No silent edit to `zerostack.cas-gc.v1` consts, paths, or fixtures.

## 7. GraphZero evidence (already landed; not this RFC)

- Opt-in: `GRAPHZERO_PROVENANCE=1` or `ZEROSTACK_PROVENANCE=1`
- Attach paths: overlay edges, indexer shard edges, outline/semantic/capsule transforms
- Surfaces: verify WHY + doctor orphaned derivations
- Local proposal copy: `crates/graphzero-store/tests/fixtures/shared-cas-gc/vnext/SPEC.md`

## 8. Decision record (freeze)

| Decision | Choice | Date | Bead |
| --- | --- | --- | --- |
| Schema id | **`zerostack.derivation-provenance.v1`** (preferred; orthogonal to cas-gc.v1; rejected additive `zerostack.cas-gc.v2`) | 2026-07-16 | `tokenzero-cas-gc-vnext-provenance-2yis` |
| Phase B GC discovery | **deferred** (Phase A engine-private `<engine>/provenance/` remains conforming) | 2026-07-16 | `tokenzero-cas-gc-vnext-provenance-2yis` |
| Normative bundle | `schemas/derivation-provenance/v1/` (`SPEC.md` + schema + fixtures) | 2026-07-16 | `tokenzero-cas-gc-vnext-provenance-2yis` |
| GraphZero migration | Follow-on: migrate `PROVENANCE_SCHEMA_VERSION` from `zerostack.cas-gc.vnext-provenance` → `zerostack.derivation-provenance.v1` | 2026-07-16 | `graphzero-iubq` |
| FSZero | N/A until FSZero emits derivations | 2026-07-16 | — |

**Rationale:** Keeping provenance under its own freeze id avoids any implication that cas-gc collectors must understand a new record type, and leaves `zerostack.cas-gc.v1` byte-stable. Folding into `zerostack.cas-gc.v2` would force collector discovery rules before engines need GC retention of provenance.
