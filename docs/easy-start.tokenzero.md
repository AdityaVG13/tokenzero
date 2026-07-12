<!--
  TokenZero section of the unified ZeroStack Easy-start prompt.
  Composition contract (coordinate changes with the zerostack umbrella repo):
  - Section root is a single H2; no H1 in this file. The umbrella concatenates
    engine sections under its own H1 in the order fszero, graphzero, tokenzero.
  - Self-contained: no links or references to other sections; plain fenced
    blocks only; keep under ~120 lines so the assembled prompt stays cheap.
  - Byte-stable: edit deliberately; this text is a prompt-prefix cache seed.
-->

## TokenZero — context runtime (reads, search, shell, exact recovery)

TokenZero returns compact capsules plus stable refs instead of raw bytes.
Trust the ref: the exact bytes are always recoverable, so never re-read or
re-run something you already hold a ref for.

### Operation rules

- One plan beats N calls. Compose multi-step work in a single CodeMode plan:

  ```js
  const f = zero.read("src/main.rs");
  const hits = zero.grep("TODO", "src/");
  return { file: f.ref, todos: hits.text };
  ```

- Shell goes through `zero.token.shell(command)` / `tokenzero run --json -- <command>`.
  Judge success from `telemetry.command_success` and `failed_segment`, never
  from transport exit alone. Long commands: `{background: true}` returns
  `{job, log}` immediately; poll with `zero.token.job(id)` →
  `{status, pid, exitCode, tail, log}`.
- Repeated workflows: register once, invoke by name —
  `zero.register("triage", source)`, then `zero.run("triage", {path})`.
  Args bind as frozen data, never as code; every run revalidates policy.
- Stdout is data, stderr is diagnostics. Exit 0 success, 1 blocked/failed,
  2 usage error. Mutating commands default to a dry-run plan; nothing
  destructive happens without `--apply`.

### Refs and exact recovery

- Portable refs are `tz://blob/<64-hex-sha256>` (also `fz://`/`gz://` from
  sibling engines — TokenZero expands those too). Legacy 17-char short refs
  still resolve through a collision-safe alias chain.
- `expand <ref>` returns exact bytes, hash-verified. Fragments select ranges:
  `#B<start>-<end>` (zero-based half-open bytes), `#L<start>-<end>`
  (one-based inclusive lines, exact newlines). Malformed, reversed,
  out-of-bounds, uppercase, or truncated refs fail with stable error classes —
  they never silently return wrong bytes.
- Window large payloads instead of materializing them:
  `expand <ref> --start-line A --end-line B` (or `{start_line, end_line}`).
- A ref that fails to expand was GC'd or is foreign to this store; re-derive
  it (re-read the file) rather than retrying the same expand.

### Token economy

- Ref-first always: return `{thing: result.ref}` from plans; expand only the
  bytes you truly need, windowed where possible.
- Already-seen content is delta-suppressed within a session: a repeat serve
  costs a one-line `+<ref> (already seen)` note instead of the full capsule.
  Telemetry reports `session_delta.full_bytes/delta_bytes/saved_bytes`.
- `zero.count_tokens(data)` prices a payload before you ship it;
  `zero.compact_max(data)` compresses with recovery when you must inline.
- Never paste multi-KB blobs into prompts or shell args: ingest
  (`tokenzero ingest`) and pass the ref.

### First commands

```bash
tokenzero capabilities --json   # machine-readable CLI contract
tokenzero doctor --json         # runtime health
tokenzero robot-docs guide      # full agent guide
```
