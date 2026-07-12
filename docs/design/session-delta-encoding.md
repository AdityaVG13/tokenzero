# Session Delta Encoding

> Wire protocol for shipping only what the model has not already seen in a
> TokenZero session.

---

## 1. Problem statement

TokenZero stores every recoverable object in a content-addressed store and
returns compact capsules containing `tz://` refs instead of raw bytes. Over a
long session the same blobs can be returned many times:

- A file read on turn 1 becomes a `tz://file/<sha>` capsule.
- The same file is re-read, grepped, or recalled on turn 5.
- The model already knows the blob; re-shipping the same ref or header is pure
  overhead.

The goal is to send, on each turn, **only the delta of store objects the model
has not seen before**, while preserving exact recovery semantics and remaining
robust to session resume, store garbage collection, and multi-agent sharing.

A naive per-turn cache is not enough: refs may be returned in compact summaries
or as part of search results, and the model can recover an object via `expand`
long after it was first mentioned. Delta tracking must be **session-scoped**,
**content-addressed**, and **cheap to compute**.

---

## 2. High-water mark mechanism

### 2.1 Per-session high-water mark

Each session maintains a single monotonic integer called `session_hwm`. It is
a high-water mark over the content-addressed store, not a timestamp, because
store ids are ordered insertion counters (or lexicographically sortable ULIDs).

- `session_hwm` is the largest store id the model has been told about in this
  session, across all turns and all tools.
- It is initialized to the id of the newest object in the store at session
  creation time, or `0` for a fresh store.
- After each turn the server raises `session_hwm` to the max id of every object
  whose ref appeared in the response.

```rust
struct SessionState {
    session_hwm: StoreId,           // largest id the model has seen
    seen_blobs: HashSet<BlobSha>,   // all blob shas mentioned in any turn
}
```

### 2.2 Why both a high-water mark and a seen-blob set?

The high-water mark catches the common case: new objects appended to the store
after the model's last turn. The seen-blob set handles the exceptions:

- **Backfills / GC / compaction**: a newly stored object may receive an id below
  the current `session_hwm` because it reuses a slot, was imported from another
  cache, or is the result of deduplication that mapped to an older id.
- **Multi-agent stores**: another agent may write objects that the current model
  has not seen, even if their ids are below the current `session_hwm`.

### 2.3 Updating the mark after each turn

For every compact output sent to the model, the server extracts all refs that
identify recoverable blobs:

- `tz://blob/<sha>`
- `tz://file/<sha>`
- `tz://unit/<sha>`
- `tz://hit/<sha>` (search hit summaries)

The server computes:

```rust
for ref in response.refs() {
    seen_blobs.insert(ref.sha);
    session_hwm = max(session_hwm, ref.store_id);
}
```

Refs that are only path-only (e.g., `tz://glob`, `tz://tree`) are not
recoverable byte objects and do not update `session_hwm`.

---

## 3. Delta ref format (wire protocol with examples)

### 3.1 Delta message envelope

Each turn response is wrapped in a delta-aware envelope. The server may choose
to send the full response or a delta; the client/MCP layer applies the delta to
reconstruct the full view.

```json
{
  "delta": {
    "from_hwm": 1047,
    "to_hwm": 1083,
    "new_refs": ["tz://file/1081", "tz://blob/1082", "tz://hit/1083"],
    "replaced_refs": ["tz://file/1042"],
    "new_entries": [
      {
        "ref": "tz://file/1081",
        "sha": "aabbcc...",
        "store_id": 1081,
        "kind": "file",
        "capsule": "// 312 lines collapsed; recover with expand tz://file/1081"
      }
    ]
  }
}
```

### 3.2 Compact delta-only turn example

Turn 1: the model asks for a large file. The server returns the full file as a
new entry because the session has not seen it yet.

```json
{
  "delta": {
    "from_hwm": 1000,
    "to_hwm": 1001,
    "new_refs": ["tz://file/1001"],
    "new_entries": [
      {
        "ref": "tz://file/1001",
        "sha": "7d2f1a...",
        "store_id": 1001,
        "kind": "file",
        "capsule": "// 1,728 lines collapsed to 150 tokens; recover with expand tz://file/1001"
      }
    ]
  }
}
```

Turn 5: the model re-reads the same file and then runs a grep that hits it.
Because the blob sha is already in the session's `seen_blobs` set, the server
emits only a delta ref line, not the full capsule.

```json
{
  "delta": {
    "from_hwm": 1001,
    "to_hwm": 1005,
    "new_refs": ["tz://hit/1005"],
    "new_entries": [
      {
        "ref": "tz://hit/1005",
        "sha": "e9b4c2...",
        "store_id": 1005,
        "kind": "hit",
        "capsule": "3 matches in tz://file/1001 (already seen): ..."
      }
    ]
  }
}
```

The file itself is not re-shipped; the model sees only the hit summary.

### 3.3 Delta ref statement syntax

A delta ref is a single-line statement that tells the model an object exists
and is already recoverable without re-shipping its contents:

```text
+tz://file/1001 7d2f1a... (already seen)
```

Rules for delta statements:

- The leading `+` indicates the object is present in the model's session view.
- The ref and sha are both present so the model can correlate with prior turns.
- The parenthetical `(already seen)` is optional; its presence is a hint to the
  model, not a protocol requirement.
- The client expands the ref on demand using the store; no bytes are sent now.

### 3.4 Full response vs. delta response

The server decides whether to send a full entry or a delta ref based on the
session state:

```text
if sha in seen_blobs or store_id <= session_hwm:
    emit "+tz://<kind>/<id> <sha> (already seen)"
else:
    emit full entry and update session_hwm
```

Note: `store_id <= session_hwm` is a fast path; a newly seen sha still requires
an explicit `+` line so the model knows the object is available.

---

## 4. Already-seen rules

An object is considered **already seen** for the current session if any of the
following is true:

1. Its **blob sha** appears in the session's `seen_blobs` set, regardless of
   which `tz://` kind or turn introduced it.
2. Its **store id** is less than or equal to `session_hwm` at the start of the
   turn.
3. It was emitted as a full entry earlier in the **same turn** (intra-turn
   deduplication).
4. The client has explicitly acknowledged the ref via an `expand` or
   `ack_seen` call in the same session.

### 4.1 Cross-kind sharing

Because `seen_blobs` is keyed by sha, a `tz://file/1001` and a later
`tz://blob/1001` pointing to the same bytes count as the same object. Search
hits that embed the same sha also reuse the same seen status.

### 4.2 Path-only refs

`tz://glob`, `tz://tree`, and `tz://list` results are not content-addressed
objects. They do not participate in delta encoding and are always sent in
full.

### 4.3 Acknowledged vs. merely visible

The model sees a ref line, but it may not yet have internalized it. A separate
`ack_seen` call (or an implicit `expand` request) lets the server promote the
sha from "visible this turn" to "known to the model." In practice, all refs
that appear in a turn are promoted at the end of the turn unless the client
opts into explicit acknowledgments.

---

## 5. Failure modes and recovery

### 5.1 Session resume

A session may be resumed after a crash, client disconnect, or a long-running
automation restart. The server has persisted `session_hwm` and `seen_blobs` in a
small per-session state file (e.g., `session-<uuid>.json` in the recovery cache
directory).

On resume:

1. Load the saved `session_hwm` and `seen_blobs`.
2. Validate that all shas in `seen_blobs` still exist in the store. If any are
   missing, mark them as **not seen** so the next mention will be sent as a full
   entry.
3. The next response may contain a mix of `+` delta lines and full entries for
   objects that were lost or newly added.

If no session state is recoverable, the server falls back to `session_hwm = 0`
and an empty `seen_blobs` set, effectively sending the full response on the
next turn. This is safe but expensive.

### 5.2 Store garbage collection between turns

The recovery cache may garbage-collect old objects to reclaim disk space. If a
sha in `seen_blobs` is removed, the model's view is stale. The server handles
this during resume validation and also at the start of each turn:

```rust
let still_present = store.metadata(sha)?.is_some();
if !still_present {
    seen_blobs.remove(sha);
    // If the object is mentioned again, it will be re-fetched and re-sent.
}
```

When a GC'd object is mentioned again, two outcomes are possible:

- The object can be recomputed (e.g., a file read can be re-executed). The server
  re-stores it and emits a full entry because the sha is no longer in
  `seen_blobs`.
- The object is lost and cannot be recovered. The server emits a tombstone line:
  ```text
  -tz://file/1001 7d2f1a... (unavailable)
  ```
  The model must treat the ref as no longer expandable.

### 5.3 Multi-agent stores

Multiple agents can write to the same recovery cache. Agent A may write a
blob with store id 500. Agent B's session was created when the newest id was
480, so its `session_hwm` is 480. Later, Agent B's turn sees the blob. Even
though `500 > 480`, Agent B's own session did not write it, so the delta logic
is unchanged: it is a new object for B and is sent as a full entry.

Conversely, if Agent B's `session_hwm` was advanced to 520 before seeing the
blob, the sha-based `seen_blobs` check still correctly sends the full entry
because Agent B has not seen the sha before. The protocol does not depend on
which agent wrote the object; it depends only on whether the current session
has seen the content.

### 5.4 Id backfill and compaction

After compaction, store ids may be rewritten. In that case the server remaps
session state:

- Old id-to-sha mappings are discarded; only the sha set matters.
- A ref is treated as "already seen" if its sha is in `seen_blobs`.
- If the new store id is lower than the current `session_hwm`, the sha check
  prevents incorrect suppression.

---

## 6. Token budget analysis

### 6.1 Cost of a full entry vs. a delta ref

Assuming a compact capsule line, the delta ref is much smaller than even a
small full entry.

| Item | Tokens (approximate) |
| --- | --- |
| Full file capsule with ref + sha + kind + summary | 50–250 |
| Full search hit with embedded snippets | 30–150 |
| Delta ref line `+tz://file/1001 7d2f1a...` | 8–12 |
| Multiple repeated refs in a long grep | 100s vs. 1 per line |

### 6.2 Savings on repeated operations

From the TokenZero demo metrics:

- A large file read costs ~150 visible tokens with a full capsule.
- Re-reading the same file in the same session costs only the delta ref line:
  ~10 tokens, a **93% reduction** on that repeated object.
- A repo-wide grep that returns many hits referencing the same files can avoid
  re-shipping each file; savings scale with the number of repeated objects.

### 6.3 State overhead

The per-session state is tiny. For a session with 10,000 unique blobs:

- `seen_blobs`: 10,000 × 32 bytes (sha256 prefix) ≈ 320 KB.
- `session_hwm`: 8 bytes.
- Persistent state file: ~400 KB JSON, negligible compared to the token savings.

### 6.4 Trade-off: explicit ack vs. end-of-turn promotion

Promoting all visible refs at end-of-turn is simple and correct for most
clients. It adds no extra wire cost. Explicit `ack_seen` adds a small request
overhead but allows the server to keep a smaller `seen_blobs` set if the client
wants to forget objects. Both modes are valid; the default is end-of-turn
promotion.

---

## 7. Implementation sketch

### 7.1 Key types

```rust
/// Stable identifier for a session. Usually a UUID generated by the MCP server.
pub type SessionId = Uuid;

/// Monotonic store id. The concrete type depends on the store backend
/// (u64 counter or ULID string).
pub type StoreId = u64;

/// Content hash of a recoverable object.
pub type BlobSha = [u8; 32]; // or a wrapped string for hashing

/// Per-session delta state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDeltaState {
    pub session_id: SessionId,
    pub session_hwm: StoreId,
    pub seen_blobs: HashSet<BlobSha>,
}

/// A single ref that can be sent either as a full entry or as a delta line.
#[derive(Debug, Clone)]
pub enum DeltaRef {
    Full {
        ref_string: String,
        sha: BlobSha,
        store_id: StoreId,
        kind: RefKind,
        capsule: String,
    },
    Delta {
        ref_string: String,
        sha: BlobSha,
        store_id: StoreId,
        kind: RefKind,
    },
}
```

### 7.2 Integration points

1. **MCP server session creation** (`crates/tokenzero-mcp/src/server.rs`):
   - On `initialize`, create a new `SessionDeltaState`.
   - Seed `session_hwm` from the current store's newest id.

2. **Tool response rendering** (`crates/tokenzero-core/src/capsule.rs`):
   - When rendering a compact output, collect all recoverable refs.
   - For each ref, decide `Full` or `Delta` based on `SessionDeltaState`.
   - After rendering, update `session_hwm` and `seen_blobs`.

3. **Recovery cache** (`crates/tokenzero-core/src/recovery.rs`):
   - Provide `store.newest_id()` and `store.exists(sha)` for delta checks.
   - Persist per-session state files on change.

4. **Session state persistence** (`crates/tokenzero-core/src/session.rs`):
   - Serialize `SessionDeltaState` to JSON after each turn.
   - On load, validate shas against the store and remove missing ones.

5. **Expand / ack handling**:
   - `expand tz://file/1001` adds the ref's sha to `seen_blobs` if it was not
     already present.
   - Optional `ack_seen` tool adds a set of shas without fetching bytes.

### 7.3 Minimal algorithm for a response

```rust
fn encode_response(
    state: &mut SessionDeltaState,
    store: &RecoveryStore,
    refs: &[RecoverableRef],
) -> Vec<DeltaRef> {
    let mut out = Vec::new();
    for r in refs {
        let already_seen = state.seen_blobs.contains(&r.sha)
            || r.store_id <= state.session_hwm
            || store.exists(&r.sha).is_none(); // missing = treat as not seen

        if already_seen && store.exists(&r.sha).is_some() {
            out.push(DeltaRef::Delta {
                ref_string: r.to_string(),
                sha: r.sha,
                store_id: r.store_id,
                kind: r.kind,
            });
        } else {
            out.push(DeltaRef::Full {
                ref_string: r.to_string(),
                sha: r.sha,
                store_id: r.store_id,
                kind: r.kind,
                capsule: render_capsule(r),
            });
            state.session_hwm = state.session_hwm.max(r.store_id);
            state.seen_blobs.insert(r.sha);
        }
    }
    out
}
```

Note: the `already_seen` branch above also checks `store.exists` because a
missing object must be emitted as a tombstone or re-sent in full if it can be
recomputed.

---

## 8. Open questions / follow-up

- Should the protocol support batched delta refs at the end of a multi-tool
  turn, or interleave them per-tool response? (Recommendation: per-tool, so
  the model can act on each tool as it returns.)
- Should the client send its own `session_hwm` to guard against a stale server
  state? (Optional; useful for distributed deployments.)
- How aggressively should the recovery cache GC objects that are still in an
  active session's `seen_blobs`? (Recommendation: keep until the session ends.)

