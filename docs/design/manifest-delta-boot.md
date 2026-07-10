# Manifest + Delta Boot Design

## 1. Problem statement

Current TokenZero session boot sends a full snapshot of the recovery cache, ref
index, and root metadata. In a long-running agent session this costs thousands
of tokens before the first user prompt is processed. Every new session also
re-sends the same stable content (allowed roots, repo layout, pinned docs), so
the prefix of the context is not cache-friendly on the model side. Goal: keep
the boot message under ~100 tokens and demand-page everything else.

## 2. Manifest line design

One line, no newlines, no unbounded fields, deterministic order.

```
TZ/1 root=<root_digest> m=<manifest_id> v=<store_version> t=<unix_ms> s=<sort_order_hint>
```

Fields:

- `TZ/1` — protocol version; stable for the whole major release.
- `root=<root_digest>` — short digest of the project root path, workspace
  identifiers, and allowed roots. Computed from sorted, normalized values.
- `m=<manifest_id>` — content-addressed ID of the current manifest blob; tells
  the server which head was current when this session opened.
- `v=<store_version>` — recovery-store schema version.
- `t=<unix_ms>` — session open timestamp, strictly advisory.
- `s=<sort_order_hint>` — a small per-session nonce so the provider can sort
  parallel sessions deterministically without reusing it for logic.

Byte-stability strategy:

- All hash values are truncated to a fixed short ID (12 chars) so the line
  length is independent of root or blob size.
- Field order is fixed and lexicographic.
- Values are escaped with URL-safe base64; no spaces or newlines.
- A root whose path, allowed roots, or pinned configs are unchanged produces
  exactly the same `root` value across sessions, so the provider prefix cache
  matches.

## 3. Delta ref computation

The delta ref is a single short token pointing to the difference between the
manifest and the current store state.

```
d=<delta_ref>
```

- The manifest is a content-addressed blob containing the frozen reference set
  expected to be common (repo map head, allowed roots, frequently recovered refs,
  schema version).
- The delta ref is computed by serializing the current store state as a small
  delta against the manifest, content-addressing the result, and truncating to
  12 characters. If the store state equals the manifest, the delta ref is the
  all-zero placeholder `d=0` and the manifest alone is sufficient to boot.
- The delta omits full payloads; it only contains changed/added/deleted refs
  and metadata since the manifest was pinned. Actual bytes are faulted in on
  first use.

Wire examples:

```
# Full state boot (today)
TZ/1 {"root":"/Users/aditya/AI/TokenZero","refs":["tz://file/abc...","tz://blob/def...",...],"version":7,"ts":...

# Manifest + delta boot (target)
TZ/1 root=a3f7b2c9d1e8 m=9c4e2a1b8d6f v=7 t=1752357600000 s=01 d=2e5a8b4c9d7e
```

## 4. Demand-paging fault path

When the agent needs a ref not present in the manifest/delta, the runtime
issues a fault:

1. Client sends a single-line request:
   ```
   fault <session_id> <ref> <selector>
   ```
2. Server resolves the ref in the recovery store, renders the visible
   capsule (or raw bytes), and returns it with the exact recovery ref attached.
3. The returned value is added to the session delta lazily; it is not
   re-sent on the next boot unless the server chooses to promote it to the
   manifest.
4. If the ref is missing, the server returns a typed `X0` short response
   so the agent can decide whether to re-read from the origin.

Faults are batched when the client sends several refs at once; the server
returns a compact multi-line capsule where each line is either the ref payload
or a missing-ref marker.

## 5. Wire examples

### Before (full state boot)

```
→ tokenzero session-open
← TZ/1 {"root":"/Users/aditya/AI/TokenZero","allowed_roots":["/Users/aditya/AI/TokenZero"],"refs":["tz://file/abc123...","tz://blob/def456...","tz://run/ghi789..."],"version":7,"ts":1752357600000}

# Boot cost: ~180-220 tokens depending on ref count.
```

### After (manifest + delta)

```
→ tokenzero session-open
← TZ/1 root=a3f7b2c9d1e8 m=9c4e2a1b8d6f v=7 t=1752357600000 s=01 d=2e5a8b4c9d7e

→ fault sess-01 tz://file/abc123 --lines 1-40
← tz://file/abc123 lines 1-40 | <capsule>
```

### After (no changes since manifest)

```
→ tokenzero session-open
← TZ/1 root=a3f7b2c9d1e8 m=9c4e2a1b8d6f v=7 t=1752357600000 s=02 d=0

# No faults needed; the entire session prefix is cache-friendly.
```

## 6. Token budget per component

| Component | Tokens | Notes |
| --- | --- | --- |
| Manifest line | ~18-22 | Fixed number of short fields, fixed delimiters. |
| Delta ref | ~2-3 | One short token; `d=0` is one token. |
| Fault request | ~4-6 | `fault <sess> <ref> <selector>`; refs are short. |
| Fault response | ~10-30 | Visible capsule; exact ref is not counted until expanded. |
| Full state boot | ~180-220+ | Grows with ref count. |
| Sub-100 token boot | yes | Manifest + delta + no immediate faults stays under 100. |

## 7. Implementation sketch

### Server side

- Add `manifest/` store namespace: `manifest/<manifest_id>` is an immutable
  blob of frozen ref metadata.
- On `session-open`, compute `root` from sorted config, load the current head
  manifest, compute delta against current store state, return one-line
  manifest + delta.
- Cache the manifest line keyed by `(root, manifest_id)` for prefix cache
  sharing across sessions.
- Implement `fault` handler: accept refs, optional selectors, return compact
  capsules; promote hot-faulted refs to a new manifest asynchronously if
  configured.

### Client side

- Parse the manifest line, store `manifest_id`, `delta_ref`, and `session_id`.
- Treat refs not listed in the manifest/delta as absent until faulted; do not
  eagerly pull them.
- Batch fault requests across consecutive tool calls where possible, e.g.
  `fault <sess> <ref1> <ref2> <ref3>`.
- On reconnect, send the previous `manifest_id` in the `prev_m` field so the
  server can compute a minimal incremental delta if the head has advanced.

### Wire format additions

```
# Session open (client may send prev_m to reuse prior manifest)
TZ/1 open root=a3f7b2c9d1e8 prev_m=9c4e2a1b8d6f

# Server response
TZ/1 root=a3f7b2c9d1e8 m=9c4e2a1b8d6f v=7 t=1752357600000 s=01 d=2e5a8b4c9d7e

# Fault request (single)
fault sess-01 tz://file/abc123 --lines 1-40

# Fault request (batched)
fault sess-01 tz://file/abc123 tz://blob/def456 tz://run/ghi789
```

### Open questions / follow-ups

- Manifest eviction policy: when a manifest head ages out, the server must
  keep the blob until no active session references it.
- Cross-session promotion: decide whether to advance the head manifest based
  on fault frequency or explicit agent signals.
- Exact short-ID collision handling: define a deterministic fallback that
  expands the ID by two characters until unique, while keeping the manifest
  line length bounded.
