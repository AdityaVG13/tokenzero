# Pulse Sync Strategy

## Source Of Truth

- Primary: JSONL.
- Rationale: Pulse events are append-only telemetry for human inspection, Git backup, and recovery. SQLite is a rebuildable query cache for fast reports, doctor checks, and exports.

## Sync Triggers

- On command: `tokenzero pulse sync`, `tokenzero pulse doctor`, `tokenzero pulse export-jsonl <output>`, and `tokenzero pulse import-jsonl <input>`.
- On normal report: `tokenzero pulse` attempts a best-effort sync before rendering the JSONL report.
- On event write: `record_event` appends one complete JSONL line under the same Pulse lock, calls `sync_data`, and fsyncs the parent directory when it creates the ledger. SQLite catches up on the next sync/report command.
- Timer/throttle: not currently used. Short JSONL to SQLite lag is expected.

## Versioning

- DB marker: SQLite `meta` table stores `schema_version`, `source_of_truth`, `ledger_sha256`, `event_count`, `skipped_lines`, and `updated_unix`.
- JSONL marker: `events.meta.json` stores the same marker for the live ledger.
- Snapshot marker: `export-jsonl` writes `<snapshot>.meta.json`.
- Import rule: marked imports must match their JSONL hash/counts and must be newer than the current marker when hashes differ.

## Concurrency

- Lock file path: sibling `sync.lock` next to the Pulse ledger, for example `.tokenzero/pulse/sync.lock`.
- Busy timeout: SQLite connections use a 5 second busy timeout.
- Sync lock timeout: sync, import, and export wait up to 5 seconds for the Pulse lock.
- Event lock timeout: event appends wait up to 30 seconds for the Pulse lock, then fail open at the caller boundary for normal TokenZero tool responses.
- Ownership: lock files carry a token so an old guard cannot remove a lock reclaimed by a newer process.

## Storage Policy

- SQLite uses WAL mode, `synchronous=NORMAL`, `fullfsync=ON`, `wal_autocheckpoint=1000`, and `foreign_keys=ON`.
- Multi-step SQLite rebuilds run in one transaction.
- Hot indexes cover `tool + timestamp` and `event + timestamp`; `doctor` verifies index usage with `EXPLAIN QUERY PLAN`.
- Append-only JSONL event writes fsync the file before returning. This favors durability over maximum write throughput for Pulse telemetry.
- Full JSONL exports and sidecar writes use temp files, fsync, atomic persist, and parent directory fsync where supported.

## Failure Handling

- DB locked: respect the busy timeout and return a non-zero error if SQLite cannot proceed.
- Sync lock held: retry briefly, then return a clear lock-held error with the `sync.lock` path. In `--json` mode the command prints `tokenzero.pulse.error.v1` with `error_kind=would_block`, `retryable=true`, and exits non-zero.
- JSONL parse error: sync skips corrupt lines and reports `skipped_lines`; imports reject corrupt input before replacing the ledger.
- Stale import: reject missing, stale, mismatched, or ambiguous markers when they would overwrite a different current ledger.
- Unsynced current ledger: reject importing a different snapshot until `tokenzero pulse sync` refreshes markers.
- Corrupt current ledger: allow a trusted marked snapshot to replace it only when the snapshot marker is newer than the current marker.
- DB corruption: remove the SQLite cache files and rebuild from JSONL.
