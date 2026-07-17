# Privacy and Scoping Audit: Cross-Session Memory

**Date**: 2026-07-11
**Scope**: Cross-session content-addressed conversation memory (`tokenzero-3jx`, `tokenzero-fpc`, `tokenzero-rdl`)
**Version**: TokenZero 1.3.0

---

## 1. Architecture Overview

TokenZero session memory persists served-payload records so that content the user has already been served is never re-shipped across sessions. This is implemented via:

- **Session memory store** (`session_persist.rs`): JSON file at `<user_root>/session-memory.json` containing per-scope `ServedRecord` entries keyed by path/query + content SHA-256.
- **Ref index** (`recovery/lib.rs`): Content-addressed blob store shared across cache roots, enabling blob deduplication at the recovery layer.
- **CAS cross-key lookup** (`session.rs`): When a key miss occurs, `SessionMemory::lookup` scans all records for a matching content SHA-256 before returning `Miss`.

---

## 2. Scoping Boundaries

### 2.1 User Scoping

Session memory is **user-scoped**, not project-scoped or store-scoped.

| Component | Scope | Mechanism |
|-----------|-------|-----------|
| Session memory file | Per-user | `HOME/.tokenzero/ref-index/session-memory.json` |
| Ref index (CAS) | Per-user | `HOME/.tokenzero/ref-index/blobs/` |
| Session scope ID | Global per user | Returns `"__user_global__"` for all projects |
| Recovery store CAS | Per-user | `SharedCas::detect_from_cache_path` or `ref_index_root()` |

**Implication**: A blob served in project A is servable by ref in project B for the same user. This is the intended semantics ("pay once") but means that **serve keys (file paths, queries) from project A are visible in the session memory JSON for project B**.

### 2.2 Cross-User Isolation

Each OS user has an independent session memory at `$HOME/.tokenzero/ref-index/`. Users with separate home directories never share session state.

**Multi-user systems**: File permissions (`0o700` on directories, `0o600` on files) enforce that other local users cannot read the session memory store. This is set at write time in `atomic_write_json`.

### 2.3 Cross-Machine Isolation

Session memory does **not** replicate across machines. It is local to the filesystem. No network transport is involved in the session memory or ref index paths.

---

## 3. Environment Variable Surface

| Variable | Effect | Risk |
|----------|--------|------|
| `TOKENZERO_SESSION_SCOPE` | Overrides the session scope ID | If set to a shared value across users, could cause cross-user leakage. Only the owning user can set this (via shell env). |
| `TOKENZERO_REF_INDEX_PATH` | Overrides the ref index root directory | If pointed at a shared location, could cause cross-user blob sharing. Permissions on the target directory control access. |
| `HOME` | Fallback for user root | Standard; no elevated risk. |
| `TOKENZERO_TELEMETRY` | Opts in to local usage-telemetry recording + inspect (`execution_path`, `raw_tokens`, `spent_tokens` only) | Default off; no exporter exists. |

### 3.1 Shareable Usage Telemetry Permission

TokenZero's response ledger, ToolMetrics, and Pulse are local accounting surfaces and remain local by default. The separate shareable usage-telemetry permission is **off by default**. Absence, parse failure, or unknown configuration resolves to disabled.

When explicitly enabled (`TOKENZERO_TELEMETRY=1|on|true|yes`, `--telemetry`, or `EngineConfig.telemetry_enabled = Some(true)`), MCP and CodeMode may append closed records containing only:

- `execution_path` (`mcp` or `codemode`)
- `raw_tokens` (authoritative uncompressed source mass)
- `spent_tokens` (tokens actually presented; must be `<= raw_tokens`)

Records live beside the recovery cache as `usage-telemetry.jsonl`. Response-local protocol metadata (for example shell `command_success`) is not usage analytics and is never exported on this path.

Inspect with:

`tokenzero session-ledger inspect --json [--telemetry | --no-telemetry]`

Permission precedence is: explicit `--no-telemetry`, explicit `--telemetry`, a programmatic `EngineConfig.telemetry_enabled` override, `TOKENZERO_TELEMETRY`, then off. Inspection reports `enabled`, `exporter=none`, and `records`. TokenZero has no telemetry exporter, so opting in performs no upload or network activity.

### 3.2 Environment Variable Scrubbing (h2c)

Per bead `tokenzero-shell-env-leak-h2c`, the runtime shell spawn (`zero.token.shell`) scrubs orchestration-prefixed environment variables from child processes:

- **Scrubbed prefixes**: `TOKENZERO_`, `ZEROSTACK_`, `FSZERO_`, `GRAPHZERO_`
- **Preserved**: `PATH`, user-set `TOKENZERO_*` via explicit env API
- **Re-added internally**: `TOKENZERO_INNER` for engine-internal spawns

This prevents the ref index and session memory paths from leaking into user shell commands, and prevents nested TokenZero instances from picking up the wrong store root.

---

## 4. File Permissions

| Path | Permissions | Set at |
|------|-------------|--------|
| `~/.tokenzero/ref-index/` | `0o700` (owner rwx) | `atomic_write_json` parent creation |
| `~/.tokenzero/ref-index/session-memory.json` | `0o600` (owner rw) | `atomic_write_json` file creation |
| `~/.tokenzero/ref-index/blobs/` | `0o700` | `create_ref_index_dir` |

These are Unix-only (`#[cfg(unix)]`). On non-Unix platforms, filesystem-level access control is the responsibility of the OS.

---

## 5. Content in Memory/Logs

### 5.1 What is stored

- **Session memory JSON**: `ServedRecord` entries containing file paths, query strings, content SHA-256 hashes, blob refs, and file refs. **No raw blob contents**.
- **Ref index blobs**: Content-addressed raw payload bytes in `blobs/sha256/` directory. These are the actual served content.

### 5.2 What is NOT stored

- Raw blob contents in session memory (only hashes and refs)
- User identifying information beyond OS user home directory
- Session memory is never transmitted over the network
- Logs do not contain blob contents (the token-prefix logging in the serve path emits paths/hashes, not payloads)

### 5.3 GC and Deletion

- Session memory records are evicted per-scope at `MAX_SESSION_MEMORY_RECORDS` (2048)
- Ref index blobs follow the recovery store GC policy
- Deleting `~/.tokenzero/ref-index/` removes all cross-session state
- No remote backup or sync of these directories exists

---

## 6. Threat Model

| Threat | Surface | Mitigation |
|--------|---------|------------|
| Local user B reads user A's seen-set | Filesystem permissions | `0o700`/`0o600` on Unix |
| Project A paths leak to project B via session memory | Same-user cross-project scope | Accepted: intended feature; user owns both projects |
| Session memory JSON exfiltrated | File read | Requires local access; scoped to user |
| Ref index blob contents exfiltrated | File read | Requires local access; scoped to user |
| Orchestration env leaks to user shell | Process environment | Prefix scrub at spawn (h2c) |
| `TOKENZERO_SESSION_SCOPE` set to shared value | Environment variable | User-controlled; same user owns the env |

---

## 7. Known Gaps and Future Work

### 7.1 Cross-Project Key Visibility

**Gap**: Session memory keys (file paths, search queries) are visible across all projects for the same user. A user working on a sensitive project may not want those paths visible when working on a different project.

**Mitigation**: The feature is opt-in via the `TOKENZERO_REF_INDEX_PATH` env var. Users who want per-project isolation can set different ref index roots per project. The `TOKENZERO_SESSION_SCOPE` env var can also be used to partition scopes.

**Severity**: Low (same user, same machine, local filesystem).

### 7.2 No Encryption at Rest

**Gap**: Session memory and ref index blobs are stored as plaintext on disk.

**Mitigation**: File permissions restrict access to the owning user. Full-disk encryption (FileVault, LUKS) provides defense in depth.

**Severity**: Low (local filesystem, no network transport).

### 7.3 No Automatic Cleanup

**Gap**: Session memory grows unbounded until eviction at 2048 records. No time-based GC exists.

**Mitigation**: The cap is generous for typical use. Users can delete `~/.tokenzero/ref-index/` to reset.

**Severity**: Low (disk usage is bounded).

---

## 8. Verification

The following tests assert scoping correctness:

- `persisted_memory_is_user_scoped_and_reports_cross_session_savings` (tokenzero-mcp): Verifies user A and user B have independent seen-sets and that user A's cross-session dedup works.
- `ref_index_pay_once_reuses_one_user_cas_object_across_sessions` (tokenzero-recovery): Verifies that identical content stored from two different cache roots resolves to a single CAS blob.

Orchestration env scrubbing (h2c) is verified by the router test asserting `TOKENZERO_`/`ZEROSTACK_`/`FSZERO_`/`GRAPHZERO_` absence in child processes.
