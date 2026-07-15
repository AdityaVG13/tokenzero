# TokenZero Core

Core is the installable runtime: compact context now, exact recovery later.

Core is local-first, CLI-first, MCP-ready, plan-first, and Rust-first. It has
no default daemon, no watcher, no vector database, no
pre-index, no mandatory repository indexing, no remote service, no cloud
account, no API key, and no external runtime requirement.

Launch offer: Compress aggressively. Recover exactly. One install.

## Commands

```bash
tokenzero read README.md
tokenzero find "fn main" .
tokenzero tree . --depth 2
tokenzero run -- cargo test --workspace
tokenzero run --mode auto --budget 1200 -- cargo test --workspace
tokenzero cache-pack --scope agent --json
tokenzero bench competitors --suite shell-heavy --json
some-tool --verbose | tokenzero ingest --stdin --kind tool-output
tokenzero expand tz://file/<id> --start-line 1 --end-line 40
tokenzero expand tz://file/<id> --summary
tokenzero stats --json
tokenzero cache
tokenzero pulse
```

## Read

`tokenzero read` returns a visible capsule plus exact recovery refs. Use `--mode fidelity` when preparing edits and `--raw --start-line N --end-line M` when exact contiguous text is required.

## Find

`tokenzero find` searches one or more roots and returns grouped recoverable hits. Zero-hit searches use a small fast path instead of a bloated capsule.

## Tree

`tokenzero tree` gives bounded repo shape. It respects gitignore by default and exposes structured entries only when requested.

## Run

`tokenzero run -- <command>` stores exact stdout, stderr, combined shell
output, and a capture record before rendering a visible view. Auto mode chooses
one of `passthrough`, `diagnostic`, `structured`, `dedupe`, `diff-aware`, or
`exact`; legacy `hybrid`, `critical`, and `fidelity` modes are compatibility
aliases that normalize on input (`hybrid` → `auto`, `critical` → `diagnostic`,
`fidelity` → `structured`) — responses always echo the normalized mode name,
not the alias you sent.

Shell JSON separates TokenZero transport success from child command success:
`transport_status`, `command_success`, `exit_code`, `failed_segment`,
`pipeline_masking_warning`, and `status_label` are explicit. A command that
exits nonzero, fails a `cd`, or masks `false | true` is not labeled as command
success. Masking detection works on the visible pipeline structure: deeply
nested subshell pipelines may not be fully decomposed, and the
`pipeline_rerun_command` hint (rerun under `pipefail`) is the definitive
check when it matters.

`--budget <tokens>` constrains visible rendering. Critical errors, warnings,
diff hunks, prompts, and status hazards remain visible or have exact refs.

## Expand

`tokenzero expand <ref>` recovers exact payloads, ranges, search hits, anchors, summaries, and symbols from `tz://`, `fz://`, or `gz://` refs. `fz://`/`gz://` remain **same-store scheme aliases** when rewritten into the TokenZero store; blob refs also expand across engines under a verified shared ZeroStack CAS / sibling store when the release's retained multi-OS evidence gate is green (see `docs/zeroref-v1-contract.md`). The checked-in fixture alone is not release evidence. Non-blob portable refs stay engine-specific. Exact refs are not counted as model-readable context until expanded.

Selectors support raw, `error_block`, `summary`, `lines:N-M`, `around:N:R`, `anchor:<kind>`, and `symbol:<name>`. Batch recovery uses multiple refs or `--refs-from`:

```bash
tokenzero expand tz://file/<id> --lines 20-40
tokenzero expand tz://file/<id> --around 55:5
tokenzero expand --refs-from refs.txt --summary --json
```

### Crash-only recovery ladder (CodeMode)

On `--mode=codemode`, per-op tools stay crash-only while the primary surface is healthy. Prefer `zero.token.expand` inside `tz_execute_code`. If expand returns X0 or the substrate is down, surface health unlocks **only** `tz_expand` / `tz_read` (and CLI `tokenzero expand` / `tokenzero read`) so agents recover bytes without native Read. Write/shell stay locked. After a successful expand, recovery re-locks. Telemetry: `resource://tokenzero/metrics` → `surface_health` (`blocked_count` / `unlocked_count`). Never claim "primary surface healthy" after expand X0.

## Ingest

`tokenzero ingest` turns external tool output, repo packs, logs, diffs, or copied payloads into TokenZero capsules with exact refs. Use it when another tool is useful but you still want TokenZero to own recovery:

```bash
external-tool --json | tokenzero ingest --stdin --kind tool-output
tokenzero expand <exact_ref> --raw --force
```

Kind hints include `shell`, `diff`, `log`, `markdown`, `json`, `code`, `pack`, `tool-output`, and `auto`. Pack ingest keeps a whole-pack ref and, for markdown-like or XML-like packs, section refs for individual files or sections.

## Cache And Pulse

Core uses bounded local recovery state. Pulse records counts, refs, cache flags, latency, and health flags. It does not record raw payloads by default. Exact, digest, capsule, and Pulse cache state can be inspected and cleared explicitly without starting a daemon.

### Multi-project store isolation (wqw.2)

Default recovery / CodeMode store paths are **per call root**, not a process-global pin:

1. If `root/.zerostack` exists → use `root/.zerostack/tokenzero/...`.
2. Else if **shared-store opt-in** is on and `ZEROSTACK_STORE_ROOT` (or `ZERO_STACK_STORE_ROOT`) is set → use that shared/meta store.
3. Else → legacy `root/.tokenzero/...`.

**Shared store is opt-in only.** Set `TOKENZERO_SHARED_STORE=1` or `ZEROSTACK_SHARED_STORE=1` (`on`/`true`/`yes` also work) to honor a global `ZEROSTACK_STORE_ROOT` pin for meta-workspaces. Without opt-in, a set `ZEROSTACK_STORE_ROOT` is **ignored** so unrelated projects never collate recovery caches.

Precedence above defaults: explicit `--cache-path` / `TOKENZERO_CACHE_PATH`.

`tokenzero doctor --json` reports `store_resolution` / `effective_cache_path` and emits findings when a global pin is ignored or when shared opt-in places the store outside the project root.

`tokenzero cache-pack --scope agent --json` builds a daemonless prompt-cache
pack from stable instruction files, docs, repo map, and MCP tool schema. The
response includes `cache_key`, `content_digest`, `cacheable_tokens`,
`volatile_tokens`, source refs, volatile-tail refs, and invalidation reason. It
does not start a watcher or service.

`tokenzero bench competitors --suite <suite> --json` writes internal benchmark
rows to a private artifact path by default. Rows include raw, visible, and
recovery tokens, recovery-adjusted savings, byte-perfect recovery, task success,
harm rate, latency, host coverage, interception depth, and Safe Savings. Public
docs do not publish competitor results or benchmark claims without approved
evidence.

## Safety

CLI and MCP file access is confined to the configured allowed roots — the workspace root (`TOKENZERO_ROOT` or the current directory) by default, extended with `--allowed-root`. The root check is component-wise and fails closed on paths whose `..` segments cannot be resolved. Mutating commands remain explicit. Install, rollback, cache prune, and Pulse compaction are dry-run-first or require explicit apply.

The recovery cache stores the exact bytes of everything TokenZero has served for its workspace, and `tz_expand`/`tz_recall` serve cache contents by ref without re-checking the origin path against the current allowed roots. The cache file is therefore part of the workspace trust domain: point a server or CLI at a workspace's cache (`--cache-path`) only when its operator is allowed to read that workspace.

## Scope

| User job | Core surface | v1 stance |
| --- | --- | --- |
| File reads | `tokenzero read`, `tz_read`, exact refs, range recovery | Core |
| Search | `tokenzero find`, `tz_find`, search-hit refs | Core |
| Repo trees | `tokenzero tree`, `tz_tree` | Core |
| Shell and test output | `tokenzero run`, `tz_shell` | Core |
| External tool output | `tokenzero ingest --stdin --kind tool-output` | Core candidate |
| Exact recovery | `tokenzero expand`, `tz_expand` | Core |
| Savings telemetry | `tokenzero stats`, Pulse, recovery-adjusted event accounting | Core |
| Client setup | `tokenzero install --plan`, `install --apply`, `client-status`, rollback | Core |
| Command rewrite | `rewrite-command`, `run --rewrite safe` for allowlisted read/search/tree/git summaries | Core |
| Graph retrieval or vector memory | Future optional plugin only | Not default Core |
| Provider-side compression | No Core surface | Not Core |
