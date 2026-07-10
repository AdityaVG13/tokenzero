# Code-Execution vs MCP-Schema Token Bake-off

> Bead: `tokenzero-da8.1` — "Fixed task suite (3–5 agent tasks) on a public corpus.
> Baselines: raw multi-tool MCP (simulated schema load), CLI-only, CodeMode
> zero_execute. Report: tokens, turns, wall, quality pass/fail — losses included.
> Artifact under benchmarks/ with BASELINE commit."
>
> Generator: `benchmarks/code-exec-vs-mcp-bakeoff.sh`
> Baseline commit: _fill from script output_

## Methodology

### Task suite

Five fixed agent tasks on the TokenZero public corpus (this repository):

1. **read_file** — read the first 20 lines of `Cargo.toml`.
2. **search_filter** — find every line containing `TokenZero` in `README.md`.
3. **edit_verify** — replace `beta` with `BETA` in a temp file, then read it back.
4. **multi_step_nav** — `tree` the repo (depth 2), `read` `Cargo.toml`, `find`
   `workspace` inside it.
5. **shell_expand** — run `find . -maxdepth 1 -name Cargo.toml` via the shell,
   then expand the resulting recovery ref.

### Approaches

| Approach | What is measured |
|---|---|
| **MCP-schema** | Simulated schema load: `tokenzero capabilities --json` is treated as the MCP tool-definition bundle. For each task, the JSON definitions for only the tools that task needs are extracted, serialized, and counted. This stands in for the per-turn schema tax a raw multi-tool MCP agent pays. No task is executed; quality is `simulated`. |
| **CLI-only** | Real `tokenzero` CLI commands. Input tokens = `ceil(command-string UTF-8 bytes / 4)`. Output tokens = `accounting.raw_tokens` from the CLI JSON envelope (the binary's own tokenizer). Turns = number of CLI invocations. Wall = summed wall-clock milliseconds. Quality = task-specific pass/fail. |
| **CodeMode** | Real `tokenzero codemode` JS plans (the same engine `zero_execute` exposes). Input tokens = `ceil(plan-string UTF-8 bytes / 4)`. Output tokens = `value.raw_tokens` from the codemode result envelope. Turns = `telemetry.logical_ops`. Wall = wall-clock milliseconds for the single `codemode` call. Quality = task-specific pass/fail. |

### Token counting

- Schema-load and command/plan input tokens use a **bytes/4** approximation
  (ceil) because the benchmark harness has no linked production tokenizer.
  This is a deliberate, documented proxy — final numbers should be recomputed
  with the agent's real tokenizer before external publication.
- CLI and CodeMode **output** tokens use TokenZero's own `raw_tokens`
  accounting, which is the same counter the runtime reports to agents.
- All three approaches are counted with the same methodology so the
  comparison is internally consistent.

### Quality rubric

Quality is binary (PASS/FAIL) and task-specific:

| Task | PASS condition |
|---|---|
| `read_file` | result contains `[workspace]` |
| `search_filter` | result contains `TokenZero` and at least one line break |
| `edit_verify` | result contains `BETA` and does not contain `beta` |
| `multi_step_nav` | result contains `workspace` (case-insensitive) |
| `shell_expand` | result contains `Cargo.toml` |

`MCP-schema` rows are marked `simulated` because they measure schema load
only, not task execution. `FAIL` rows are published unchanged — losses are
part of the comparison.

## Results

| Task | Approach | input_tokens | output_tokens | turns | wall_ms | quality |
|---|---|---:|---:|---:|---:|---|
| `read_file` | `MCP-schema` | … | 0 | 1 | … | simulated |
| `read_file` | `CLI` | … | … | … | … | … |
| `read_file` | `CodeMode` | … | … | … | … | … |
| `search_filter` | `MCP-schema` | … | 0 | 1 | … | simulated |
| `search_filter` | `CLI` | … | … | … | … | … |
| `search_filter` | `CodeMode` | … | … | … | … | … |
| `edit_verify` | `MCP-schema` | … | 0 | 1 | … | simulated |
| `edit_verify` | `CLI` | … | … | … | … | … |
| `edit_verify` | `CodeMode` | … | … | … | … | … |
| `multi_step_nav` | `MCP-schema` | … | 0 | 1 | … | simulated |
| `multi_step_nav` | `CLI` | … | … | … | … | … |
| `multi_step_nav` | `CodeMode` | … | … | … | … | … |
| `shell_expand` | `MCP-schema` | … | 0 | 1 | … | simulated |
| `shell_expand` | `CLI` | … | … | … | … | … |
| `shell_expand` | `CodeMode` | … | … | … | … | … |

### Aggregate (fill after a run)

| Approach | total input | total output | total turns | total wall | passes |
|---|---:|---:|---:|---:|---:|
| MCP-schema | … | 0 | … | … | 0/5 |
| CLI | … | … | … | … | …/5 |
| CodeMode | … | … | … | … | …/5 |

## Positioning vs Anthropic code-execution post

Anthropic's code-execution framing trades many narrow MCP tool schemas for one
broader code-execution surface. The trade-off is the same one this bake-off
measures: a raw multi-tool MCP agent pays a per-turn schema tax (every tool
definition the model must see before it can choose one), while a code-exec
agent pays one plan-string cost and amortizes tool dispatch across the whole
plan. CodeMode is TokenZero's instantiation of that idea — the agent writes a
small JS plan against the `zero.*` surface, and the runtime collapses multiple
read/search/shell/expand steps into a single round-trip with one result
envelope.

The bake-off does not assume CodeMode wins everywhere. `edit_verify` is the
known stress case: `zero.edit` is currently denied in the CodeMode sandbox
until transaction support lands, so the CodeMode row is expected to `FAIL`
while the CLI row `PASS`es. That loss is published, not hidden. The point is
to show where the code-exec model helps (multi-step navigation, shell+expand)
and where it still owes a fallback (mutation).

## Integrity note (tokenzero-bl6)

- Numbers are generated by a committed, reproducible script — no hand edits.
- The bytes/4 token proxy is disclosed openly in the methodology, not buried.
- Losses are published: `FAIL` rows and `simulated` rows appear in the table
  with the same prominence as `PASS` rows.
- If a run cannot complete a task, the row shows the failure and the script
  exit status reflects it; no row is silently dropped.
- Baseline commit is captured in the script output so any later regression is
  comparable to the original artifact.

## How to reproduce

```bash
# Build the binary under test (one-time)
cargo build --release --bin tokenzero

# Run the bake-off and capture the results table
./benchmarks/code-exec-vs-mcp-bakeoff.sh > benchmarks/code-exec-vs-mcp-report.md

# Override the binary under test
TOKENZERO_BIN=$PWD/target/release/tokenzero \
  ./benchmarks/code-exec-vs-mcp-bakeoff.sh > results.md
```

Artifacts: temp edit file and capabilities cache live under `/tmp` and are
cleaned up on exit. No repository files are modified.
