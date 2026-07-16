# Shared entity novelty contract v1

Bead: `graphzero-entity-refs-lfoo.5`

This document freezes the engine-neutral **known-entity novelty set** identified by
`zerostack.entity-novelty.v1`. TokenZero owns the contract file. GraphZero owns
entity identity minting (`EntityId` / `gz://entity/<64-hex>`). RFC 2119 terms are
normative.

## 1. Scope

A novelty record answers: which knowledge facts (entities) has this scope already
paid for? It upgrades byte-level seen-sets to know-this-fact across GraphZero and
TokenZero when they share a ZeroStack store / CAS root.

This freeze is **orthogonal** to `zerostack.cas-gc.v1` and
`zerostack.derivation-provenance.v1`. It does **not** revise ZeroRef v1 portable
blob grammar.

## 2. Non-goals / hard rules

- **Do not invent a second entity namespace.** There is no `tz://entity/` and no
  FSZero entity scheme. The only entity ref grammar is GraphZero-owned
  `gz://entity/<64-lowercase-hex>`.
- Portable ZeroRef v1 remains **blob-only**. Entity refs MUST be treated as
  `unsupported` by ZeroRef v1 parsers.
- Mutable novelty metadata MUST NOT live inside `blobs/sha256/…`. CAS may hold an
  immutable **snapshot** of a novelty record; the live pointer is the path below.

## 3. Schema identity

| Field | Requirement |
| --- | --- |
| `schema_version` | MUST be the exact string `zerostack.entity-novelty.v1` |
| `record_type` | MUST be the exact string `entity-novelty` |

Machine-readable schema: `entity-novelty.schema.json`.

## 4. Required fields

| Field | Type | Meaning |
| --- | --- | --- |
| `scope_key` | string | Stable scope spelling (`session:…`, `repo:…`, `workspace:…`, `global`) |
| `entity_ids` | array of 64-hex | GraphZero `EntityId` digests (sorted unique lowercase) |
| `producing_engine` | enum | `tokenzero` \| `fszero` \| `graphzero` (last writer) |
| `updated_at` | date-time | RFC3339 |

## 5. Optional fields

| Field | Type | Meaning |
| --- | --- | --- |
| `cas_digest` | 64-hex | SHA-256 of the last immutable novelty snapshot published via SharedCas |

`additionalProperties` MUST be false.

## 6. Storage

Live pointer (mutable, merge-friendly):

```text
<store-root>/shared/entity-novelty/v1/<scope_digest>.json
```

`scope_digest` is the lowercase SHA-256 of the UTF-8 `scope_key` bytes.

Optional CAS snapshot: publish the same JSON bytes via SharedCas
(`blobs/sha256/<hh>/<hash>`) and record `cas_digest`. Readers MAY verify the
pointer body against `cas_digest` when present.

## 7. Merge semantics

Writers MUST union `entity_ids` (set union), rewrite `producing_engine` /
`updated_at`, and MAY refresh `cas_digest`. Never delete foreign ids on merge
unless the scope is explicitly cleared by the owning session.

## 8. Conformance

- Producers write only GraphZero EntityId hex strings (no scheme prefix in the
  array). Display/ref form for agents is always `gz://entity/<id>`.
- Consumers MUST reject unknown `schema_version` values and MUST reject any
  attempt to interpret a `tz://entity/` or `fz://entity/` ref as an entity id.
- Sibling: GraphZero bead `graphzero-entity-refs-lfoo.5` implements the first
  producer/consumer over SharedCas.
