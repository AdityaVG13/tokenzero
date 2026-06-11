//! Agent-harness hook adapters.
//!
//! `tokenzero hook claude-code` is a Claude Code `PreToolUse` adapter: it
//! reads one hook JSON object from stdin and, for Bash tool calls, emits a
//! decision that transparently rewrites the command to run under
//! `tokenzero run`. The contract is strictly fail-open — a hook that exits
//! nonzero or emits garbage would degrade every Bash call in the harness, so
//! every error path here is "exit 0, no output" (pass through unchanged).

use std::io::{Read, Write};

use serde_json::{Value, json};

use crate::cli_args::{HookArgs, HookTarget};

/// Interactive/TTY-bound launchers that must never be wrapped: the wrapper
/// captures stdio, which would hang or break these programs.
const SKIP_PROGRAMS: &[&str] = &[
    "vim",
    "vi",
    "nano",
    "less",
    "more",
    "top",
    "htop",
    "ssh",
    "python -i",
    "irb",
    "psql",
    "mysql",
    "docker exec -it",
    "git rebase -i",
    "git add -i",
    "sudo",
];

pub(crate) fn handle_hook(args: HookArgs) {
    match args.target {
        HookTarget::ClaudeCode(hook) => run_claude_code_hook(&hook.mode),
        HookTarget::ClaudeCodeSessionStart(hook) => run_session_start_hook(hook.max_tokens),
    }
}

/// SessionStart adapter: after a compaction or resume wiped the model's
/// context, inject a compact pack of what TokenZero already served this
/// workspace (exact refs, recall pointer) so the agent re-orients without
/// re-reading files or re-running commands. Fail-open like every hook.
fn run_session_start_hook(max_tokens: usize) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    if let Some(decision) = session_start_decision(&input, max_tokens) {
        let _ = writeln!(std::io::stdout(), "{decision}");
    }
}

fn session_start_decision(input: &str, max_tokens: usize) -> Option<Value> {
    let payload: Value = serde_json::from_str(input).ok()?;
    if payload.get("hook_event_name")?.as_str()? != "SessionStart" {
        return None;
    }
    // Fresh sessions have no prior context to restore; only compaction and
    // resume lose it.
    let source = payload.get("source").and_then(Value::as_str)?;
    if !matches!(source, "compact" | "resume") {
        return None;
    }
    let cwd = payload.get("cwd").and_then(Value::as_str)?;
    let cache = std::path::Path::new(cwd)
        .join(".tokenzero")
        .join("recovery-cache.json");
    let pack = tokenzero_mcp::session_pack(&cache, max_tokens.max(50))?;
    Some(json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": pack,
        }
    }))
}

/// PreToolUse adapter loop: read stdin once, print at most one decision
/// object, and always return so `main` exits 0. Exit 2 would block the tool
/// call outright and any other nonzero exit surfaces an error to the model.
fn run_claude_code_hook(mode: &str) {
    let mut input = String::new();
    if std::io::stdin().read_to_string(&mut input).is_err() {
        return;
    }
    // current_exe keeps the rewrite correct for repo builds and renamed
    // installs; without it there is no reliable wrapper path, so pass through.
    let Some(self_exe) = std::env::current_exe()
        .ok()
        .and_then(|path| path.to_str().map(str::to_string))
    else {
        return;
    };
    let no_wrap = no_wrap_enabled(std::env::var("TOKENZERO_NO_WRAP").ok());
    if let Some(decision) = claude_code_decision(mode, &input, &self_exe, no_wrap) {
        // A closed stdout is still a pass-through, never a hard failure.
        let _ = writeln!(std::io::stdout(), "{decision}");
    }
}

/// Pure decision core, unit-testable without process state. Returns `None`
/// for every pass-through (malformed payload, non-Bash tool, skip rules,
/// `off` mode, and unknown `--mode` values — a misconfigured flag must not
/// change Bash behavior).
fn claude_code_decision(mode: &str, input: &str, self_exe: &str, no_wrap: bool) -> Option<Value> {
    let payload: Value = serde_json::from_str(input).ok()?;
    if payload.get("tool_name")?.as_str()? != "Bash" {
        return None;
    }
    let tool_input = payload.get("tool_input")?.as_object()?;
    let command = tool_input.get("command")?.as_str()?;
    if no_wrap {
        return None;
    }
    match mode {
        "rewrite" => {
            let rewritten = rewrite_decision(command, self_exe)?;
            // Clone tool_input so sibling keys (timeout, description, ...)
            // survive: updatedInput replaces the whole tool_input object.
            let mut updated = tool_input.clone();
            updated.insert("command".to_string(), Value::String(rewritten));
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "updatedInput": Value::Object(updated),
                }
            }))
        }
        "guide" => {
            if should_skip(command) {
                return None;
            }
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "deny",
                    "permissionDecisionReason": "TokenZero routing: use the TokenZero MCP tools (read/find/grep/glob/tree/shell) for this operation, or run it as `tokenzero run -- <command>` to keep output compact and recoverable.",
                }
            }))
        }
        _ => None,
    }
}

/// Wrap `command` so it executes under `tokenzero run`, or `None` to pass
/// through unchanged.
///
/// Delivery mechanism: `tokenzero run --stdin` pipes stdin to the *child
/// process* (`handle_run` forwards it as the spawn's stdin payload); it does
/// NOT read the command text from stdin, so a quoted-heredoc delivery would
/// execute an empty command. The wrapper therefore uses
/// `run -- sh -c '<command>'` with POSIX single-quote escaping: the harness
/// shell hands tokenzero the original bytes verbatim (quotes, `$VAR`,
/// backticks, newlines untouched), and the inner `sh -c` applies normal shell
/// semantics at execution time, exactly like the unwrapped command.
pub(crate) fn rewrite_decision(command: &str, self_exe: &str) -> Option<String> {
    if should_skip(command) {
        return None;
    }
    Some(format!(
        "{} run -- sh -c {}",
        single_quote(self_exe),
        single_quote(command)
    ))
}

/// The env-var opt-out is value-aware: `0`/`false`/`off` keep wrapping ON;
/// any other present value disables it.
fn no_wrap_enabled(value: Option<String>) -> bool {
    value.is_some_and(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "off"))
}

/// Programs whose execution mutates persistent-shell state the harness
/// tracks across Bash calls (cwd, exported vars): wrapping them confines the
/// change to the wrapper's child shell and silently desyncs later calls.
const STATE_PROGRAMS: &[&str] = &["cd", "export", "unset", "alias"];

fn should_skip(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return true;
    }
    // Any mention anywhere: cheap, safe protection against double-wrapping
    // and recursive self-invocation.
    if trimmed.contains("tokenzero") {
        return true;
    }
    // An inline `TOKENZERO_NO_WRAP=...` assignment prefix is an explicit
    // per-command opt-out, regardless of value.
    if trimmed.starts_with("TOKENZERO_NO_WRAP=") {
        return true;
    }
    // Heredoc bodies (and herestrings) are whitespace-sensitive multi-line
    // constructs; pass them through rather than risk a re-quoting trip.
    if trimmed.contains("<<") {
        return true;
    }
    // Background jobs anywhere at top level (`server & curl ...`, trailing
    // `&`): the wrapper waits on its child's pipes, which turns
    // fire-and-forget into a blocking call that gets killed at the shell
    // deadline.
    let Some(segments) = top_level_segments(trimmed) else {
        return true;
    };
    // A dangling operator leaves an empty segment (e.g. `sleep 5 &&`):
    // malformed or half-typed input passes through untouched.
    if segments.iter().any(|segment| segment.trim().is_empty()) {
        return true;
    }
    // Persistent-shell state changes and interactive programs are skipped in
    // ANY top-level segment, not just the first: `make && cd build` desyncs
    // the harness cwd and `make && vim x` hangs on captured stdio the same
    // way their standalone forms do.
    segments.iter().any(|segment| {
        let segment = segment.trim();
        STATE_PROGRAMS
            .iter()
            .chain(SKIP_PROGRAMS.iter())
            .any(|program| starts_with_program(segment, program))
    })
}

/// Split a command at top-level `&&`, `||`, `;`, and `|` boundaries,
/// respecting single/double quotes and backslash escapes. Returns `None`
/// when a top-level job-control `&` is present (callers skip those commands
/// entirely); `>&` / `&>` redirections are not job control.
fn top_level_segments(command: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = command.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;
    let mut prev: Option<char> = None;
    while let Some(ch) = chars.next() {
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            current.push(ch);
            prev = Some(ch);
            continue;
        }
        if in_double {
            if ch == '\\' {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                    prev = Some(next);
                }
                continue;
            }
            if ch == '"' {
                in_double = false;
            }
            current.push(ch);
            prev = Some(ch);
            continue;
        }
        match ch {
            '\\' => {
                current.push(ch);
                if let Some(next) = chars.next() {
                    current.push(next);
                    prev = Some(next);
                }
                continue;
            }
            '\'' => {
                in_single = true;
                current.push(ch);
            }
            '"' => {
                in_double = true;
                current.push(ch);
            }
            '&' => {
                if chars.peek() == Some(&'&') {
                    chars.next();
                    segments.push(std::mem::take(&mut current));
                    prev = None;
                    continue;
                }
                // `2>&1` / `>&2` / `&>file` are redirections, not jobs.
                if prev == Some('>') || chars.peek() == Some(&'>') {
                    current.push(ch);
                    prev = Some(ch);
                    continue;
                }
                return None;
            }
            '|' => {
                if chars.peek() == Some(&'|') {
                    chars.next();
                }
                segments.push(std::mem::take(&mut current));
                prev = None;
                continue;
            }
            ';' => {
                segments.push(std::mem::take(&mut current));
                prev = None;
                continue;
            }
            _ => current.push(ch),
        }
        prev = Some(ch);
    }
    segments.push(current);
    Some(segments)
}

/// Prefix match on a whole-token boundary: `vi` matches `vi notes.txt` but
/// not `vim notes.txt`; `git rebase -i` matches `git rebase -i main` but not
/// `git rebase -into`.
fn starts_with_program(command: &str, program: &str) -> bool {
    match command.strip_prefix(program) {
        Some(rest) => rest.is_empty() || rest.starts_with(char::is_whitespace),
        None => false,
    }
}

fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests;
