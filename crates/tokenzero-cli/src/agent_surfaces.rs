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

const COMMANDS: &[CommandSurface] = &[
    CommandSurface {
        name: "read",
        aliases: &[],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero read <path> --json",
        description: "Read bounded file content with exact recovery refs.",
    },
    CommandSurface {
        name: "find",
        aliases: &["grep", "search"],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero find <query> <path> --json",
        description: "Search local text and return compact matches.",
    },
    CommandSurface {
        name: "recall",
        aliases: &[],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero recall <query> --json",
        description: "Search payloads already stored in the recovery cache.",
    },
    CommandSurface {
        name: "fetch",
        aliases: &[],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero fetch <url> --json",
        description: "Fetch an http(s) URL via curl with a TTL cache and exact refs.",
    },
    CommandSurface {
        name: "glob",
        aliases: &[],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero glob '<pattern>' <path> --json",
        description: "List matching paths without dumping file contents.",
    },
    CommandSurface {
        name: "tree",
        aliases: &[],
        category: "context",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero tree <path> --json",
        description: "Inspect a bounded directory tree.",
    },
    CommandSurface {
        name: "edit",
        aliases: &[],
        category: "execution",
        mutates: true,
        json: true,
        primary_invocation: "tokenzero edit <path> --edits-json '<json>' --json",
        description: "Apply multi-hunk find/replace edits to one file: all-or-nothing, atomic write, undo ref.",
    },
    CommandSurface {
        name: "run",
        aliases: &[
            "shell",
            "rn",
            "run <command>",
            "run --json <command>",
            "run <command> --json",
            "--jsno",
            "--jason",
            "--timout",
        ],
        category: "execution",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero run --json -- <command>",
        description: "Run a command with status-truth telemetry and refs; common JSON/timeout typos and missing -- delimiters are recovered.",
    },
    CommandSurface {
        name: "expand",
        aliases: &[],
        category: "recovery",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero expand <tz-ref> --raw",
        description: "Recover exact bytes from a prior TokenZero ref.",
    },
    CommandSurface {
        name: "mem",
        aliases: &["cache status", "cache statuz"],
        category: "state",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero mem --json",
        description: "Inspect recovery-cache state.",
    },
    CommandSurface {
        name: "pulse",
        aliases: &["pulse stats", "pulse status"],
        category: "state",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero pulse --json",
        description: "Inspect local Pulse telemetry; stats/status recover to the read-only report.",
    },
    CommandSurface {
        name: "doctor",
        aliases: &["doctor health", "doctor status", "doctor statuz"],
        category: "health",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero doctor --json",
        description: "Check local TokenZero health and next steps.",
    },
    CommandSurface {
        name: "install",
        aliases: &["install plan", "install status"],
        category: "setup",
        mutates: true,
        json: true,
        primary_invocation: "tokenzero install --plan --json",
        description: "Plan or apply local integration writes with rollback data; --hooks wires the Claude Code PreToolUse hook, --shims installs the universal PATH shims, and install status recovers to clients detect.",
    },
    CommandSurface {
        name: "hook claude-code",
        aliases: &[],
        category: "setup",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero hook claude-code",
        description: "Claude Code PreToolUse adapter: reads hook JSON on stdin, rewrites Bash commands to run under `tokenzero run`, and always exits 0 (fail-open).",
    },
    CommandSurface {
        name: "capabilities",
        aliases: &["capability", "capabilites", "--jsno", "--jason"],
        category: "agent-contract",
        mutates: false,
        json: true,
        primary_invocation: "tokenzero capabilities --json",
        description: "Emit the machine-readable CLI contract for agents.",
    },
    CommandSurface {
        name: "robot-docs guide",
        aliases: &[
            "robot-doc guide",
            "robotdocs guide",
            "--robot-help",
            "robot-help",
            "robot-docs manual",
            "robot-docs commands",
            "robot-docs examples",
        ],
        category: "agent-contract",
        mutates: false,
        json: false,
        primary_invocation: "tokenzero robot-docs guide",
        description: "Print a paste-ready agent guide with canonical commands.",
    },
];

const EXIT_CODES: &[ExitCode] = &[
    ExitCode {
        code: 0,
        label: "success",
        meaning: "The requested command completed.",
        retryable: false,
    },
    ExitCode {
        code: 1,
        label: "blocked",
        meaning: "TokenZero refused or could not complete a requested operation; JSON includes a stable error or finding.",
        retryable: false,
    },
    ExitCode {
        code: 2,
        label: "usage",
        meaning: "The CLI invocation was malformed; rerun with the exact command shown in the error or help output.",
        retryable: false,
    },
];

const FEATURES: &[&str] = &[
    "capabilities_json",
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
            "Use `tokenzero run --json -- <command>` for command telemetry; inspect `command_success`, not only process exit."
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
```

Recoveries: `capability`, `capabilites`, `robot-help`, `--robot-help`, `rn`, `shell`, `search`, `--jsno`, `--jason`, `--timout`, `cache statuz`, `doctor status`, `doctor statuz`, `pulse stats`, `pulse status`, `install plan`, and `install status` redirect to safe canonical surfaces.
"#
}

pub fn robot_docs_examples() -> &'static str {
    r#"# TokenZero Robot Examples

```bash
tokenzero capabilities --json | jq '.commands'
tokenzero search TokenZero AGENTS.md --json
tokenzero read Cargo.toml --json
tokenzero tree crates/tokenzero-cli --json
tokenzero rn rustc --version --json
tokenzero run --json -- cargo test -p tokenzero-cli
tokenzero doctor status --json
tokenzero pulse stats --json
tokenzero install status --json
```

For `run`, inspect `telemetry.command_success`, `telemetry.failed_segment`, and `telemetry.pipeline_rerun_command`.
"#
}
