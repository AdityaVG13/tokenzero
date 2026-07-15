# TokenZero Pulse

TokenZero Pulse is the recovery-aware observability layer for TokenZero tool calls. It answers the question that a plain savings counter cannot answer: did compression actually help after exact recovery, cache behavior, failed writes, negative-savings events, and latency are counted?

Pulse has four parts:

- Recovery Flight Recorder: append-only event ledger for TokenZero tool calls.
- Pulseboard: terminal dashboard from `tokenzero pulse --tui`, `tokenzero pulse --live`, or `tokenzero dashboard`.
- Pulse Forecast: deterministic Monte Carlo projection from recent events.
- Pulse graphs: local SVG/Markdown/JSON artifacts under `results/current/`.

## Commands

```bash
tokenzero pulse
tokenzero pulse --session
tokenzero pulse --today
tokenzero pulse --json
tokenzero pulse --today --json --detail summary
tokenzero pulse --today --json --detail drains --max-items 20
tokenzero pulse --today --json --detail tools --max-items 20
tokenzero pulse --today --json --detail sessions --max-items 20
tokenzero pulse --today --json --detail timeline --max-events 100
tokenzero pulse --today --json --detail raw-events --max-events 100
tokenzero pulse expand pulse://event/<event_id>
tokenzero pulse --live
tokenzero pulse --tui
tokenzero pulse replay
tokenzero pulse replay --session
tokenzero pulse replay --today --json
tokenzero pulse forecast --seed 13 --samples 500 --horizon session
tokenzero pulse graph
tokenzero pulse export
tokenzero pulse export-jsonl /tmp/tokenzero-pulse.jsonl
tokenzero pulse import-jsonl /tmp/tokenzero-pulse.jsonl
tokenzero pulse sync
tokenzero pulse doctor
tokenzero pulse compact --dry-run
tokenzero pulse compact --apply
tokenzero pulse clear --older-than 30d
tokenzero pulse doctor
tokenzero pulse import-stats --dry-run
tokenzero dashboard
tokenzero graph
tokenzero expand pulse://event/<event_id>
```

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

Pulse is local observability; it is not the default-off shareable telemetry permission. Existing Pulse JSONL/SQLite, ToolMetrics, and response-ledger accounting continue locally regardless of `TOKENZERO_TELEMETRY`. The shareable dry-run payload contains only `schema=tokenzero.telemetry.v1`, current crate `version`, `raw_tokens`, and `saved_tokens`. Inspect it with `tokenzero session-ledger inspect --json`; `--telemetry` opts in, `--no-telemetry` opts out with precedence, and `TOKENZERO_TELEMETRY` accepts only `1/on/true/yes` case-insensitively. It is off otherwise. Inspection always reports `exporter=none`: no exporter or upload path exists, and enabling it sends nothing.

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

`tokenzero pulse --json` returns a bounded summary by default. It intentionally omits raw event rows, timeline walls, raw command output, payloads, and debug-only fields so Pulse cannot become its own context bomb. The default JSON shape is decision-focused:

```json
{
  "ok": true,
  "summary": {},
  "rates": {},
  "top_wins": [],
  "top_drains": [],
  "tool_rollup": {},
  "strategy_rollup": {},
  "cache_health": {},
  "exact_ref_health": {},
  "next_action": {},
  "recovery_refs": [],
  "truncated": true,
  "omitted_events": 0
}
```

Use explicit detail modes for bounded deeper views:

- `--detail summary`: default compact decision view.
- `--detail drains --max-items N`: top token drains and recovery drains.
- `--detail tools --max-items N`: bounded per-tool rollup.
- `--detail sessions --max-items N`: bounded recent session rollup.
- `--detail timeline --max-events N`: compact event timeline.
- `--detail raw-events --max-events N`: explicit sanitized event rows.

`--events` remains as a legacy alias for `--detail raw-events`, but new scripts should prefer the detail flag. `tokenzero pulse compact --dry-run` reports compaction candidates; `--apply` rewrites sanitized JSONL segments and a summary sidecar without adding raw payloads.

Normal Pulse JSON enforces `--max-json-bytes`, `--max-items`, `--max-events`, and recovery-ref caps before printing. Over-budget output is trimmed from bounded rows first and includes `truncated`, `omitted_events`, and a `next_detail_command`.

Pulse summaries include `pulse://` recovery refs. Recover exact sanitized event rows with:

```bash
tokenzero pulse expand pulse://event/<event_id>
tokenzero pulse expand pulse://range/<first_event_id>..<last_event_id>
tokenzero expand pulse://event/<event_id>
```

Pulse recovery expansions are recorded as recovery-cost events, so inspecting Pulse itself is counted in recovery-adjusted accounting.

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

## Pulse Forecast

`tokenzero pulse forecast` uses deterministic Monte Carlo sampling from recent Pulse events. A fixed seed produces stable results.

Forecast output includes:

- expected visible tokens saved.
- expected recovery-adjusted tokens saved.
- p10/p50/p90 bands.
- probability that savings go negative.
- probability recovery cost exceeds the threshold.
- projected cache hit rate.
- projected latency p95.
- likely token wins and drains.
- confidence and low-sample warnings.

Forecasts are projections, not guarantees.

## Pulseboard

Pulseboard sections:

- Real Score: visible, recovery-adjusted, recovery drag, negative events.
- Integrity: exact refs emitted/expanded, exact-ref success, hidden-token accounting, anchor risk.
- Cache: cache hit rate and exact-cache byte-lossless savings.
- Forecast: p10/p50/p90 trajectory, negative-savings risk, next best lever.
- Flow: tool mix, per-tool contribution, top wins/drains, latency p95.
- Health: fail-open events, parser errors, corruption skipped, storage/retention status.
- ROI Guard: raw passthroughs, near-raw responses, empty results, tiny-output passthroughs, guarded expansions, and compression-increase skips.

Normal mode shows summaries only. Debug mode may show sanitized event summaries.

`tokenzero pulse replay` renders the session/day as a readable timeline instead of a JSON wall. Each row shows the tool, visible tokens saved, recovery-adjusted tokens saved, recovery cost, exact-ref status, cache hit, latency, negative-savings moments, and turning points where recovery erased the visible benefit. JSON mode returns the same stable fields under `timeline`, `top_wins`, `top_drains`, `turning_points`, and `suggested_next_lever`.

## Graphs

`tokenzero pulse graph` generates:

- `results/current/pulse_dashboard.md`
- `results/current/pulse_metrics.json`
- `results/current/pulse_graphs/*.svg`

Current SVGs include recovery-adjusted trend, visible vs recovery-adjusted trend, recovery drag waterfall, cache trend, exact-ref expansion trend, negative-savings timeline, latency trend, per-tool contribution, top drains Pareto, hour/tool heatmap, forecast fan chart, recovery-cost histogram, savings density, integrity gauge, cache reuse flow, and tool mix.

`tokenzero pulse graph --json` and `tokenzero graph --json` print only artifact paths and graph names. The larger metrics payload is written to `pulse_metrics.json` instead of being dumped into the model-visible reply.

Foundation budget artifacts are generated by:
```bash
tokenzero perf-budget --json
```
This writes `results/current/perf_budget.json` and `results/current/perf_budget.md`. The checks are small release guards for event-write overhead, aggregation, replay rendering, graph generation, command rewriting, bounded tree walking, and MCP startup. They are not public performance claims.

## Config

Pulse defaults are safe:

```json
{
  "pulse": {
    "enabled": true,
    "global_event_path": "~/.tokenzero/pulse/events.jsonl",
    "project_event_path": ".tokenzero/pulse/events.jsonl",
    "record_raw_payloads": false,
    "max_event_bytes": 8192,
    "max_events_per_day": 100000,
    "max_storage_mb": 25,
    "retention_days": 30,
    "redact_secrets": true,
    "post_response_summary": true,
    "dashboard_artifact_dir": "results/current",
    "session_id_source": "env",
    "forecast": {
      "enabled": true,
      "samples": 500,
      "seed": 13,
      "default_horizon": "session"
    }
  }
}
```

Disable event writing:

```bash
TOKENZERO_PULSE_DISABLED=1 tokenzero read tokenzero/cli.py
```

Delete old local history:

```bash
tokenzero pulse clear --older-than 30d
```

## Fail-Open Behavior

Pulse recording is best-effort. If the event ledger is locked, corrupt, missing, oversized, or unwritable, TokenZero tools still return their normal compressed response. `tokenzero pulse doctor` reports skipped corrupt rows and storage status.

## Legacy Stats

If `.tokenzero/stats.jsonl` exists, import safe summary rows with:

```bash
tokenzero pulse import-stats --dry-run
tokenzero pulse import-stats
```

Unavailable fields are left empty. Imported events are marked `source=legacy_stats`.
