# Shared derivation provenance contract v1

Bead: `tokenzero-cas-gc-vnext-provenance-2yis`

This document freezes the engine-neutral derivation provenance record identified by `zerostack.derivation-provenance.v1`. TokenZero owns the contract. GraphZero is the first producer (opt-in); FSZero and TokenZero MAY adopt when they emit derivations. RFC 2119 terms are normative.

This freeze is **orthogonal** to `zerostack.cas-gc.v1`. It does **not** revise, extend, or unfreeze the CAS-GC metadata bundle at `schemas/shared-cas-gc/v1/`.

## 1. Scope

A derivation provenance record explains WHY a derived row exists: which source blob span, which engine/transform, and which expandable evidence ref. It is not a GC root, pin, lease, or dry-run report under the cas-gc.v1 path grammar.

## 2. Schema identity

| Field | Requirement |
| --- | --- |
| `schema_version` | MUST be the exact string `zerostack.derivation-provenance.v1` |
| `record_type` | MUST be the exact string `derivation-provenance` |

The proposal tag `zerostack.cas-gc.vnext-provenance` is **not** a freeze id. Emitters MUST migrate to `zerostack.derivation-provenance.v1`. Consumers MUST reject unknown `schema_version` values for this record type.

Machine-readable schema: `derivation-provenance.schema.json`.

## 3. Required fields

| Field | Type | Meaning |
| --- | --- | --- |
| `row_id` | 64-hex | stable SHA-256 identity of the derivation |
| `derived_kind` | string | e.g. `graph_edge`, `outline_span`, `semantic_chunk`, `query_capsule` |
| `derived_ref` | string | expandable evidence ref (`gz://blob/<hash>#B…`, etc.) |
| `source_blob_digest` | 64-hex | lowercase source blob digest |
| `byte_span` | `{start,end}` | half-open byte offsets in the source blob (`0 ≤ start ≤ end`) |
| `producing_engine` | enum | `tokenzero` \| `fszero` \| `graphzero` |
| `producing_commit` | string | engine commit / package pin |
| `transform_id` | string | stable transform id (engine-prefixed) |
| `created_at` | date-time | RFC3339 |

## 4. Optional fields

| Field | Type | Meaning |
| --- | --- | --- |
| `line_span` | `{start,end}` | 1-based inclusive lines |
| `edge_src` / `edge_dst` / `edge_kind` | string / string / 0–255 | graph_edge endpoints |

`additionalProperties` MUST be false. Unknown keys invalidate the record.

## 5. Phase A storage (conforming now)

Store under engine-private paths only:

```
<store-root>/<engine>/provenance/<row_id>.json
```

Example: `<store-root>/graphzero/provenance/<row_id>.json`.

v1 CAS-GC collectors discover only `gc/roots|pins|leases|reports`. Engine-private provenance MUST NOT appear there. Source-blob retention for derived rows remains the producing engine's responsibility (pins/roots) until Phase B.

## 6. Phase B GC discovery (deferred)

Phase B is **not** part of this freeze. Collectors MUST NOT discover provenance under `gc/provenance/...` until a later freeze adds collector rules and retain-on-uncertainty tests. Unsupported schema versions MUST NOT be placed under `gc/` discovery paths before collectors bump.

## 7. Conformance

- A conforming Phase A producer writes only the frozen schema id, validates against `derivation-provenance.schema.json`, and stores records under `<engine>/provenance/`.
- A conforming consumer treats records with any other `schema_version` as non-conforming for this contract.
- Sibling migration: GraphZero MUST migrate `PROVENANCE_SCHEMA_VERSION` from the proposal tag to `zerostack.derivation-provenance.v1` (bead `graphzero-iubq`). FSZero: N/A until it emits derivations.

## 8. Fixtures

Golden fixtures and a stdlib validator live under `fixtures/`. Run:

```bash
python3 schemas/derivation-provenance/v1/fixtures/validate_fixtures.py
```

## 9. Coordination history

Draft proposal lived at `schemas/shared-cas-gc/vnext-provenance/` (SHA `4926a5b`). Decision record: that RFC §8.
