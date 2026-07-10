# Table of Contents + Working Set

> Demand-paging model for "infinite context": a per-session table of contents
> (TOC) listing every recoverable object the session has touched, plus a
> bounded working set of objects currently resident in the model's prompt.

---

## 1. Problem statement

TokenZero already serves every recoverable object as a content-addressed
`tz://` ref (`tz://blob/<sha>`, `tz://file/<sha>`, `tz://unit/<sha>`, etc.)
and can deliver an exact byte range on demand. What's missing is the
**prompt-side discipline** that turns that capability into a real memory model:

- The model's context window is bounded. We cannot keep every ref a session
  has ever produced in the prompt.
- Objects that are not in the prompt must still be discoverable in bounded
  tokens (the user must be able to ask "what did we look at earlier?" without
  re-reading the world).
- When the model needs an evicted object, rehydration must be cheap, exact,
  and **partial** (line ranges, not whole files).
- The host agent loop is the source of truth for what the model "sees"; we
  cannot silently rewrite that contract.

This doc defines the TOC + working-set model that closes those gaps and the
contract we expose to agent frameworks.

---

## 2. TOC design

The **TOC** is the durable, per-session pointer table that lets anyone (model,
agent loop, future turn) recover what was touched in bounded tokens.

### 2.1 Structure

A single per-session JSON document, content-addressed so its own
evolution is replayable. Logical shape:

```jsonc
{
  "session_id": "tzs_2026_07_10_a3f1",
  "schema": "tz.toc/v1",
  "hwm": 4271,                      // monotonic, seeded from manifest
  "tot_bytes": 18_372_944,          // sum of byte_weight across entries
  "tot_tokens_est": 6128,           // prompt-resident cost right now
  "working_set": { "tokens_est": 9216, "ids": ["...", "..."] },
  "entries": [
    {
      "id": "E0001",
      "kind": "blob",               // blob | file | unit | search | snapshot
      "ref": "tz://blob/9af2…",
      "anchor": null,
      "byte_weight": 4096,
      "token_weight": 312,
      "summary": "AgentMail macro_start_session return body",
      "first_touched": 1,
      "last_touched": 87,
      "touch_count": 3,
      "importance": 0.81,            // model-decayed score
      "state": "resident",          // resident | paged | evicted
      "tags": ["agent-mail", "session-boot"]
    }
  ]
}
```

### 2.2 Entry kinds

| `kind` | When created | Default summary source |
|---|---|---|
| `blob` | raw bytes returned by `tz_read`, `tz_fetch`, `tz_shell` stdout, or `tz_ingest` payload | first ≤8 lines or content-type header |
| `file` | path resolved via `tz_glob` → backed by blob | first ≤8 lines |
| `unit` | semantic chunk (e.g. fn, paragraph, section) | chunk header |
| `search` | hit set from `tz_find` / `tz_grep` | query + top-k hits |
| `snapshot` | point-in-time TOC capture (`--checkpoint`) | TOC digest |

### 2.3 Token budget

The TOC must stay small enough that listing it costs less than rehydrating
anything from it.

| TOC slice | Budget | Notes |
|---|---|---|
| TOC header | ≤ 80 tok | session id, hwm, totals |
| Per-entry row | ≤ 28 tok average | kind + ref + 1-line summary |
| TOC delta (per turn) | ≤ 40 tok | only new/changed entries since last emit |
| Full TOC dump | ≤ 2 000 tok for ≤ 500 entries | hard cap; older entries collapse to ranges |

If the live TOC would exceed 2 000 tok, entries beyond the **k-core**
(graph-derived importance core, see §3.3) are compacted: summaries replaced
with a 6-byte prefix hash and `state=evicted`.

### 2.4 Lifecycle

- TOC is **append-mostly**: `entries[]` only grows; updates via monotonic
  `hwm`.
- TOC is itself content-addressed; the latest TOC `id` is always in the
  prompt header.
- On session boot, TOC arrives as a `tz://toc/<sha>` ref plus a 1-3k tok
  cached snapshot of the k-core only.
- A `--toc-checkpoint` flag freezes the current TOC into a `snapshot`
  entry; checkpoint events become `tz_ref`s the model can reference.

---

## 3. Working set selection

The **working set** is the subset of TOC entries currently resident in the
model's prompt, sized to a token budget.

### 3.1 What gets paged in

The working set always contains three pinned classes:

1. **Kernel** (≈ 600 tok) — TOC header, current `tz_ref` to latest TOC, schema
   digest, contact policy of the current agent.
2. **Hot tail** (≈ 6 000 tok) — the N most-recently-touched entries, where N
   is the budget / average entry cost.
3. **Anchor** (≈ 1 000 tok) — the top-k PageRank / eigenvector entries from
   the TOC's touch graph (see §3.3). Provides "what is important" context
   even if the model has not visited an entry recently.

Remaining budget (typically 0–24k tok) is filled by **adjacency** entries:
those sharing tags, callers, or file paths with currently-resident entries.

### 3.2 Sizing

Total `budget_working_set` is configured at agent start:

```
budget_working_set = model_ctx_window
                   - kernel_overhead
                   - response_reserve         // tokens reserved for the reply
                   - tool_io_reserve          // tokens reserved in-flight
                   - residual_margin          // safety floor, e.g. 512
```

Default `response_reserve = 4096`, `tool_io_reserve = 1024`. Models without
a context window declaration get a conservative `budget_working_set = 8192`.

### 3.3 Importance score

For each entry:

```
importance = α·recency      + β·touch_count_norm  + γ·graph_score
           + δ·user_anchor  + ε·kind_weight
```

- `graph_score` ∈ [0,1] — eigenvector centrality over the touch graph
  (entry → tagged entry, entry → caller/callee). Provided by
  `bv --robot-insights` style metric on the in-session graph.
- `user_anchor` ∈ {0, 1} — set when the user or agent loop has flagged
  the entry as durable.
- `kind_weight` — 1.0 for `snapshot`, 0.9 for `file`, 0.7 for `unit`,
  0.5 for `blob`, 0.3 for `search`.

Default weights: `α=0.45, β=0.15, γ=0.25, δ=0.10, ε=0.05`.

### 3.4 Eviction triggers

Eviction is **never** automatic during a turn. It happens only at the
three explicit gates below, in this order:

1. **Turn boundary (post-response).** Working set is recomputed; entries
   not in the new hot tail or anchor set drop to `state=paged` and the
   full body is replaced by a 1-line summary + `tz://` ref.
2. **Budget pressure (during turn).** If a single tool response would
   push total prompt tokens past the configured ceiling, TokenZero
   proposes a compaction (drop adjacency, collapse summaries) before
   admitting the response. The agent loop decides to apply it.
3. **Delta manifest apply.** On receipt of a new TOC delta from a peer
   session, the working set is rebaselined against the merged TOC.

Eviction is **content-aware**: we never split a `unit` mid-entry; if the
budget cannot hold the unit whole, we page the whole unit and rely on
fault-time partial rehydration (see §4).

### 3.5 What evicted means

An evicted entry:

- keeps its TOC row (id, ref, summary, importance),
- loses its in-prompt body,
- still serves line/symbol-level partial reads via the fault path.

There is no hard delete of an evicted entry's bytes. Recovery is always
possible; only the prompt cost is gone.

---

## 4. Fault path

When the model (or tool call) references an evicted paged entry, the
**fault path** materializes it back into the prompt.

### 4.1 Reference forms

The model emits one of:

- `tz://blob/<sha>` — whole blob (rare; explicit only).
- `tz://file/<sha>?lines=120-180` — line range.
- `tz://file/<sha>?symbol=fn:parse_args` — symbol span.
- `tz://unit/<sha>` — pre-chunked unit.
- `tz://search/<sha>?k=5` — top-k hits.

A bounded regex matcher inside `tz_fault` accepts any of these plus
`#L120-L180` suffix form on a `tz://` ref the model has already seen.

### 4.2 Rehydration

```text
model emits:   "see tz://file/faa1…?lines=42-60"
  ↓
tz_fault(ref, range):
  1. resolve ref → blob store (sha-keyed, O(1))
  2. look up session's seen_blobs (delta-encoding view)
  3. compute range delta: lines that have NOT been
     delivered to this session yet
  4. return capsule:
       {
         "ref":  "tz://file/faa1…?lines=42-60",
         "delta_only": true,
         "lines": ["42: ...", ..., "60: ..."],
         "byte_count": 1842,
         "tokens_est": 412,
         "size": "small"
       }
```

Stale-fault protection: if `seen_blobs` says the requested range was
already delivered, the fault returns a no-op capsule with
`size="already-seen"` and a `tz://` ref — never re-ships bytes.

### 4.3 Partial rehydration

- **Line ranges**: truncation point at first blank-line-then-non-indent
  boundary within ±8 lines of the request edge. Reduces accidental
  mid-statement splits.
- **Symbols**: `tz_locate` first resolves the symbol's byte span in the
  blob, then returns that span (no fuzzy fallback; if the symbol has
  drifted, the model gets a `stale=true` flag and a digest diff).
- **Units**: smallest unit containing the requested anchor (function,
  paragraph, section). Units cross ranges; if the requested symbol is
  inside a multi-line statement, the containing unit is returned whole
  and the requested lines are highlighted in the `lines:` field.

Max bytes per fault response: **16 KiB** (≈ 4 000 tok). Larger requests
fall back to a TOC update (`state=resident` for the requested entry)
rather than a one-shot rehydration.

### 4.4 What the fault path MUST NOT do

- It MUST NOT mutate the TOC without an explicit hook from the agent loop.
- It MUST NOT call the model.
- It MUST NOT touch network sockets outside the local blob store
  (fetch-from-URL is a separate `tz_fetch` op, never a fault).
- It MUST NOT silently upgrade a partial rehydration to a full one.

---

## 5. Agent framework contract

The agent loop and TokenZero share a strict boundary. Violating it on
either side breaks the memory model.

### 5.1 What TokenZero owns

- The blob store (`tz_object_store`) and its content addressing.
- The TOC: construction, compaction, gating, delta emission.
- The working set: importance scoring, eviction, residency decisions.
- The fault path: ref resolution, range computation, capsule formatting.
- `seen_blobs` per session (delta encoding).
- Crash-safe checkpoints (`tz://snapshot/<sha>`).

### 5.2 What the agent loop owns

- The model invocation: prompt construction, tool-call parsing, response
  synthesis.
- Decide *when* to call `tz_toc_compact` (the agent loop sees total
  tokens; TokenZero proposes, the loop applies).
- Decide *when* to call `tz_fault` — the model must reference a `tz://`
  ref explicitly; the agent loop never invents ref expansion.
- Final answer construction: the agent loop is the source of truth for
  the user-visible reply text.
- Session lifecycle: start, fork, merge, end, archive.

### 5.3 Boundary primitives

```text
tz://toc/<sha>             // durable TOC snapshot
tz://working-set/<sha>     // signed working-set manifest (epoch, ids, hashes)
tz://fault/<id>            // ad-hoc fault response, content-addressed
tz://snapshot/<sha>        // checkpoint of (TOC + working-set + seen_blobs)
```

The agent loop MUST check the working-set manifest before each model
call. If the manifest's epoch does not match the prompt's epoch, the
loop MUST rebuild the prompt before invoking the model. This is the
single rule that keeps demand paging from going stale.

### 5.4 Failure modes the contract must handle

| Failure | Owner | Recovery |
|---|---|---|
| Prompt built against stale manifest | agent loop | rebuild from working-set manifest |
| Fault returns an unseen-by-session blob | TokenZero | marks entry `seen_after_fault=true`, emits TOC delta |
| TOC compaction loses a needed entry | TokenZero | compact only from adjacency; k-core is never compacted |
| Working set exceeds budget after compaction | agent loop | abort turn, request larger budget or model down-switch |
| Two peer sessions fault same ref concurrently | TokenZero | single blob read; both get delta-encoded slice |

---

## 6. Token budgets per component

| Component | Token budget | Notes |
|---|---|---|
| `kernel` (TOC header + schema + agent profile) | 600 | always resident |
| `hot_tail` (N most-recent entries) | 6 000 | N ≈ 200 at 30 tok/entry |
| `anchor` (top-k importance) | 1 000 | k = 32 at 28 tok/entry |
| `adjacency` (tag/path neighbours) | up to remaining budget | bounded by §3.2 |
| `budget_working_set` total | 8 000 – 32 000 | configured at agent start |
| TOC header in prompt | 80 | session id, hwm, totals |
| TOC delta per turn | ≤ 40 per entry | only new/changed |
| Full TOC dump (cold path) | 2 000 hard cap | older entries compacted |
| Fault response (single) | ≤ 4 000 | 16 KiB byte cap, see §4.3 |
| Checkpoint payload | ≤ 500 | TOC digest + working-set manifest only |

A model with a 32k context window typically runs:

```
kernel 600 + hot_tail 6000 + anchor 1000 + adjacency ~22 000
+ response_reserve 4096 + tool_io_reserve 1024 + margin 512
= 32 712                        // fits 32 768 with 56-token slack
```

---

## 7. Implementation sketch

### 7.1 Rust types (in `crates/tokenzero-context`)

```rust
pub struct TocEntry {
    pub id: EntryId,                 // "E0001"
    pub kind: EntryKind,             // Blob | File | Unit | Search | Snapshot
    pub ref_uri: TzRef,              // tz://blob/<sha> etc.
    pub anchor: Option<Anchor>,      // line 42, symbol, query
    pub byte_weight: u32,
    pub token_weight: u32,
    pub summary: CompactString,
    pub first_touched: u32,
    pub last_touched: u32,
    pub touch_count: u32,
    pub importance: f32,             // [0, 1]
    pub state: EntryState,           // Resident | Paged | Evicted
    pub tags: SmallVec<[Tag; 4]>,
}

pub struct Toc {
    pub session_id: SessionId,
    pub schema: SchemaVersion,
    pub hwm: u64,
    pub entries: Vec<TocEntry>,
}

pub struct WorkingSet {
    pub epoch: u64,
    pub kernel: Vec<TocEntry>,
    pub hot_tail: Vec<TocEntry>,
    pub anchor: Vec<TocEntry>,
    pub adjacency: Vec<TocEntry>,
    pub budget_tokens: u32,
    pub used_tokens: u32,
    pub manifest_hash: Sha256,
}

pub enum FaultRequest {
    Ref(TzRef, Option<Range>),
    LineRange { ref_uri: TzRef, start: u32, end: u32 },
    Symbol { ref_uri: TzRef, name: CompactString },
    TopK { ref_uri: TzRef, k: u32 },
}

pub struct FaultCapsule {
    pub ref_uri: TzRef,
    pub delta_only: bool,
    pub lines: Vec<Line>,
    pub byte_count: u32,
    pub tokens_est: u32,
    pub size: FaultSize,             // Small | Medium | AlreadySeen
    pub stale: bool,
}
```

### 7.2 Core functions

```rust
impl Toc {
    /// Append an entry and bump hwm. Caller supplies byte/token weights.
    pub fn append(&mut self, entry: TocEntry) -> EntryId;

    /// Emit a delta since `since_hwm`. Always ≤ 40 tok/entry returned.
    pub fn delta_since(&self, since_hwm: u64) -> TocDelta;

    /// Compact adjacency beyond k-core into summary-only rows.
    /// k-core is identified via `bv` graph metrics over the touch graph.
    pub fn compact(&mut self, target_tokens: u32) -> CompactionReport;
}

impl WorkingSet {
    /// Recompute residency at a turn boundary. Returns the new manifest.
    pub fn rebaseline(&mut self, toc: &Toc) -> Manifest;

    /// Returns whether `entry` must be paged in to answer `req`.
    pub fn fault_pressure(&self, req: &FaultRequest) -> FaultPressure;

    /// Apply an eviction plan produced by `rebaseline`. Idempotent.
    pub fn apply_eviction(&mut self, plan: EvictionPlan) -> Manifest;
}

pub fn tz_fault(
    store: &BlobStore,
    seen: &mut SeenBlobs,
    req: FaultRequest,
) -> Result<FaultCapsule, FaultError>;
```

### 7.3 Control flow per turn

```text
1. agent loop: build prompt from current working_set manifest
2. agent loop: validate manifest epoch == prompt epoch
3. model: invokes tools (reads, greps, faults)
4. TokenZero:
     a. on tool call: append TOC entry, touch graph edge, bump hwm
     b. on fault: tz_fault → capsule (delta-encoded vs seen_blobs)
     c. on turn end: rebaseline working_set, emit TOC delta
5. agent loop: ingest TOC delta into prompt if delta > 0
6. agent loop: ship response
```

### 7.4 Observability

- Per-turn metrics exported: TOC size, working-set used vs budget, fault
  count, fault bytes, compaction ratio, k-core drift.
- `tz://metrics/<session>` snapshot is itself a `snapshot` entry kind,
  allowing the model to inspect its own memory state in bounded tokens.

---

## 8. Open questions

- Should `adjacency` selection be a synchronous cost, or precomputed as
  a TOC field updated at every `append`? (Recommendation: precomputed;
  saves ~12 ms per turn at p99.)
- How aggressively should `seen_blobs` GC entries that are still
  `Resident`? (Recommendation: never; residency implies future delta
  references.)
- Should `compact` be a hint (advisory) or a hard rule? (Recommendation:
  advisory; the agent loop retains final say on prompt size.)
- Cross-session TOC merging: do we union by `kind` only, or by
  `(ref_uri, anchor)`? (Recommendation: by ref; anchor join happens at
  the unit level.)

---

## 9. References

- `docs/design/session-delta-encoding.md` — wire protocol for shipping
  only unseen bytes; complements partial rehydration in §4.
- `docs/design/manifest-delta-boot.md` — TOC arrival at session start.
- `crates/tokenzero-context` — proposed crate location for the types
  in §7.
- `bv --robot-insights` — graph metrics used by §3.3 importance score.
