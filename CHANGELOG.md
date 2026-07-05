# Changelog

All notable changes to TokenZero will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
