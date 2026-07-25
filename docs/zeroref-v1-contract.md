# ZeroRef v1 Contract

Bead: `tokenzero-zeroref-v1-shared-cas-cqr.2`

This document defines the canonical ZeroRef v1 contract for portable blob references in TokenZero.

## 1. Portable scope

The only portable ZeroRef v1 refs are blob refs of the form:

```
(tz|fz|gz)://blob/<full-hash>[#<fragment>]
```

Execution, error, session, file, graph, index, and unit refs remain engine-specific and are NOT covered by this contract. They may resolve inside a single engine but must not be treated as portable across engines or stores.

## 2. Identity

The blob identity is the full lowercase 64-hex-character SHA-256 digest of the complete, unfragmented bytes.

- Emit the full 64-hex hash.
- Reject short hashes, prefix hashes, uppercase hex, non-hex characters, and any extra path segments.

## 3. Fragment selectors

### 3.1 `#Bstart-end` — byte range

- Zero-based, half-open range `[start, end)` over the complete object bytes.
- `start == end` is allowed and yields an empty fragment.
- Reversed ranges (`start > end`) and ranges where `end` exceeds the byte length are rejected.
- Checked arithmetic must be used; overflow is rejected.

### 3.2 `#Lstart-end` — line range

- One-based, inclusive range (`start..=end`).
- Exact newline retention is required; a line slice ending on a blank line must keep that line's trailing newline.
- `start == 0`, reversed ranges (`start > end`), and a start past the available line count are rejected.
- An end past EOF clamps to the final available line when the start is valid. The empty blob has zero lines, so every line start is out of bounds.

## 4. Digest verification

Before applying any fragment selection, the consumer must verify the complete-object digest against the stored bytes. Fragment selection is only valid after the full object has been authenticated.

## 5. Stable error taxonomy

| Error | Meaning |
|---|---|
| `malformed` | Ref string is not structurally a valid ZeroRef v1 blob ref. |
| `unsupported` | Scope is not a portable blob ref. |
| `missing` | Object is not present in the store. |
| `io` | Underlying read operation failed. |
| `corruption` | Complete-object digest verification failed. |
| `policy` | Expansion denied by policy. |
| `incompatible_version` | Ref version is incompatible with this consumer. |
| `legacy_ambiguity` | Legacy short/prefix ID cannot be disambiguated under v1 rules. |

## 6. Scheme semantics

The scheme (`tz`, `fz`, `gz`) denotes the producer or provenance of the ref. It does not imply authorization by itself. Cross-engine **blob** expansion is supported when engines share a verified ZeroStack shared CAS (or sibling recovery store under the same unified root); evidence is retained in `fixtures/zeroref-conformance-evidence.json`. Non-blob refs are not portable.

## 7. Effective root precedence and isolation

Resolution is fail-isolated by default. The executable contract is
`crates/tokenzero/tests/store_root_precedence.rs`:

1. Explicit `--cache-path` wins, then `TOKENZERO_CACHE_PATH`.
2. A project-local `<root>/.zerostack` wins over any global store pin, even
   when shared mode is enabled.
3. `ZEROSTACK_STORE_ROOT` (or legacy `ZERO_STACK_STORE_ROOT`) is honored only
   with an explicit truthy `TOKENZERO_SHARED_STORE` or
   `ZEROSTACK_SHARED_STORE` opt-in. A relative pin resolves against the project
   root.
4. Without a project-local unified root or explicit shared opt-in, TokenZero
   remains in its legacy per-root `.tokenzero/recovery-cache.json`; unrelated
   roots, including roots with the same basename, remain isolated.

`tokenzero doctor --json` reports `effective_store_root`,
`effective_cache_path`, `isolation_mode`, `shared_store_opt_in`, and mismatch
advice. A configured global pin that lacks opt-in is reported and ignored.

## 8. Legacy migration and rollback

Legacy short-ref compatibility is enabled by default only for the compatibility
window through `tokenzero-v2.0`; doctor reports the effective legacy flag,
deadline, legacy blob count, and session read count. Migration is dry-run first:

```text
tokenzero cache migrate-refs --json
tokenzero cache migrate-refs --apply --json
tokenzero cache migrate-verify --json
tokenzero cache migrate-rollback --json
tokenzero cache migrate-rollback --apply --json
```

Migration records a manifest beside the recovery cache. Rollback removes
migration aliases and the manifest, never CAS objects or source bytes. Cleanup
is a separate irreversible operation and is not part of the rollback path.

## 9. Capability negotiation

Consumers must inspect the MCP capability descriptor's `zeroref_v1` object
rather than infer support from a URI scheme. Evidence-backed support requires
`enabled`, `shared_cas`, `blob_ref_expand`, and `cross_engine` to be true;
`portable_ref_kinds` must be exactly `["blob"]`; `fragment_selectors` must list
`#B` and `#L`; and `unsupported_portable_ref_kinds` names execution, error,
session, file, graph, index, and unit refs. The descriptor's limitations keep
correctness separate from deferred performance claims.

The boundary policy is fixed by ZeroRef v1: byte endpoints and line starts are
strict, while line ends clamp. It is not a per-engine negotiable capability, so
the descriptor advertises the ZeroRef version and selectors but must not add a
`clamp_policy` or `selection_policy` field.

## 10. Multi-OS evidence gate

Cross-engine **blob** claims require retained real-binary evidence. The authoritative
workflow is `.github/workflows/ci.yml`:

1. Its macOS, Linux, and Windows jobs build pinned FSZero and GraphZero revisions
   plus the candidate TokenZero revision, then run `zeroref_conformance_matrix`
   with an explicit native-OS filter. Each job retains the commands, binary
   paths, versions, commits, binary SHA-256 values, refs, hashes, and results.
2. `.github/scripts/aggregate_zeroref_evidence.py` rejects a missing or duplicate
   OS shard, an unknown/unpinned sibling commit, malformed binary or payload
   hashes, skipped/failed 3×3 blob or fragment rows, and failed wrong-store or
   concurrent-writer checks. The merged artifact is retained by CI.
3. Release gates depend on that fail-closed aggregation job. A host-only run is
   diagnostic evidence and cannot authorize a release.

The retained fixture is a reproducible snapshot, not a substitute for the CI
artifacts from the candidate revision. Correctness evidence does not authorize
zero-copy, latency, or performance marketing; those claims remain deferred to
`tokenzero-9pb` and `tokenzero-485` evidence.

## 11. Release checklist

A release is blocked unless the dependency chain in `.github/workflows/ci.yml`
completes:

- `rust-core` runs formatting, workspace tests, clippy, shell verification, and
  platform verification on macOS, Linux, and Windows.
- Each native job runs the real three-binary matrix and all eight lifecycle
  smokes, then uploads both evidence documents. Missing artifacts are errors.
- `zeroref-conformance-gate` runs the negative self-tests, requires all three
  pinned native shards, emits the generated `claim_evidence` mapping, and
  retains the merged green artifact.
- `rust-release-gates` cannot start until aggregation succeeds. Its existing
  help/doctor/golden-output tests are part of the workspace test gate; it also
  performs source/module audits, release build, doctor, package audits, package
  dry-runs, MCP smoke, and install/rollback smoke.

No required row may be skipped or synthetically retagged. Sibling commits,
binary SHA-256 values, command paths, refs, payload hashes, lifecycle cells, and
the generated claim map are retained in the candidate's artifacts.
