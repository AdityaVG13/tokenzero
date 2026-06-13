# TokenZero — design suggestions

A design review of the `tokenzero` workspace against four principles:

1. **One place.** Anything we do, we do in one place — similar code is not scattered around.
2. **Concentrated decisions.** Complexity and decision-making live in one place, not sprinkled across modules.
3. **No magic numbers.** Tuning knobs are named constants with intent, not literals in the middle of expressions.
4. **Modularity & ownership.** Modules own their state and expose it through methods, not raw `pub` fields.

This document walks through what the codebase is doing today, where it falls short on each principle (with file/line citations), and what to change. The closing section proposes a single first PR that addresses one offender from each principle.

---

## 1. What TokenZero is actually doing

An AI coding agent (Copilot, Claude Code, Cursor) running against your repo calls tools — `read`, `grep`, `view`, shell commands — and every tool's output gets fed back into the model's context as raw text. A `grep` over a sizeable repo can return 30,000 tokens; a `read` of one big file is 8,000. After a few minutes of investigation the agent is dragging tens of thousands of tokens of stale output behind it, and the LLM provider is charging per token, on every turn, forever.

TokenZero sits between the agent and its tools and shrinks what the agent sees while preserving the ability to get the original bytes back exactly. It installs as an MCP server, so when the agent calls `tz_read` instead of the built-in `view`, the response that lands in context is a **compressed render** — maybe the first 40 lines plus a placeholder like `tz://blob/4f8a…` standing in for the rest. If the agent later realises it needs the omitted bytes, it calls `tz_expand 4f8a…` and gets them back byte-for-byte from a local cache. The compression is honest: TokenZero counts the placeholder tokens against itself, so the savings it reports are net of any recovery the agent ends up doing. That honesty is what they call **RACC** — Recovery-Aware Context Compression.

The workspace is eight crates, each owning one layer of that story:

| Crate | Owns |
|---|---|
| `tokenzero-core` | Shared types (`Capsule`, `RefRecord`, `Accounting`, `ToolResponse`, `Visible`), the canonical token counter, the shell-output renderer. |
| `tokenzero-recovery` | The local cache. Stores blobs keyed by `tz://` refs, persists under a file lock, hands them back on `expand`. The keeper of byte-exact recovery. |
| `tokenzero-runtime` | The shell executor. Spawns processes, captures stdout/stderr with size limits, spills oversized output to disk. |
| `tokenzero-filters` | Rewrites known-bad command lines into safer equivalents. |
| `tokenzero-mcp` | The MCP server bridge. Owns `TokenZeroEngine`, the seen-set for deduping repeated reads, the search-backend choice, and a pile of routing decisions. |
| `tokenzero` (bin) | The `clap` CLI wrapping everything above; `main.rs` dispatches ~26 subcommands. |
| `tokenzero-install` | Installation, client (IDE/editor) detection, config-file writes, and an archive-extraction audit. |
| `tokenzero-pulse` | Event-log / telemetry layer for `tokenzero stats` and `tokenzero doctor`. |

The data path on a single `tz_read` call: agent → `mcp` parses MCP request → `engine.read()` consults its seen-set → asks `core` to render → asks `recovery` to store the placeholder bytes for later expand → returns a `ToolResponse` → `mcp` serialises back as MCP JSON → agent gets a short answer instead of the whole file. Every concern in that sentence is owned by exactly one crate, and that part of the design is fine.

The promise of the product is **byte-exact recovery with honest accounting**. Everything below is in service of that promise: the four principles aren't aesthetic, they're the difference between "recovery is guaranteed" and "recovery is guaranteed for the cases the current author remembered."

---

## 2. Principle 1 — one place for any given concern

### Evidence

**(a) Path canonicalisation is forked across crates.** Two near-identical helpers with different names, neither tested as a shared contract:

- `crates/tokenzero-install/src/lib.rs:814` — `fn canonicalize_existing_or_parent`
- `crates/tokenzero-mcp/src/paths.rs:14` — `fn canonicalize_existing_prefix`
- `crates/tokenzero-install/src/inspect.rs:376` — `fn normalize_path_sep`

Each crate independently needed to answer "what is the real absolute path of something the user typed, when the thing might not yet exist?" The two helpers have different names, different signatures, different return types, and no shared test suite. The mcp crate uses its version to enforce `allowed_roots`, which is a **security boundary** — TokenZero is supposed to refuse to read files outside the roots the user authorised.

**(b) Env-var parsing is repeated ten-plus times.** Same five-line shape — `var().ok().and_then(parse).unwrap_or(DEFAULT).clamp(MIN, MAX)` — hand-rolled in:

- `crates/tokenzero-mcp/src/lib.rs:154` `default_shell_timeout`
- `crates/tokenzero-mcp/src/lib.rs:168` `default_mcp_idle_timeout`
- `crates/tokenzero-runtime/src/lib.rs:74` `RunOutputPolicy::default`
- `crates/tokenzero-mcp/src/lib.rs:124,127` `rg_path_override`, `curl_path_override`
- `crates/tokenzero-mcp/src/lib.rs:144` `env_toggle_enabled`
- plus four more.

**(c) Token counting** is single-source (`tokenzero-core/src/tokens.rs:146`). Keep it that way.

### Why this matters

If the two canonicalisation helpers ever drift — say, on how they handle Windows long-paths or symlinks with trailing `..` — the install and mcp crates will disagree about what "the same path" means, and a path that looked safe to one will read as unsafe to the other. Sooner or later the wrong one becomes the basis for an access decision.

For env parsing, "mostly identical" is the problem. When a user reports "I set `TOKENZERO_SHELL_TIMEOUT_SECS=0` and it ignored me," nobody can answer "intentional or bug?" without reading all ten copies. One helper makes the answer come from one place; as a side benefit, you can grep for `env::parsed(` and instantly enumerate every supported environment variable.

### Proposed change

1. New module `tokenzero-core/src/paths.rs` owning **all** path normalisation (`canonicalize_or_parent_prefix`, `normalize_separators`, `is_under_root`). Other crates re-export — no in-crate copies. Drift becomes impossible by construction.
2. New module `tokenzero-core/src/env.rs` with `env_parsed<T: FromStr>(name, default, range)` and `env_toggle(name, default_on)`. All `TOKENZERO_*` reads route through these.

---

## 3. Principle 2 — concentrate complexity and decision-making

### Evidence

The "routing policy" — *when to dedupe, diff-read, expand, spill, fall back to internal scanner* — is the actual intelligence of the product. It decides whether TokenZero saves you 90 % of your tokens or 5 %. Right now it's fragmented across six locations in three crates:

| Decision | Today's home |
|---|---|
| Search backend choice (rg vs internal) | `mcp/lib.rs:74` `SearchBackend::from_env` |
| Search visit limits | `mcp/lib.rs:51-53` `SEARCH_VISIT_MULTIPLIER`, `MIN/MAX_SEARCH_VISITED_FILES` |
| Diff-read eligibility | `mcp/lib.rs:60-61` `DIFF_MAX_BYTES`, `DIFF_MAX_LINES` |
| Session dedup gate | `mcp/lib.rs:134` `session_dedup_default` |
| Spill threshold | `runtime/lib.rs:66-104` `RunOutputPolicy` |
| Cache eviction caps | `recovery/lib.rs:54-65` `RecoveryConfig::default` |

There's no single struct anywhere you can point a new engineer at and say "this is the policy."

### Why this matters

If product says "let's dedupe more aggressively when context is over 80 % full," the engineer making that change has to find, read, and update three files in three crates, and hope they found them all. Worse, when two of those defaults get out of sync (say, the spill threshold ends up larger than the capture threshold), the only place that detects it is `RunOutputPolicy::normalized()`, a function nobody is required to call.

### Proposed change

Introduce a `RoutingPolicy` in a new `tokenzero-policy` crate (or `tokenzero-core::policy`) that owns every routing decision as methods, not fields:

```rust
pub struct RoutingPolicy { /* private */ }

impl RoutingPolicy {
    pub fn from_env() -> Self { /* one place reads env */ }

    pub fn should_dedupe(&self, ctx: &ReadCtx) -> bool;
    pub fn should_diff_read(&self, prev: &Served, next: &Render) -> bool;
    pub fn search_caps(&self, root_size_hint: usize) -> SearchCaps;
    pub fn spill_decision(&self, bytes_seen: usize) -> SpillDecision;
    pub fn eviction_caps(&self) -> EvictionCaps;
}
```

`EngineConfig` / `RecoveryConfig` / `RunOutputPolicy` stop owning those fields — they hold a `&RoutingPolicy`. Tests inject a `RoutingPolicy::for_test(...)` builder. ~60 lines of scattered defaults and ~12 env reads collapse into one auditable type. `docs/routing.md` becomes the spec for one struct, not five.

---

## 4. Principle 3 — no magic numbers

### Evidence (worst offenders)

| Site | Magic literal | Should be |
|---|---|---|
| `recovery/lib.rs:57-62` | `max_blobs: 128, max_files: 256, max_units: 2048, max_search_hits: 1024, max_bytes: 8_000_000, max_load_bytes: 16_000_000` | named consts (or env-driven via §2's helper) |
| `mcp/lib.rs:1682` | `ttl_seconds.unwrap_or(24 * 60 * 60)` | reuse `runtime::DEFAULT_SPILL_TTL` |
| `runtime/lib.rs:707` | `.min(64 * 1024)` initial buffer | `INITIAL_CAPTURE_BUFFER_BYTES` |
| `runtime/lib.rs:710` | `[0u8; 16 * 1024]` read buffer | `STREAM_READ_BUFFER_BYTES` |
| `mcp/lib.rs:117` | `max_visible_tokens: 4000` | `DEFAULT_MAX_VISIBLE_TOKENS` |
| `core/lib.rs:825` | `score += 60` | named relevance weight |
| `recovery/lib.rs:21-24` | `LOCK_RETRIES = 240`, `LOCK_RETRY_DELAY = 25 ms` | derive both from a single `LOCK_BUDGET = Duration::from_secs(6)` |
| `hook/tests.rs:274` | `1024` | reuse `READ_GUARD_DEFAULT_MAX_BYTES` |

### Why this matters

In a perf-oriented codebase, every byte limit, every retry count, every clamp is a tuning knob. When the cache config inlines six numbers, the next reader has no idea whether they're arbitrary or tuned against benchmarks. The project has no single place to look at the cache's memory budget — you have to multiply six numbers in your head to estimate worst case.

The most dangerous current example is `mcp/lib.rs:1682` where `unwrap_or(24 * 60 * 60)` reinvents a TTL that already exists as `DEFAULT_SPILL_TTL` in the runtime crate. They happen to agree today. They will not necessarily agree six months from now, and when they diverge the bug will be *"old spilled files are getting cleaned up faster than the engine expects, so `expand` sometimes returns 'gone'"* — a recovery-correctness bug, the exact category that breaks the product's core promise.

### Proposed change

Every crate gets a `consts.rs` module; **no inline byte/time literals in business code**. Add a lint via `clippy::disallowed_methods` or a custom check in `scripts/` — there's already `scripts/check_module_boundaries.py` as a template.

---

## 5. Principle 4 — ownership through methods, not fields

### Evidence

Almost every public struct in the workspace is a bag of `pub` fields. Concrete leaks:

- **`TokenZeroEngine.config: pub EngineConfig`** (`mcp/lib.rs:206`) — any caller can do `engine.config.allowed_roots.push(...)` mid-session, with zero invariant checks. **`allowed_roots` is the security boundary.**
- **`StoredFile { pub text: String, … }`** (`recovery/lib.rs:68`) — the cache's entire correctness contract is "the bytes you get back are the bytes that were stored." A `pub text: String` permits a caller to mutate a stored payload after the fact, silently breaking the byte-exact guarantee. No compiler error, no test failure.
- **`RecoveryConfig` all-pub** (`recovery/lib.rs:45`) — limits get changed without re-validating that `max_load_bytes >= max_bytes`. `RunOutputPolicy` has the same shape but at least exposes `normalized()`; nothing forces callers to call it.
- **`EngineConfig` all-pub** (`mcp/lib.rs:85`) — including `allowed_roots: Vec<PathBuf>`, `cache_path: PathBuf`, `mode`. Should be `pub fn allowed_roots(&self) -> &[Path]` plus `pub fn add_allowed_root(&mut self, p: PathBuf) -> Result<(), PolicyError>` with canonicalisation + duplicate-check inside.
- **`RuntimePlan`, `RunResult`, `StreamCapture`** (`runtime/lib.rs:39, 108, 57`) — these are result types serialised to JSON; OK to be records, but should derive `#[non_exhaustive]` so adding fields isn't a breaking change.

### Why this matters

`TokenZeroEngine.config` being `pub` means the contract *"`allowed_roots` is the security boundary"* is enforced only by social convention and grep. The first time someone writes a feature that mutates that vec from a request handler, the security model has a hole and you'll never see it in code review unless the reviewer happens to remember that this field is special.

Exactly the same problem exists for `StoredFile.text`. Making it private and exposing only `StoredFile::content(&self) -> &str` is a one-line change that turns a runtime trust assumption into a compile-time guarantee. After that, the only way to put bytes into the cache is through the constructor, and the only way to get them out is read-only.

The `pub(crate)` discipline isn't about hiding things from end users — every consumer here is internal. It's about making the boundaries between crates **legible**. Any field that's `pub` is a tendril that crosses the boundary; replacing tendrils with methods doesn't restrict what callers can do, it makes what they're doing visible at the call site, which is the same thing as making the boundary real.

### The same problem at the file level

Three lib.rs files are bags-of-things:

- `crates/tokenzero-mcp/src/lib.rs` — **2,266 LOC**. Owns the env parsing, routing constants, the search-backend enum, the engine struct, edit-hunk type, serve options, and 30 more responsibilities. The directory already has 10 sibling files carved out (`catalog.rs`, `jsonrpc.rs`, `session.rs`, …); the carving just stopped halfway. Split into `engine.rs`, `config.rs`, `routing.rs`, `search_backend.rs`. lib.rs becomes ~100 LOC of re-exports.
- `crates/tokenzero-recovery/src/lib.rs` — **1,363 LOC** as one file. Currently no module boundary between "in-memory store" and "on-disk cache". Split into `store.rs`, `persist.rs`, `locking.rs`, `eviction.rs`, `tmp_sweep.rs`.
- `crates/tokenzero/src/main.rs` — **1,637 LOC**, 26 `handle_*` functions. main.rs should be a dispatcher (~50 LOC); each `handle_*` should live in `commands/<name>.rs`. Today every new subcommand grows main.rs; with the split, main.rs never changes.

### Proposed change

1. Demote `pub` fields to `pub(crate)` everywhere they cross a security or correctness boundary (`allowed_roots`, `StoredFile::text`, `RecoveryConfig` limits, `cache_path`). Add accessors and constructors that validate invariants.
2. Mark all serialised result records with `#[non_exhaustive]`.
3. Finish the three lib.rs splits described above.

---

## 6. Why these four principles, here

TokenZero is a **trust product**. Agents only use it because they trust that compressed output is recoverable, that `allowed_roots` actually means allowed-roots, and that the token accounting is honest. Every duplicated helper, every scattered decision, every magic literal, every `pub` field on a security- or correctness-critical type is a place where that trust depends on the author having remembered to do the right thing — instead of on the structure of the code making the wrong thing impossible.

Applied here, these four principles convert *"we did the right thing this time"* into *"the code wouldn't compile if we hadn't."* That's the version of the product that survives contact with the next ten contributors.

---

## 7. Recommended first PR

If you want to land this incrementally, the highest-leverage single change is:

> **Extract `RoutingPolicy` and a `consts/` module per crate, and make `TokenZeroEngine.config` private with accessors.**

That single PR knocks out one offender from each of the four principles and unblocks the bigger lib.rs splits without changing public crate APIs (because `TokenZeroEngine`'s methods stay the same — only field access goes away).

Suggested follow-up sequence after that:

1. Path-canonicalisation consolidation into `tokenzero-core::paths` (Principle 1).
2. `env::parsed` helper + audit every `TOKENZERO_*` read (Principle 1).
3. Split `tokenzero-mcp/src/lib.rs` (Principle 4 at file level).
4. Split `tokenzero/src/main.rs` into `commands/` (Principle 4 at file level).
5. Privacy pass on `StoredFile`, `RecoveryConfig`, `EngineConfig` (Principle 4).
6. Split `tokenzero-recovery/src/lib.rs` (Principle 4 at file level).
