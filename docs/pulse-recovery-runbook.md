# Pulse Recovery Runbook

## Symptoms

- `tokenzero pulse doctor` reports `ok: false`.
- `sqlite_integrity` is not `ok`.
- `marker_match` is false.
- `skipped_lines` is non-zero.
- `import-jsonl` refuses a stale, corrupt, or marker-mismatched snapshot.
- Sync, import, or export reports that `.tokenzero/pulse/sync.lock` is held.

## Commands

```bash
tokenzero pulse doctor --json
tokenzero pulse sync --json
tokenzero pulse export-jsonl /tmp/tokenzero-pulse-snapshot.jsonl --json
tokenzero pulse import-jsonl /tmp/tokenzero-pulse-snapshot.jsonl --json
```

## Rebuild SQLite From JSONL

1. Run `tokenzero pulse doctor --json`.
2. If the DB is corrupt, rerun `tokenzero pulse sync --json`.
3. Confirm `sqlite_integrity` is `ok`, `marker_match` is true, and `skipped_lines` is `0`.
4. Keep the JSONL ledger as the recovery source. SQLite is disposable cache.
5. If recovery fails because `sync.lock` is held, JSON mode returns `error_kind=would_block`, `retryable=true`, and the lock path. Wait for the owning process to finish and rerun the command. Do not delete the lock anchor while a process may still hold it.

## Export A Clean Snapshot

1. Run `tokenzero pulse sync --json`.
2. Run `tokenzero pulse export-jsonl <snapshot.jsonl> --json`.
3. Keep `<snapshot>.meta.json` with the snapshot.
4. Verify the exported snapshot with `tokenzero pulse import-jsonl <snapshot.jsonl> --json` in a temporary root before using it for recovery.

## Import A Snapshot

1. Keep the snapshot JSONL and sidecar meta together.
2. Run `tokenzero pulse import-jsonl <snapshot.jsonl> --json`.
3. If import fails with a stale marker, run `tokenzero pulse sync --json` and inspect whether the current ledger has newer events.
4. If import fails with a marker mismatch, regenerate the snapshot sidecar from a trusted export.
5. If import fails on corrupt JSONL, restore the snapshot from Git or another backup and retry.
6. If the current ledger is corrupt, import a trusted marked snapshot whose marker is newer than the current `events.meta.json` marker.

## Safety Rules

- Do not overwrite a different current ledger with an unmarked snapshot.
- Do not import a marked snapshot whose hash/counts differ from its sidecar.
- Do not use a same-age or older snapshot to replace a corrupt current ledger.
- Do not manually delete `sync.lock` unless the owning process is dead and the lock is stale.
- Do not edit `events.sqlite` directly. Delete it only when rebuilding from JSONL.
- Preserve `events.jsonl`, `events.meta.json`, and exported `<snapshot>.meta.json` as a set.
