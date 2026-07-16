# Changelog

All notable changes to TokenZero will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.0] -- 2026-07-15

### Added
- **Embeddable recovery API**: `TokenZeroStore` exposes the shared recovery store as a reusable Rust handle with byte-exact put, get, expand, pin, and lifecycle contracts.
- **Conservative shared-CAS maintenance**: mark-and-sweep GC, orphan repair, durable pin metadata, and cross-engine reachability preserve live ZeroRef payloads.
- **Reproducible performance evidence**: automated northstar rebaselining, release-binary provenance, source-state fingerprints, find backend crossover measurements, and retained history make benchmark claims auditable.

### Changed
- **Lower MCP latency**: retained before/after evidence measures p50 reductions of 53.7% for read, 58.1% for find, and 57.1% for expand while preserving advisory locking and JSONL append semantics.
- **Smaller implementation**: behavior-preserving consolidation and generated corpus materialization reduce code across `crates`, `benches`, `benchmarks`, and `scripts` from 110,723 to 59,072 lines (46.6%).
- **Search routing**: deterministic crossover evidence retains the internal scanner for small trees and `rg` for larger directory searches.
- **Benchmark integrity**: northstar runs now use one release binary for every component, fail closed on stale reuse, and record binary SHA-256 and source provenance.

### Fixed
- Recovery publication, garbage collection, concurrent writer synchronization, stale portable-reference hashes, malformed repeated fragments, and orphan segment cleanup now fail safely without exposing corrupted bytes or deleting live data.
- Windows CodeMode journal persistence preserves I/O errors and durable replacement semantics.
- MCP working-set admission, capability descriptor revisioning, async plan parsing, release telemetry audits, package-audit fixtures, and deleted regression coverage were restored and hardened.
- Release verification is portable across Windows command-length limits, path separators, platform-specific warnings, and recovery-cache resolution.
- Parallel MCP tests isolate ref-index overrides and content fixtures so short-lived test stores cannot interfere with one another.
- Workspace package manifests declare the crates.io version for the pinned `fastmcp-rust` dependency.

## [1.3.0] -- 2026-07-12

### Added
- **ZeroRef v1 contract**: portable blob refs of the form
  `(tz|fz|gz)://blob/<sha256>[#fragment]` with full-hash identity, digest
  verification before fragment selection, and a stable error taxonomy
  (`malformed`, `missing`, `corruption`, `unsupported`, …). Spec and golden
  vectors live under `docs/zeroref-v1-contract.md`.
- **Shared-CAS adapter**: canonical content-addressed storage for ZeroRef v1
  blobs, with reachability/pin schema v1 frozen so GC and multi-engine
  expand share one truth.
- **Cross-engine blob expand**: `fz://` and `gz://` **blob** refs minted by
  fszero or graphzero expand via the shared CAS and, on miss, sibling engine
  stores under the same unified `.zerostack` root. Non-blob portable refs are
  still unsupported. Release evidence is the retained merged CI artifact; the
  checked-in fixture is only a reproducible host snapshot.
- **Strict fragment algebra**: typed `#Bstart-end` (byte, half-open) and
  `#Lstart-end` (line, inclusive) selectors with structured OOB errors.
- **Capsule-default expand**: expand returns preview + ref by default instead
  of shipping the full body into the model context.
- **Session-delta protocol**: watermark, tombstones, and byte telemetry so
  multi-turn sessions only ship what changed (`tokenzero.ledger.v1` curves
  and observatory evidence included).
- **Queryable session ledger** (`tokenzero.ledger.v1`): fail-open JSONL cost
  stream with visible/raw/prevented token mass, rotation, and CLI queries for
  repo/window cost, version delta, and per-agent spend. Pulse CLI aggregates
  per-session cost.
- **Sub-100-token manifest+delta session boot**: TZ/1 sidecars, demand-paged
  session memory, session-boot MCP resource and `tokenzero session-open` CLI
  (measured ~21 tokens on large corpora).
- **Loss-free working-set span eviction**: LRU-bounded resident set with
  TZ-EVICT markers, demand-paged rehydration, and byte-exact expand
  round-trips.
- **BlobEntry Inline/FileRef storage**: large reads store path+fingerprint
  pointers instead of duplicating payloads; FileRef verifies content on
  rehydrate.
- **Pay-once user ref index + cross-session memory**: same content across
  cache roots resolves to one user CAS object; privacy/scoping audit
  documents isolation boundaries.
- **Crash-safe plan-scoped mutation journaling** with bounded journal segment
  rotation (sealed generations + snapshot compaction).
- **CodeMode heavy-execution containment**: machine-wide permit, bounded
  queue, identical-plan dedup, and tracked lifecycle for background shell
  jobs.
- **Bounded session recipe registry**: `zero.register` / `zero.run` /
  `zero.list` for named parameterized plans with size and mutation gates.
- **Per-model tokenizer registry** and boundary-aware packing (provider-
  qualified model ids; residual budget packed to token boundaries).
- **Telemetry**: granular envelope token attribution, prevented-read bytes,
  prefix-cache hit rate, expand accounting contracts.
- **Legacy ref migration**: command + complete lifecycle for pre-v1 refs.
- **Portable engine binary discovery** and PR18 capability descriptor as sole
  tool/ZeroRef policy owner.
- **Bench harnesses**: ZeroRef 3x3 binary/store conformance matrix, ledger
  regression gate, byte-stable prefix suite, delta-encoding evidence,
  expand latency by size class, competitor bake-off / 1M-line navigation
  frameworks, elision predictor evaluation.

### Changed
- **MCP policy ownership**: single owner for CodeMode list/call; tools/call
  unified behind `gate_tools_call`; Classic surface gate restored.
- **Store resolution**: single workspace store resolver for CLI and MCP;
  recovery cache isolated per call root; store-root precedence tests frozen.
- **Expand surface**: `parse_ref` accepts only `tz://` after canonicalize for
  the portable path; sibling-engine fallback handles `fz://`/`gz://`.
- **Read/search path**: source-backed admission for large files cuts peak RSS;
  chunked bounded-memory expand reads; Pulse lock metadata skips redundant
  durability barriers; auto literal search on direct files runs in-process.
  Retained before/after evidence measures MCP p50 reductions of 53.7% for read,
  58.1% for find, and 57.1% for expand.
- **Program footprint**: behavior-preserving consolidation and generated corpus
  materialization reduce code across `crates`, `benches`, `benchmarks`, and
  `scripts` from 110,723 to 59,072 lines (46.6%).
- **Binary resolution**: typed `BinaryResolution` Result; require executable
  bit for env/PATH and well-known binaries.
- **Write recovery ladder** on CodeMode edit failure with QuickJS deny ladder
  parity.

### Fixed
- Cross-engine `fz://`/`gz://` expand no longer fails with ref-not-found when
  the blob lives only in a sibling engine store under the unified root.
- Evict livelock: victim selection no longer pins CAS-reachable refs forever
  after `drop_ref`.
- Relative CLI search paths resolve against the call root, not cwd.
- Allowlist escape for MCP-supplied roots; expand health signal on
  zeroref-malformed; search backend-parity keys.
- Shell mutation classification by command position (data is not intent);
  orchestration env scrubbed from user command children.
- SurfaceHealth shared across plan engines; crash-only expand unlocked when
  surface unhealthy; default recovery cache shared with expand.
- Journal lowering scope, exact explicit expand, routed execution roots.
- Observatory ref regex so ledger replay emits full expand accounting.
- Tokenizer metadata matches provider-qualified model ids
  (`openai/gpt-4o…`).
- `tz_report_tool_issue` menu cluster restored to the seven-entry jsonrpc
  contract; accepts `zero_execute`.

### Security / privacy
- User-scoped session memory and ref index with 0700/0600 permissions;
  cross-user isolation via home directory; documented threat model and
  known gaps in `docs/privacy-and-scoping.md`.
- h2c-style orchestration env scrub on user-command spawns.

## [1.2.0] -- 2026-07-05

### Added
- **FastMCP dual-mode transport**: CodeMode plans are delivered through both
  streamable-HTTP and stateless JSON-RPC FastMCP modes. The v2 ref-first
  envelope is the default for all `tz_execute_code` paths.
- **Envelope v2**: structured two-part wire protocol (primary text + compact
  JSON payload) with per-op ref tracking, telemetry scoring, and payload
  envelope token attribution.
- **CodeMode composition benchmark**: seven reproducible workloads measuring
  plan-based CodeMode execution against equivalent raw subprocess output and
  classic per-op MCP tool calls. Artifact committed as
  `demo/composition_benchmark.json`.
- **Per-user ref index**: a cross-cache-root SQLite index that maps every
  `tz://blob/*` ref to its owning cache path, making refs durable across
  engine restarts and cache directory moves.
- **Expand exactness guarantee**: `zero.token.expand` always returns byte-exact
  original content, verified via SHA-256 stored at compact time and checked on
  every expand.
- **Shell inline economics**: shell output is now policy-scored for compact
  rendering, with token savings reported per call in the `visible_tokens` and
  `raw_tokens` telemetry fields.
- **Corrective-hint errors**: the engine's non-ref expand error now suggests
  the correct API (`zero.fs.compound('read',{path})`) instead of just
  rejecting the input. Invalid refs on compact/expand include the malformed
  value for quick diagnosis.
- **README command audit**: every documented command in the README is verified
  by a CI gate (`make readme-command-audit`) that runs each command against
  the installed binary and checks for non-zero exit or unexpected output.

### Changed
- **Store resolution hygiene**: relative `ZEROSTACK_STORE_ROOT` env values are
  now resolved against the passed `repo_root`, never `current_dir()`,
  eliminating cwd contamination.
- **Object compact fidelity**: `zero.token.compact` of a non-string value
  JSON-serializes it with stable key ordering before storage; the expanded
  result is the exact JSON text (or parsed object in plan context).
- **Statement parser**: literal `return <scalar>;` expressions (`12345`,
  `"x"`, `true`) now fold correctly in lowered plans, no longer treated as
  variable references.
- **Benchmark determinism**: scale workloads use deterministic synthetic
  payloads instead of live git state. Two consecutive benchmark runs produce
  identical JSON except wall-time fields.

### Fixed
- `zerostack_store` tests no longer observe cwd-level `.zerostack` directories
  when resolving paths for a tempdir root.
- `zero.token.compact(someObject)` no longer stores `"[object Object]"`;
  objects are JSON-serialized before compression.
- `return 12345;` in a statement-plan no longer errors with "undefined
  variable: 12345".
- Non-ref expand errors now include a corrective hint pointing to
  `zero.fs.compound('read',{path})`.

## [1.0.x] -- earlier releases

- TokenZero 1.0.0 through 1.0.2: initial public release with CLI tools, MCP
  transport, QuickJS sandbox, and content-aware compression.
