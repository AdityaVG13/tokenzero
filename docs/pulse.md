# TokenZero Pulse

TokenZero Pulse is the recovery-aware observability layer for TokenZero tool calls. It answers the question that a plain savings counter cannot answer: did compression actually help after exact recovery, cache behavior, failed writes, negative-savings events, and latency are counted?

The shipped surface is the Recovery Flight Recorder plus a bounded local report:

- Recovery Flight Recorder: append-only JSONL event ledger for TokenZero tool calls.
- SQLite query cache: rebuilt from JSONL by `tokenzero pulse sync`; never the source of truth.
- Local report: `tokenzero pulse stats` (alias `status`) prints the aggregate summary.

## Commands

The actual clap surface (verified against `tokenzero pulse --help`):

```bash
tokenzero pulse                    # same as `pulse stats`, human-readable report
tokenzero pulse stats [--json]     # aggregate report (alias: status)
tokenzero pulse sync [--json]      # reconcile JSONL ledger into the SQLite cache
tokenzero pulse doctor [--json]    # check markers, PRAGMA integrity_check, hot index
tokenzero pulse export-jsonl <OUTPUT>   # atomic JSONL snapshot from the reconciled cache
tokenzero pulse import-jsonl <INPUT>    # validate snapshot, replace ledger, rebuild cache
```

All subcommands accept the global `--root <ROOT>` (override the ledger root) and `--json` (machine-readable envelope).

### Not in the shipped binary

The following were drafted for Pulse but are not implemented in this CLI; listing them here as design intent, not as runnable commands. They are excluded because the bounded stats envelope above is sufficient for agent self-inspection without becoming its own context bomb, and the richer views were never wired to a clap surface: `--today`, `--session`, `--detail` modes, `--live`, `--tui`, `replay`, `forecast`, `graph`, `compact`, `clear`, `import-stats`, `dashboard`, `perf-budget`, and `pulse expand pulse://...` recovery refs. Do not script against them; this section is the contract that they do not exist yet.

Where hooks or slash commands are supported, `/tz pulse` and `/tz pulse session` can be mapped to the same CLI calls.

## Event Storage

Global events default to:

```text
~/.tokenzero/pulse/events.jsonl
```

Project-local mirror events can be enabled at:

```text
.tokenzero/pulse/events.jsonl
```

The default event schema records token counts, refs, latency, cache hits, recovery cost, and health flags. It does not record raw code, raw shell output, secrets, or daemon artifacts.

### Local Pulse versus shareable telemetry

Pulse is local observability; it is not the default-off shareable usage-telemetry permission. Existing Pulse JSONL/SQLite, ToolMetrics, and response-ledger accounting continue locally regardless of `TOKENZERO_TELEMETRY`. Shareable usage telemetry is off by default and, when explicitly enabled, records only `{execution_path, raw_tokens, spent_tokens}` for MCP and CodeMode into `usage-telemetry.jsonl`. Inspect with `tokenzero session-ledger inspect --json`; `--telemetry` opts in, `--no-telemetry` opts out with precedence, and `TOKENZERO_TELEMETRY` accepts only `1/on/true/yes` case-insensitively. Inspection always reports `exporter=none`: no exporter or upload path exists.

`TOKENZERO_PULSE_DISABLED` is not read by the current Pulse implementation and therefore does **not** disable local Pulse recording. Do not rely on that variable; control whether a caller records Pulse events at that caller's integration surface. This documents the previously implicit name/behavior mismatch rather than implying an unsupported global kill switch.

Pulse uses JSONL as the source of truth. SQLite is a locked, rebuildable query cache at `.tokenzero/pulse/events.sqlite` or `~/.tokenzero/pulse/events.sqlite`. Reconciliation is one-way from JSONL into SQLite and guarded by `.tokenzero/pulse/sync.lock`. Sync, import, and export commands wait briefly for transient lock contention before returning a clear lock-held error. Event appends wait longer, call `sync_data` before returning, and still fail open for normal TokenZero tool responses. Full snapshot exports use temp files, fsync, and atomic persist.

When Pulse sync/import/export/doctor commands are run with `--json`, failures return a machine-readable error body before exiting non-zero. Lock contention uses `schema_version=tokenzero.pulse.error.v1`, `ok=false`, `error_kind=would_block`, `retryable=true`, and an `error` string containing the held lock path.

Version markers are written to both stores: SQLite table `meta` and the sidecar `events.meta.json` contain the source marker, ledger hash, valid event count, skipped-line count, and update time. Snapshot exports also write `<snapshot>.meta.json`; imports refuse marker mismatches, same-second ambiguous snapshots, unmarked overwrites of a different current ledger, and snapshots that would discard unsynced current ledger changes. `tokenzero pulse doctor` compares those markers, runs `PRAGMA integrity_check`, and verifies the hot `tool + timestamp` index through `EXPLAIN QUERY PLAN`.

Use `tokenzero pulse export-jsonl <output>` to write an atomic JSONL snapshot from the reconciled SQLite cache. Use `tokenzero pulse import-jsonl <input>` to validate a snapshot, atomically replace the ledger, and rebuild SQLite. Imports with corrupt JSONL lines fail before replacing the current ledger. A trusted marked snapshot can recover a corrupt current ledger only when its marker is newer than the current ledger marker.

Operational details live in [pulse-sync-strategy.md](pulse-sync-strategy.md) and [pulse-recovery-runbook.md](pulse-recovery-runbook.md).

Fast-path fields record why TokenZero skipped compression: `output_strategy`, `skip_reason`, `roi_guard_applied`, `raw_passthrough`, `near_raw`, `empty_result`, `tiny_output_passthrough`, `guarded_expansion`, `forced_expansion`, and `compression_would_increase_tokens`. `cache_hit` is separate telemetry. It is never treated as a display strategy.

Batch read/find/tree calls record one parent event with `batch=true`, `item_count`, batch overhead metrics, and capped item rollups. Pulse does not store full item displays, raw file contents, shell output, or debug JSON for batch calls.

## Accounting

Pulse never collapses these into one headline:

- visible-context savings: raw tool tokens minus model-readable capsule tokens.
- recovery-adjusted savings: raw tool tokens minus visible capsule tokens minus recovery expansion tokens.
- exact-cache byte-lossless savings: hidden exact payload tokens kept server-side, not model-readable.
- cache savings: repeated-output/cache wins, separate from first-response compression.
- output/reply savings: optional response budgeting, separate from tool context.
- schema/shell-routing savings: separate module categories when enabled.

Hidden exact refs are useful because they guarantee local recovery. They are not counted as readable context until `tz_expand` returns visible text, and those recovery tokens are charged.

`tokenzero pulse stats --json` returns a bounded aggregate summary. It contains no raw event rows, raw command output, payloads, or debug-only fields, so Pulse cannot become its own context bomb. The JSON shape (schema `tokenzero.pulse.v1`):

```json
{
  "schema_version": "tokenzero.pulse.v1",
  "status": "ok",
  "event_count": 34810,
  "raw_tokens": 836693768,
  "visible_tokens": 670084004,
  "recovery_tokens": 109956688,
  "task_lossless_tokens": 779957692,
  "failures": 606,
  "cache_hits": 0,
  "exact_ref_count": 184009,
  "visible_savings": 0.199,
  "recovery_adjusted_savings": 0.0677,
  "skipped_lines": 0
}
```

`visible_savings` and `recovery_adjusted_savings` are fractions (0.199 = 19.9%). Recovery tokens for `tz_expand` re-expansion are charged into the ledger, so inspecting TokenZero output is counted in recovery-adjusted accounting.

There are no `--detail`/`--max-items`/`--max-events` flags and no `pulse://` recovery refs in the shipped binary; the aggregate above is the entire report. Deeper inspection is done by reading the JSONL ledger or querying the SQLite cache directly.

## Stateless ROI Guard

TokenZero deliberately returns raw or near-raw output when compression would cost more than it saves. This is a hot-path decision and does not require a daemon, watcher, or background index.

Examples:

- tiny shell results such as `echo ok`, `pwd`, `mktemp -d`, and compact `git status --short --branch` use `raw_passthrough`.
- zero-hit searches render a one-line `# <tool> <query> — 0 matches` note (clamped query echo) with refs intact.
- short search hits and tiny files use `near_raw_with_ref`.
- broad expansions use `guarded_expansion` unless force is explicit.
- shallow tree passthrough and rewrite-control events are neutral in Pulse when the only cost is bounded TokenZero routing overhead.

Pulse records these as positive behavior, not failures. The purpose is to avoid inflated visible context while preserving exact refs and honest recovery-adjusted accounting.

Normal MCP/CLI display is also part of the guard: tiny outputs are shown as compact text with lowercase `tz_*` labels, while full metadata remains available only in JSON/debug or explicit structured paths. Pulse records `display_tokens`, `model_visible_tokens`, and `debug_tokens`; `visible_tokens` tracks the model-visible display, not the debug JSON envelope, structured tree rows, or hidden exact payload. Raw payloads are still not logged.

## Configuration surface

There is no `pulse` config-file block in the shipped binary, and `TOKENZERO_PULSE_DISABLED` is not read by the implementation; it does not disable local Pulse recording. There is also no `clear`, `compact`, or `import-stats` subcommand: retention and compaction are not yet automated, so prune the JSONL ledger by hand if it grows beyond what you want to keep. This documents the current behavior exactly; do not rely on variables or commands listed here as absent.

## Fail-Open Behavior

Pulse recording is best-effort. If the event ledger is locked, corrupt, missing, oversized, or unwritable, TokenZero tools still return their normal compressed response. `tokenzero pulse doctor` reports store integrity, marker agreement between JSONL and SQLite, and the hot `tool + timestamp` index plan.
