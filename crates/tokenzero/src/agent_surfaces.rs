use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CommandSurface {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub category: &'static str,
    pub mutates: bool,
    pub json: bool,
    pub primary_invocation: &'static str,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExitCode {
    pub code: i32,
    pub label: &'static str,
    pub meaning: &'static str,
    pub retryable: bool,
}

const fn cmd(
    name: &'static str,
    aliases: &'static [&'static str],
    category: &'static str,
    mutates: bool,
    json: bool,
    primary_invocation: &'static str,
    description: &'static str,
) -> CommandSurface {
    CommandSurface {
        name,
        aliases,
        category,
        mutates,
        json,
        primary_invocation,
        description,
    }
}

const fn exit(code: i32, label: &'static str, meaning: &'static str, retryable: bool) -> ExitCode {
    ExitCode {
        code,
        label,
        meaning,
        retryable,
    }
}

const COMMANDS: &[CommandSurface] = &[
    cmd(
        "read",
        &[],
        "context",
        false,
        true,
        "tokenzero read <path> --json",
        "Read bounded file content with exact recovery refs.",
    ),
    cmd(
        "find",
        &["grep", "search"],
        "context",
        false,
        true,
        "tokenzero find <query> <path> --json",
        "Search local text and return compact matches.",
    ),
    cmd(
        "recall",
        &[],
        "context",
        false,
        true,
        "tokenzero recall <query> --json",
        "Search payloads already stored in the recovery cache.",
    ),
    cmd(
        "fetch",
        &[],
        "context",
        false,
        true,
        "tokenzero fetch <url> --json",
        "Fetch an http(s) URL via curl with a TTL cache and exact refs.",
    ),
    cmd(
        "glob",
        &[],
        "context",
        false,
        true,
        "tokenzero glob '<pattern>' <path> --json",
        "List matching paths without dumping file contents.",
    ),
    cmd(
        "tree",
        &[],
        "context",
        false,
        true,
        "tokenzero tree <path> --json",
        "Inspect a bounded directory tree.",
    ),
    cmd(
        "edit",
        &[],
        "execution",
        true,
        true,
        "tokenzero edit <path> --edits-json '<json>' --json",
        "Apply multi-hunk find/replace edits to one file: all-or-nothing, atomic write, undo ref.",
    ),
    cmd(
        "run",
        &[
            "shell",
            "rn",
            "run <command>",
            "run --json <command>",
            "run <command> --json",
            "--jsno",
            "--jason",
            "--timout",
        ],
        "execution",
        false,
        true,
        "tokenzero run --json -- <command>",
        "Run a command with status-truth telemetry and refs; common JSON/timeout typos and missing -- delimiters are recovered.",
    ),
    cmd(
        "expand",
        &[],
        "recovery",
        false,
        true,
        "tokenzero expand <tz-ref> --raw",
        "Recover exact bytes from a prior TokenZero ref.",
    ),
    cmd(
        "mem",
        &["cache status", "cache statuz"],
        "state",
        false,
        true,
        "tokenzero mem --json",
        "Inspect recovery-cache state.",
    ),
    cmd(
        "pulse",
        &["pulse stats", "pulse status"],
        "state",
        false,
        true,
        "tokenzero pulse --json",
        "Inspect local Pulse telemetry; stats/status recover to the read-only report.",
    ),
    cmd(
        "doctor",
        &["doctor health", "doctor status", "doctor statuz"],
        "health",
        false,
        true,
        "tokenzero doctor --json",
        "Check local TokenZero health and next steps.",
    ),
    cmd(
        "install",
        &["install plan", "install status"],
        "setup",
        true,
        true,
        "tokenzero install --plan --json",
        "Plan or apply local integration writes with rollback data; --hooks wires the Claude Code PreToolUse hook, --shims installs the universal PATH shims, and install status recovers to clients detect.",
    ),
    cmd(
        "hook claude-code",
        &[],
        "setup",
        false,
        true,
        "tokenzero hook claude-code",
        "Claude Code PreToolUse adapter: reads hook JSON on stdin, rewrites Bash commands to run under `tokenzero run`, and always exits 0 (fail-open).",
    ),
    cmd(
        "capabilities",
        &["capability", "capabilites", "--jsno", "--jason"],
        "agent-contract",
        false,
        true,
        "tokenzero capabilities --json",
        "Emit the machine-readable CLI contract for agents.",
    ),
    cmd(
        "codemode",
        &[],
        "agent-contract",
        false,
        true,
        "tokenzero codemode --json --budget <n> --stdin <<'EOF' … EOF",
        "Tier B shell trampoline: one line / heredoc runs a full plan; same tokenzero.codemode.v1 envelope and refs as MCP; --budget caps visible tokens.",
    ),
    cmd(
        "robot-docs guide",
        &[
            "robot-doc guide",
            "robotdocs guide",
            "--robot-help",
            "robot-help",
            "robot-docs manual",
            "robot-docs commands",
            "robot-docs examples",
        ],
        "agent-contract",
        false,
        false,
        "tokenzero robot-docs guide",
        "Print a paste-ready agent guide with canonical commands.",
    ),
];

const EXIT_CODES: &[ExitCode] = &[
    exit(0, "success", "The requested command completed.", false),
    exit(
        1,
        "blocked",
        "TokenZero refused or could not complete a requested operation; JSON includes a stable error or finding.",
        false,
    ),
    exit(
        2,
        "usage",
        "The CLI invocation was malformed; rerun with the exact command shown in the error or help output.",
        false,
    ),
];

const FEATURES: &[&str] = &[
    "capabilities_json",
    "codemode_surface",
    "exact_recovery_refs",
    "intent_inference_aliases",
    "json_output",
    "non_tty_output_discipline",
    "pipeline_rerun_guidance",
    "robot_docs_guide",
    "status_truth_shell",
];

fn commands_by_name() -> BTreeMap<&'static str, CommandSurface> {
    COMMANDS
        .iter()
        .copied()
        .map(|command| (command.name, command))
        .collect()
}

pub fn capabilities_json() -> serde_json::Value {
    json!({
        "schema_version": "tokenzero.capabilities.v1",
        "tool": "tokenzero",
        "version": env!("CARGO_PKG_VERSION"),
        "contract_version": 1,
        "features": FEATURES,
        "stdout_contract": {
            "rule": "stdout is data; stderr is diagnostics",
            "json_flag": "--json",
            "refs_are_recoverable_with": "tokenzero expand <tz-ref> --raw"
        },
        "feature_flags": {
            "json_output": true,
            "exact_recovery_refs": true,
            "status_truth_shell": true,
            "pipeline_rerun_guidance": true,
            "intent_inference_aliases": true,
            "capabilities_json": true,
            "codemode_surface": true,
            "robot_docs_guide": true
        },
        "commands": COMMANDS,
        "commands_by_name": commands_by_name(),
        "output_schemas": {
            "capabilities": {
                "schema_version": "tokenzero.capabilities.v1",
                "required_keys": [
                    "schema_version",
                    "tool",
                    "version",
                    "contract_version",
                    "features",
                    "feature_flags",
                    "commands",
                    "commands_by_name",
                    "exit_codes",
                    "env_vars"
                ]
            },
            "run": {
                "shape": "tool_response",
                "status_fields": [
                    "status",
                    "tool",
                    "telemetry.command_success",
                    "telemetry.status_label",
                    "telemetry.failed_segment",
                    "refs"
                ]
            }
        },
        "exit_codes": EXIT_CODES,
        "env_vars": [
            {
                "name": "NO_COLOR",
                "effect": "suppress color where supported"
            },
            {
                "name": "CI",
                "effect": "non-interactive output discipline"
            },
            {
                "name": "TOKENZERO_CACHE_PATH",
                "effect": "override recovery cache path when configured by wrappers"
            }
        ],
        "canonical_invocations": [
            "tokenzero capabilities --json",
            "tokenzero --robot-help",
            "tokenzero robot-help",
            "tokenzero robot-docs guide",
            "tokenzero robot-docs commands",
            "tokenzero read <path> --json",
            "tokenzero find <query> <path> --json",
            "tokenzero search <query> <path> --json",
            "tokenzero run --json -- <command>",
            "tokenzero doctor --json",
            "tokenzero doctor status --json",
            "tokenzero pulse stats --json",
            "tokenzero cache statuz --json",
            "tokenzero install plan --json",
            "tokenzero install status --json",
            "tokenzero install --hooks --plan --json",
            "tokenzero install --shims --plan --json",
            "tokenzero hook claude-code"
        ],
        "codemode": {
            "schema": "tokenzero.codemode.v1",
            "cli": "tokenzero codemode --json --budget <n> --stdin <<'EOF' … EOF",
            "tier": "B",
            "transport": "shell_trampoline",
            "discovery": [
                "tokenzero codemode 'search:read'",
                "tokenzero codemode 'describe:zero.read'"
            ],
            "plan_sources": [
                "--plan / PLAN positional",
                "--plan-file <path>",
                "--stdin or PLAN=-",
                "non-TTY stdin auto-read (heredoc / pipe)"
            ],
            "budget_flag": "--budget / --max-visible-tokens",
            "cache_default": "recovery-cache.json",
            "cache_note": "CodeMode shares the default recovery-cache.json with CLI expand/MCP so refs expand without re-running the producer. Pass --cache-path only for an isolated store.",
            "pattern": "https://developers.cloudflare.com/agents/tools/codemode/",
            "when_to_use": "Compose multi-step workflows on the same base tools as MCP but faster (fewer round-trips, composition via plans, progressive search:/describe: discovery). Tier B shell trampoline for harnesses without MCP. Not an MCP tool."
        },
        "dangerous_operations": [
            {
                "command": "install",
                "safe_default": "tokenzero install --plan --json",
                "mutation_gate": "--apply"
            },
            {
                "command": "cache prune",
                "safe_default": "tokenzero cache prune --json",
                "mutation_gate": "--apply"
            }
        ],
        "agent_next_steps": [
            "Start with `tokenzero capabilities --json` to discover the contract.",
            "Use `--json` for read/find/tree/run/doctor when composing with jq or another agent.",
            "`tokenzero search <query> <path> --json` is accepted as an agent-friendly alias for `find`.",
            "Use refs from JSON responses with `tokenzero expand <tz-ref> --raw` instead of re-reading broad files.",
            "If you type `tokenzero run true --json` or `tokenzero run --jason true`, TokenZero recovers to `tokenzero run --json -- true`.",
            "If you type `tokenzero rn true --json`, TokenZero recovers to `tokenzero run --json -- true`.",
            "`tokenzero doctor status --json`, `tokenzero pulse stats --json`, `tokenzero cache statuz --json`, and `tokenzero install plan --json` recover to safe read-side or plan surfaces.",
            "`tokenzero install status --json` recovers to `tokenzero clients detect --json`.",
            "Use `tokenzero run --json -- <command>` for command telemetry; inspect `command_success`, not only process exit.",
            "Read resource://tokenzero/codemode for the full CodeMode method catalog.",
            "CodeMode is a separate plan-based execution layer on the same base tools/engine. Faster for multi-step workflows (fewer round-trips). Tier B trampoline: `tokenzero codemode --json --budget <n> --stdin` (heredoc OK)."
        ]
    })
}

pub fn robot_docs_guide() -> &'static str {
    r#"# TokenZero Robot Guide

TokenZero is an agent-facing context runtime. Use it when you need bounded file reads, search, trees, command telemetry, and exact recovery refs.

## First Commands

```bash
tokenzero capabilities --json
tokenzero --robot-help
tokenzero robot-help
tokenzero robot-docs guide
tokenzero robot-docs commands
tokenzero doctor --json
tokenzero doctor status --json
tokenzero pulse stats --json
tokenzero install status --json
```

## Context

```bash
tokenzero read <path> --json
tokenzero find <query> <path> --json
tokenzero search <query> <path> --json
tokenzero tree <path> --json
tokenzero expand <tz-ref> --raw
```

Prefer refs from TokenZero responses over broad re-reads. `expand` recovers exact bytes from `tz://...` refs.

## Shell

```bash
tokenzero run --json -- <command>
```

For shell results, inspect `telemetry.command_success`, `telemetry.status_label`, `telemetry.failed_segment`, and `telemetry.pipeline_rerun_command`. Do not infer success from transport exit alone.
Common recoveries: `tokenzero run true --json`, `tokenzero run --json true`, `tokenzero run --jsno true`, `tokenzero run --jason true`, and `tokenzero run --timout 5 true` are normalized to the canonical run shape.
`tokenzero rn true --json` is treated as the common typo for `tokenzero run --json -- true`.
Setup/status recoveries are read-side by default: `tokenzero doctor status --json`, `tokenzero pulse stats --json`, `tokenzero cache statuz --json`, `tokenzero install plan --json`, and `tokenzero install status --json` all avoid unintended writes.

## Output Contract

Stdout is data. Stderr is diagnostics. JSON commands include `schema_version` or `tool`/`status` fields and stable refs when recovery is available.

## Exit Codes

0 means success. 1 means TokenZero blocked or could not complete the operation. 2 means command-line usage error. For command telemetry, inspect the JSON telemetry fields because the wrapper can transport a failed child command successfully.

## Safe Mutation Defaults

`tokenzero install` defaults to a plan. Use `tokenzero install --plan --json` before any `--apply`. `tokenzero cache prune --json` is a dry run unless `--apply` is supplied.

## CodeMode (plan-based execution on the same engine)

CodeMode dispatches through the **exact same TokenZeroEngine and tool implementations** as the MCP `tz_*` surface. The difference is execution shape: instead of one round-trip per operation, you compose multi-step workflows in a single plan call.

Tier B shell trampoline (any harness with a bash tool): one shell line / stdin heredoc runs a full plan, emits the same `tokenzero.codemode.v1` envelope and `tz://` refs as MCP, and honors `--budget` / `--max-visible-tokens` so output fits harness result caps. Errors are typed (`status=error` + `error.kind`); empty or conflicting plan sources never silently succeed.

```bash
# Tier B trampoline (stdin heredoc + visible-token budget)
tokenzero codemode --json --budget 2000 --root . --stdin <<'EOF'
const f = await zero.read("src/main.rs");
const hits = await zero.grep("TODO", "src/");
return { file: f.ref, todos: hits.text };
EOF

# Multi-step in one call (inline plan)
tokenzero codemode --json --plan 'const f = await zero.read("src/main.rs"); const hits = await zero.grep("TODO", "src/"); return { file: f.ref, todos: hits.text }'

# Progressive discovery
tokenzero codemode 'search:read'        # find methods by keyword (includes signatures + examples)
tokenzero codemode 'describe:zero.read'  # full signature, example, related methods
```

Plan-level helpers (not in MCP, only available within plans):
- `zero.pipe(steps)` — sequential composition with auto-threaded `_prev`
- `zero.pick(obj, keys)` — project specific keys from a result
- `zero.filter_lines(text, pattern)` — filter output lines in-plan
- `zero.count_tokens(data)` — introspect token/byte/line count without storing
- `zero.assert(condition, msg)` — fail-fast guard within a plan
- `zero.compact_max(data)` — aggressive content-aware compression with recovery

All `zero.*` methods that touch files, shell, or cache dispatch through the same code path as `tz_read`, `tz_find`, `tz_shell`, etc. Refs from one surface work in the other (`tz_expand` accepts refs from CodeMode plans and vice versa).

Cache: CodeMode defaults to the same `recovery-cache.json` as CLI expand/MCP (under `.tokenzero/` or `.zerostack/tokenzero/`). Isolated stores require matching `--cache-path` on both mint and expand.
"#
}

pub fn robot_docs_commands() -> &'static str {
    r#"# TokenZero Robot Commands

```bash
tokenzero capabilities --json
tokenzero robot-docs guide
tokenzero robot-docs commands
tokenzero read <path> --json
tokenzero find <query> <path> --json
tokenzero search <query> <path> --json
tokenzero tree <path> --json
tokenzero run --json -- <command>
tokenzero doctor --json
tokenzero doctor status --json
tokenzero pulse stats --json
tokenzero install status --json
tokenzero codemode --json --plan 'await zero.compact("payload")'
tokenzero codemode 'search:read'
```

Recoveries: `capability`, `capabilites`, `robot-help`, `--robot-help`, `rn`, `shell`, `search`, `--jsno`, `--jason`, `--timout`, `cache statuz`, `doctor status`, `doctor statuz`, `pulse stats`, `pulse status`, `install plan`, and `install status` redirect to safe canonical surfaces.

CodeMode shares `recovery-cache.json` with expand/MCP by default. CodeMode is a separate plan-based execution on the same base tools (not an MCP tool).
"#
}

pub fn robot_docs_examples() -> &'static str {
    r#"# TokenZero Robot Examples

```bash
tokenzero capabilities --json | jq '.commands'
tokenzero search TokenZero AGENTS.md --json
tokenzero read Cargo.toml --json
tokenzero tree crates/tokenzero --json
tokenzero rn rustc --version --json
tokenzero run --json -- cargo test -p tokenzero
tokenzero doctor status --json
tokenzero pulse stats --json
tokenzero install status --json
tokenzero codemode --json --plan 'const t = await zero.read("README.md"); return t'
```

For `run`, inspect `telemetry.command_success`, `telemetry.failed_segment`, and `telemetry.pipeline_rerun_command`.
For CodeMode, inspect the `value` field in the JSON envelope; use `search:` / `describe:` for in-plan discovery.
"#
}
