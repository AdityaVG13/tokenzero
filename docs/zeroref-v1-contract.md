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
- `start == 0`, reversed ranges (`start > end`), and out-of-bounds ranges are rejected.

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

The scheme (`tz`, `fz`, `gz`) denotes the producer or provenance of the ref. It does not imply authorization, storage location, or cross-engine resolution. Cross-engine expansion requires a verified shared-CAS adapter.
