#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

pub const CLI_SCHEMA_VERSION: &str = "tokenzero.cli.v1";
pub const MCP_SCHEMA_VERSION: &str = "tokenzero.mcp.v1";
pub const INSTALL_SCHEMA_VERSION: &str = "tokenzero.install_plan.v1";
pub const PULSE_SCHEMA_VERSION: &str = "tokenzero.pulse.v1";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Auto,
    Passthrough,
    Diagnostic,
    Structured,
    Dedupe,
    DiffAware,
    Exact,
    Hybrid,
    Critical,
    Fidelity,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "passthrough" => Ok(Self::Passthrough),
            "diagnostic" => Ok(Self::Diagnostic),
            "structured" => Ok(Self::Structured),
            "dedupe" => Ok(Self::Dedupe),
            "diff-aware" | "diff_aware" | "diffaware" => Ok(Self::DiffAware),
            "hybrid" => Ok(Self::Auto),
            "critical" => Ok(Self::Diagnostic),
            "fidelity" => Ok(Self::Structured),
            "exact" => Ok(Self::Exact),
            other => Err(format!("unsupported mode: {other}")),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Passthrough => "passthrough",
            Self::Diagnostic => "diagnostic",
            Self::Structured => "structured",
            Self::Dedupe => "dedupe",
            Self::DiffAware => "diff-aware",
            Self::Exact => "exact",
            Self::Hybrid => "hybrid",
            Self::Critical => "critical",
            Self::Fidelity => "fidelity",
        })
    }
}

impl Mode {
    pub fn effective_policy(self) -> Self {
        match self {
            Self::Hybrid => Self::Auto,
            Self::Critical => Self::Diagnostic,
            Self::Fidelity => Self::Structured,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Code,
    ShellOutput,
    SearchResult,
    Tree,
    Diff,
    JsonConfig,
    Markdown,
    Logs,
    Unknown,
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Code => "code",
            Self::ShellOutput => "shell_output",
            Self::SearchResult => "search_result",
            Self::Tree => "tree",
            Self::Diff => "diff",
            Self::JsonConfig => "json_config",
            Self::Markdown => "markdown",
            Self::Logs => "logs",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visible {
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefRecord {
    pub kind: String,
    #[serde(rename = "ref")]
    pub ref_id: String,
    pub bytes: usize,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Accounting {
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub recovery_tokens: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_ref_tokens: Option<usize>,
}

impl Accounting {
    pub fn visible_savings_ratio(&self) -> f64 {
        savings_ratio(self.raw_tokens, self.visible_tokens)
    }

    pub fn recovery_adjusted_savings_ratio(&self) -> f64 {
        savings_ratio(self.raw_tokens, self.visible_tokens + self.recovery_tokens)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResponse {
    pub schema_version: String,
    pub status: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible: Option<Visible>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<RefRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accounting: Option<Accounting>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CliError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety: Option<serde_json::Value>,
}

impl ToolResponse {
    pub fn ok(
        tool: impl Into<String>,
        mode: Mode,
        visible: String,
        refs: Vec<RefRecord>,
        accounting: Accounting,
    ) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION.to_string(),
            status: "ok".to_string(),
            tool: tool.into(),
            mode: Some(mode.to_string()),
            visible: Some(Visible {
                kind: "capsule".to_string(),
                text: visible,
            }),
            refs,
            accounting: Some(accounting),
            diagnostic: None,
            error: None,
            content_type: None,
            telemetry: None,
            safety: None,
        }
    }

    pub fn error(
        tool: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        repair: Option<String>,
    ) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION.to_string(),
            status: "error".to_string(),
            tool: tool.into(),
            mode: None,
            visible: None,
            refs: Vec::new(),
            accounting: None,
            diagnostic: None,
            error: Some(CliError {
                code: code.into(),
                message: message.into(),
                repair,
            }),
            content_type: None,
            telemetry: None,
            safety: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capsule {
    pub text: String,
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub omitted_lines: usize,
    pub mode: Mode,
}

pub fn make_capsule(
    text: &str,
    mode: Mode,
    max_visible_tokens: usize,
    label: Option<&str>,
) -> Capsule {
    let raw_tokens = count_tokens(text);
    make_capsule_with_raw_tokens(text, raw_tokens, mode, max_visible_tokens, label)
}

pub fn make_capsule_with_raw_tokens(
    text: &str,
    raw_tokens: usize,
    mode: Mode,
    max_visible_tokens: usize,
    label: Option<&str>,
) -> Capsule {
    make_capsule_with_recovery_ref(text, raw_tokens, mode, max_visible_tokens, label, None)
}

/// `make_capsule_with_raw_tokens` plus a recovery cue: when the payload's
/// exact ref is already known, budget truncation names it inline so the
/// agent can recover without consulting the response's refs footer.
pub fn make_capsule_with_recovery_ref(
    text: &str,
    raw_tokens: usize,
    mode: Mode,
    max_visible_tokens: usize,
    label: Option<&str>,
    recovery_ref: Option<&str>,
) -> Capsule {
    let line_count = text.lines().count();
    let prefix = capsule_prefix(label, max_visible_tokens, raw_tokens);
    let policy = mode.effective_policy();
    let visible = match policy {
        Mode::Exact => format!("{prefix}[exact payload stored; use expand for raw bytes]"),
        Mode::Passthrough => format!("{prefix}{}", text.trim_end()),
        Mode::Diagnostic => {
            let block = error_block(text, 3);
            if block.trim().is_empty() {
                summarize_lines(text, 8, 6, &prefix)
            } else {
                format!("{prefix}{}", block.trim_end())
            }
        }
        Mode::Structured => summarize_lines(text, 24, 16, &prefix),
        Mode::Dedupe => format!("{prefix}{}", dedupe_lines(text, 8).trim_end()),
        Mode::DiffAware => format!("{prefix}{}", diff_summary(text, 120).trim_end()),
        Mode::Auto => {
            if max_visible_tokens == 0 || raw_tokens <= max_visible_tokens {
                format!("{prefix}{}", text.trim_end())
            } else {
                summarize_lines(text, 18, 12, &prefix)
            }
        }
        Mode::Hybrid | Mode::Critical | Mode::Fidelity => unreachable!("legacy modes are mapped"),
    };
    let visible = if policy == Mode::Passthrough {
        visible
    } else {
        enforce_token_budget_with_ref(&visible, max_visible_tokens, recovery_ref)
    };
    let mut visible_tokens = count_tokens(&visible);
    // Adaptive floor: a capsule must never cost more than the raw text it
    // wraps. When framing overhead exceeds the savings (small payloads), fall
    // back to the raw text; exact refs still provide recovery either way.
    // Exact mode is excluded because hiding the payload is its contract.
    let raw_fits_budget = max_visible_tokens == 0 || raw_tokens <= max_visible_tokens;
    let visible = if policy != Mode::Exact && raw_fits_budget && visible_tokens > raw_tokens {
        let fallback = text.trim_end().to_string();
        let fallback_tokens = count_tokens(&fallback);
        if fallback_tokens < visible_tokens {
            visible_tokens = fallback_tokens;
            fallback
        } else {
            visible
        }
    } else {
        visible
    };
    Capsule {
        visible_tokens,
        raw_tokens,
        omitted_lines: line_count.saturating_sub(visible.lines().count()),
        text: visible,
        mode,
    }
}

pub fn summarize_lines(text: &str, head: usize, tail: usize, prefix: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= head + tail + 3 {
        return format!("{prefix}{}", text.trim_end());
    }
    let mut out = String::new();
    out.push_str(prefix);
    out.push_str(&lines[..head].join("\n"));
    out.push_str("\n\n... omitted ");
    out.push_str(&lines.len().saturating_sub(head + tail).to_string());
    out.push_str(" lines; exact ref available ...\n\n");
    out.push_str(&lines[lines.len() - tail..].join("\n"));
    out
}

fn capsule_prefix(label: Option<&str>, max_visible_tokens: usize, raw_tokens: usize) -> String {
    let Some(label) = label else {
        return String::new();
    };
    let full = format!("# {label}\n");
    if max_visible_tokens == 0 {
        return full;
    }
    let label_budget = max_visible_tokens.saturating_sub(raw_tokens).max(4);
    if count_tokens(&full) <= label_budget {
        return full;
    }
    let compact = compact_label(label);
    let compact_prefix = format!("# {compact}\n");
    if count_tokens(&compact_prefix) <= label_budget
        || count_tokens(&compact_prefix) < count_tokens(&full)
    {
        return compact_prefix;
    }
    "# source\n".to_string()
}

fn compact_label(label: &str) -> String {
    let path = Path::new(label);
    if label.contains('\\') || label.contains('/') {
        if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
            return format!(".../{name}");
        }
    }
    let mut chars = label.chars();
    let head = chars.by_ref().take(48).collect::<String>();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        label.to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandStatus {
    pub transport_status: String,
    pub command_success: bool,
    pub exit_code: Option<i32>,
    pub failed_segment: Option<String>,
    pub pipeline_masking_warning: Option<String>,
    pub pipeline_rerun_command: Option<String>,
    pub shell_syntax_summary: String,
    pub status_label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub policy: String,
    pub reason: String,
    pub family: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellRender {
    pub visible: String,
    pub policy: PolicyDecision,
    pub command_status: CommandStatus,
    pub diagnostics: Vec<Diagnostic>,
    pub omitted_lines: usize,
    pub output_strategy: String,
}

#[derive(Debug, Clone)]
pub struct ShellRenderInput<'a> {
    pub command: &'a str,
    pub stdout: &'a str,
    pub stderr: &'a str,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub mode: Mode,
    pub max_visible_tokens: usize,
    pub stdout_ref: Option<&'a str>,
    pub stderr_ref: Option<&'a str>,
    pub combined_ref: Option<&'a str>,
}

pub fn render_shell(input: ShellRenderInput<'_>) -> ShellRender {
    let status = classify_command_status(
        input.command,
        input.stdout,
        input.stderr,
        input.exit_code,
        input.timed_out,
    );
    let policy = decide_shell_policy(
        input.command,
        input.stdout,
        input.stderr,
        input.exit_code,
        input.mode,
    );
    let combined =
        shell_combined_output(input.command, input.exit_code, input.stdout, input.stderr);
    // Pre-compute line counts so we don't re-scan combined/visible later.
    let combined_line_count = combined.lines().count();
    let combined_tokens = count_tokens(&combined);
    let mut minimal_envelope = false;
    let mut success_compacted = false;
    let compact_passthrough = should_compact_tiny_shell(&input, &policy, &status);
    let compact_diagnostic =
        should_compact_short_failure_shell(&input, &policy, &status, &combined);
    let compact_inventory = should_compact_repo_inventory_shell(&input, &policy, &status);
    let body = if compact_passthrough {
        compact_shell_view(input.stdout)
    } else if compact_diagnostic {
        compact_diagnostic_shell_view(input.stdout, input.stderr)
    } else if compact_inventory {
        compact_repo_inventory_view(input.command, input.stdout)
    } else if policy.policy == "exact" || policy.policy == "passthrough" {
        // Move combined directly into body — no clone needed.
        combined
    } else {
        match policy.policy.as_str() {
            "diagnostic" => {
                diagnostic_shell_view(input.stdout, input.stderr, input.max_visible_tokens)
            }
            "structured" => structured_shell_view(input.command, input.stdout, input.stderr),
            "dedupe" => dedupe_lines_structural(&combined, 6),
            "diff-aware" => diff_summary(&combined, 160),
            _ => summarize_lines(&combined, 18, 12, ""),
        }
    };
    // Success-noise compaction: when a command verifiably succeeded and its
    // exact bytes are recoverable, replace boilerplate-heavy bodies with a
    // denser candidate. Candidates only win when strictly cheaper, and every
    // critical line (error/warning/assertion evidence) is kept verbatim, so
    // protected anchors survive. Failures, timeouts, masked pipelines, and
    // explicit non-auto modes never enter this branch.
    let body = if should_compact_success_noise(&input, &status)
        && !compact_passthrough
        && !compact_inventory
        && policy.policy != "exact"
    {
        let mut best = body;
        let mut best_tokens = count_tokens(&best);
        if let Some(view) = success_noise_view(input.command, input.stdout, input.stderr) {
            let view_tokens = count_tokens(&view);
            // Adopt when strictly cheaper, or — for diagnostic-policy success
            // bodies — when the family view sits far under raw cost: it keeps
            // whole critical blocks verbatim where radius-based context both
            // truncates anchors and retains adjacent noise.
            let diagnostic_headroom =
                policy.policy == "diagnostic" && view_tokens * 2 <= combined_tokens;
            if view_tokens < best_tokens || diagnostic_headroom {
                best = view;
                best_tokens = view_tokens;
                success_compacted = true;
            }
        }
        // Search/json/diff bodies are requested content, not success noise:
        // only boilerplate-prone policies get the token-aware squeeze.
        let squeezable = matches!(
            policy.policy.as_str(),
            "dedupe" | "passthrough" | "diagnostic"
        );
        if squeezable && best_tokens > shell_success_summary_budget(input.max_visible_tokens) {
            let squeezed = summarize_tokens(
                &best,
                shell_success_summary_budget(input.max_visible_tokens),
                "",
            );
            if count_tokens(&squeezed) < best_tokens {
                best = squeezed;
                success_compacted = true;
            }
        }
        best
    } else {
        body
    };
    let body = if policy.policy == "exact" || policy.policy == "passthrough" {
        body
    } else {
        mask_visible_secrets(&body)
    };
    let visible = if compact_passthrough {
        body
    } else if compact_diagnostic {
        let visible = compact_diagnostic_shell_capsule(&input, &status, &body);
        enforce_token_budget(&visible, input.max_visible_tokens)
    } else if compact_inventory {
        let visible = compact_repo_inventory_shell_capsule(&input, &body);
        enforce_token_budget(&visible, input.max_visible_tokens)
    } else {
        use std::fmt::Write as _;
        let display_command = if policy.policy == "exact" || policy.policy == "passthrough" {
            input.command.to_string()
        } else {
            mask_visible_secrets(input.command)
        };
        // Pre-size the header buffer so we don't re-alloc per push_str.
        let mut visible = String::with_capacity(256 + body.len());
        let _ = writeln!(visible, "# shell");
        let _ = writeln!(visible, "command: {display_command}");
        let _ = writeln!(visible, "policy: {} ({})", policy.policy, policy.reason);
        let _ = writeln!(visible, "status: {}", status.status_label);
        let _ = writeln!(visible, "command_success: {}", status.command_success);
        let _ = writeln!(
            visible,
            "exit_code: {}",
            status
                .exit_code
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string())
        );
        if let Some(segment) = status.failed_segment.as_deref() {
            let _ = writeln!(visible, "failed_segment: {segment}");
        }
        if let Some(warning) = status.pipeline_masking_warning.as_deref() {
            let _ = writeln!(visible, "pipeline_masking_warning: {warning}");
        }
        if let Some(command) = status.pipeline_rerun_command.as_deref() {
            let _ = writeln!(
                visible,
                "pipeline_rerun_command: {}",
                mask_visible_secrets(command)
            );
        }
        if let Some(stdout_ref) = input.stdout_ref {
            let _ = writeln!(visible, "stdout_ref: {stdout_ref}");
        }
        if let Some(stderr_ref) = input.stderr_ref {
            let _ = writeln!(visible, "stderr_ref: {stderr_ref}");
        }
        if let Some(combined_ref) = input.combined_ref {
            let _ = writeln!(visible, "combined_ref: {combined_ref}");
        }
        visible.push('\n');
        visible.push_str(body.trim_end());
        // Adaptive floor: when a small successful command's telemetry header
        // costs more than the raw output it frames, shrink to a header of
        // "# shell ok" plus the combined ref (protected recovery anchor).
        // Failures, timeouts, masked pipelines, and explicit render modes
        // keep the full diagnostics.
        let header_dominates = count_tokens(&visible) > combined_tokens;
        let minimal_eligible = input.mode.effective_policy() == Mode::Auto
            && status.command_success
            && !input.timed_out
            && status.failed_segment.is_none()
            && status.pipeline_masking_warning.is_none();
        if (header_dominates || success_compacted) && minimal_eligible {
            let mut minimal = String::with_capacity(64 + body.len());
            minimal.push_str("# shell ok");
            if let Some(combined_ref) = input.combined_ref {
                let _ = write!(minimal, "\ncombined_ref: {combined_ref}");
            }
            let trimmed_body = body.trim_end();
            if !trimmed_body.is_empty() {
                minimal.push('\n');
                minimal.push_str(trimmed_body);
            }
            if count_tokens(&minimal) < count_tokens(&visible) {
                minimal_envelope = true;
                visible = minimal;
            }
        }
        enforce_token_budget(&visible, input.max_visible_tokens)
    };
    let visible_line_count = visible.lines().count();
    ShellRender {
        omitted_lines: combined_line_count.saturating_sub(visible_line_count),
        visible,
        policy,
        command_status: status,
        diagnostics: Vec::new(),
        output_strategy: if compact_passthrough {
            "compact_adaptive_shell".to_string()
        } else if compact_diagnostic {
            "compact_diagnostic_shell".to_string()
        } else if compact_inventory {
            "compact_inventory_shell".to_string()
        } else if success_compacted {
            "compact_success_shell".to_string()
        } else if minimal_envelope {
            "minimal_envelope_shell".to_string()
        } else {
            "exact_first_adaptive_shell".to_string()
        },
    }
}

/// Render a shell result that recovery verified as a byte-identical repeat
/// of the previous run (same combined output, same exit code). Verified
/// successes compact to a delta envelope pointing at the content-addressed
/// ref; everything else — failures, timeouts, masked pipelines, explicit
/// modes, first runs, or envelopes that would not beat raw — delegates to
/// `render_shell` untouched. A repeated failure keeps repeating its full
/// evidence.
pub fn render_shell_repeat(input: ShellRenderInput<'_>, repeat_seen: u32) -> ShellRender {
    let status = classify_command_status(
        input.command,
        input.stdout,
        input.stderr,
        input.exit_code,
        input.timed_out,
    );
    let eligible = repeat_seen >= 2
        && input.mode.effective_policy() == Mode::Auto
        && status.command_success
        && !input.timed_out
        && status.failed_segment.is_none()
        && status.pipeline_masking_warning.is_none()
        && input.combined_ref.is_some();
    if !eligible {
        return render_shell(input);
    }
    let combined =
        shell_combined_output(input.command, input.exit_code, input.stdout, input.stderr);
    let combined_tokens = count_tokens(&combined);
    let mut visible = format!("# shell ok (unchanged; run {repeat_seen})");
    if let Some(combined_ref) = input.combined_ref {
        visible.push_str(&format!("\ncombined_ref: {combined_ref}"));
    }
    if count_tokens(&visible) >= combined_tokens {
        return render_shell(input);
    }
    let visible = enforce_token_budget(&visible, input.max_visible_tokens);
    let visible_line_count = visible.lines().count();
    ShellRender {
        omitted_lines: combined.lines().count().saturating_sub(visible_line_count),
        visible,
        policy: PolicyDecision {
            policy: "passthrough".to_string(),
            reason: "verified unchanged repeat".to_string(),
            family: shell_family(input.command, input.stdout, input.stderr),
        },
        command_status: status,
        diagnostics: Vec::new(),
        output_strategy: "repeat_unchanged_shell".to_string(),
    }
}

/// Continuation lines of a compiler/test diagnostic block: indented context,
/// gutter pipes, caret underlines, and note/help follow-ups.
fn is_critical_continuation_line(line: &str) -> bool {
    if line.starts_with(' ') || line.starts_with('\t') {
        return true;
    }
    let trimmed = line.trim_start();
    if trimmed.starts_with("-->")
        || trimmed.starts_with('|')
        || trimmed.starts_with('^')
        || trimmed.starts_with('=')
        || trimmed.starts_with("note:")
        || trimmed.starts_with("help:")
    {
        return true;
    }
    // Source gutter lines: "5 |     let x = 1;"
    trimmed.split_once(' ').is_some_and(|(num, rest)| {
        !num.is_empty()
            && num.chars().all(|c| c.is_ascii_digit())
            && rest.trim_start().starts_with('|')
    })
}

fn is_cargo_test_ok_line(trimmed: &str) -> bool {
    trimmed.starts_with("test ")
        && (trimmed.ends_with("... ok") || trimmed.ends_with("... ignored"))
        && !trimmed.contains("FAILED")
}

fn is_pytest_pass_marker(trimmed: &str) -> bool {
    trimmed.contains("::")
        && (trimmed.ends_with("PASSED")
            || trimmed.ends_with("XPASS")
            || trimmed.ends_with("SKIPPED"))
        && !trimmed.contains("FAILED")
        && !trimmed.contains("ERROR")
}

fn is_pytest_summary_line(trimmed: &str) -> bool {
    trimmed.starts_with("==")
        && trimmed.ends_with("==")
        && (trimmed.contains(" passed") || trimmed.contains(" skipped"))
        && !trimmed.contains(" failed")
        && !trimmed.contains(" error")
}

fn is_pytest_noise_line(trimmed: &str) -> bool {
    if trimmed.starts_with("platform ")
        || trimmed.starts_with("rootdir:")
        || trimmed.starts_with("configfile:")
        || trimmed.starts_with("cachedir:")
        || trimmed.starts_with("plugins:")
        || trimmed.starts_with("collected ")
        || trimmed.starts_with("collecting ")
        || (trimmed.starts_with("==")
            && trimmed.ends_with("==")
            && trimmed.contains("session starts"))
    {
        return true;
    }
    if (trimmed.contains("::") && (trimmed.ends_with("PASSED") || trimmed.contains(" PASSED ")))
        || trimmed.ends_with("XPASS")
        || trimmed.ends_with("SKIPPED")
    {
        return true;
    }
    let body = trimmed
        .strip_suffix(|c: char| c == ']')
        .map(|rest| rest.trim_end_matches(|c: char| c.is_ascii_digit() || c == '%' || c == '['))
        .unwrap_or(trimmed);
    !body.is_empty()
        && body
            .trim()
            .chars()
            .all(|c| matches!(c, '.' | 's' | 'x' | 'X'))
}

fn is_npm_summary_line(trimmed: &str) -> bool {
    trimmed.starts_with("added ")
        || trimmed.starts_with("removed ")
        || trimmed.starts_with("changed ")
        || trimmed.starts_with("audited ")
        || trimmed.starts_with("found 0 vulnerabilities")
        || trimmed.starts_with("up to date")
}

fn is_npm_noise_line(trimmed: &str) -> bool {
    trimmed.starts_with("npm http")
        || trimmed.starts_with("npm timing")
        || trimmed.starts_with("npm verb")
        || trimmed.starts_with("npm sill")
        || trimmed.starts_with("npm info")
        || trimmed.contains("packages are looking for funding")
        || trimmed.starts_with("run `npm fund`")
        || trimmed.starts_with("run \"npm fund\"")
}

fn git_progress_prefix(trimmed: &str) -> Option<&'static str> {
    [
        "remote: Enumerating objects",
        "remote: Counting objects",
        "remote: Compressing objects",
        "remote: Total",
        "Receiving objects",
        "Resolving deltas",
        "Counting objects",
        "Compressing objects",
        "Writing objects",
        "Unpacking objects",
    ]
    .into_iter()
    .find(|prefix| trimmed.starts_with(prefix))
}

/// Token-aware summarizer: spends `max_tokens` on the most information-dense
/// lines (critical evidence first, then numeric/path-rich lines, then head and
/// tail context) instead of a blind head/tail split. Critical lines are kept
/// even when they alone exceed the budget — anchor protection outranks the
/// soft budget. Gaps are marked with line counts so omissions stay explicit
/// and recoverable via the exact refs.
pub fn summarize_tokens(text: &str, max_tokens: usize, prefix: &str) -> String {
    if max_tokens == 0 {
        return format!("{prefix}{}", text.trim_end());
    }
    let lines: Vec<&str> = text.lines().collect();
    if count_tokens(text) <= max_tokens || lines.len() <= 4 {
        return format!("{prefix}{}", text.trim_end());
    }
    let n = lines.len();
    let line_tokens: Vec<usize> = lines.iter().map(|line| count_tokens(line)).collect();
    let mut order: Vec<usize> = (0..n).collect();
    let score = |idx: usize| -> u32 {
        let line = lines[idx];
        if looks_critical_line(line) {
            return 100;
        }
        let mut score = 0u32;
        if idx < 3 || idx + 3 >= n {
            score += 60;
        }
        if line_information_density(line) {
            score += 30;
        }
        if line.trim().is_empty() {
            return 0;
        }
        score.max(1)
    };
    let scores: Vec<u32> = (0..n).map(score).collect();
    order.sort_by(|a, b| scores[*b].cmp(&scores[*a]).then(a.cmp(b)));
    // "... +N lines; exact ref available ..." tokenizes to ~13 units.
    let gap_marker_tokens = 13usize;
    let mut selected = vec![false; n];
    let mut spent = 0usize;
    for &idx in &order {
        let cost = line_tokens[idx];
        if scores[idx] >= 100 {
            // Criticals are always kept; protection outranks the budget.
            selected[idx] = true;
            spent = spent.saturating_add(cost);
            continue;
        }
        if scores[idx] == 0 {
            continue;
        }
        if spent + cost + gap_marker_tokens > max_tokens {
            continue;
        }
        selected[idx] = true;
        spent += cost;
    }
    // Fill single-line gaps that cost no more than their marker would.
    for idx in 1..n.saturating_sub(1) {
        if !selected[idx]
            && selected[idx - 1]
            && selected[idx + 1]
            && line_tokens[idx] <= gap_marker_tokens
        {
            selected[idx] = true;
        }
    }
    if !selected.iter().any(|v| *v) {
        return summarize_lines(text, 8, 6, prefix);
    }
    let mut out = String::new();
    out.push_str(prefix);
    let mut omitted_run = 0usize;
    let mut first = true;
    for idx in 0..n {
        if selected[idx] {
            if omitted_run > 0 {
                if !first {
                    out.push('\n');
                }
                out.push_str(&format!(
                    "... +{omitted_run} lines; exact ref available ..."
                ));
                first = false;
                omitted_run = 0;
            }
            if !first {
                out.push('\n');
            }
            out.push_str(lines[idx]);
            first = false;
        } else {
            omitted_run += 1;
        }
    }
    if omitted_run > 0 {
        if !first {
            out.push('\n');
        }
        out.push_str(&format!(
            "... +{omitted_run} lines; exact ref available ..."
        ));
    }
    out
}

/// Heuristic: a line is information-dense when it carries identifiers tied to
/// concrete artifacts — paths, line references, numbers, or hashes.
fn line_information_density(line: &str) -> bool {
    let mut digits = 0usize;
    let mut path_chars = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        }
        if ch == '/' || ch == '\\' {
            path_chars += 1;
        }
    }
    digits >= 3 || path_chars >= 2 || line.contains(".rs:") || line.contains(".py:")
}

/// Shell-path dedupe: in addition to exact-run collapse, consecutive lines
/// that are identical after digit-normalization (timestamps, counters,
/// progress percentages) collapse into one representative plus a count.
/// Critical lines never collapse. Read-path `dedupe_lines` is unchanged.
fn dedupe_lines_structural(text: &str, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let normalized: Vec<String> = lines
        .iter()
        .map(|line| normalize_digit_runs(line))
        .collect();
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = lines[idx];
        if looks_critical_line(line) {
            out.push(line.to_string());
            idx += 1;
            continue;
        }
        let mut exact = 1usize;
        while idx + exact < lines.len() && lines[idx + exact] == line {
            exact += 1;
        }
        if exact >= 3 {
            out.push(line.to_string());
            out.push(format!("... repeated {} more times ...", exact - 1));
            idx += exact;
            continue;
        }
        let mut similar = 1usize;
        while idx + similar < lines.len()
            && !looks_critical_line(lines[idx + similar])
            && normalized[idx + similar] == normalized[idx]
        {
            similar += 1;
        }
        if similar >= 4 {
            out.push(line.to_string());
            out.push(format!(
                "... {} similar lines collapsed (digits vary); exact ref available ...",
                similar - 1
            ));
            idx += similar;
            continue;
        }
        out.push(line.to_string());
        idx += 1;
    }
    if out.len() > context * 2 + 20 {
        let omitted = out.len().saturating_sub(context * 2);
        let mut compact = out[..context].to_vec();
        compact.push(format!(
            "... omitted {omitted} lines; exact ref available ..."
        ));
        compact.extend_from_slice(&out[out.len() - context..]);
        compact.join("\n")
    } else {
        out.join("\n")
    }
}

fn normalize_digit_runs(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_digits = false;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(ch);
        }
    }
    out
}

fn should_compact_short_failure_shell(
    input: &ShellRenderInput<'_>,
    policy: &PolicyDecision,
    status: &CommandStatus,
    combined: &str,
) -> bool {
    let has_visible_diagnostic_output =
        !input.stdout.trim().is_empty() || !input.stderr.trim().is_empty();
    input.mode.effective_policy() == Mode::Auto
        && policy.policy == "diagnostic"
        && !status.command_success
        && input.exit_code.is_some()
        && !input.timed_out
        && input.combined_ref.is_some()
        && has_visible_diagnostic_output
        && !has_visible_secret_marker(combined)
        && !has_protected_failure_context(combined)
        && count_tokens(combined) <= 160
        && combined.lines().count() <= 20
        && (looks_diagnostic(combined) || status.failed_segment.is_some())
}

fn compact_diagnostic_shell_view(stdout: &str, stderr: &str) -> String {
    let mut failure_anchors = Vec::new();
    let mut critical = Vec::new();
    let mut fallback = Vec::new();
    for line in stdout.lines().chain(stderr.lines()) {
        let trimmed = line.trim();
        if trimmed.is_empty() || is_shell_diagnostic_boilerplate(trimmed) {
            continue;
        }
        if looks_failure_anchor_line(trimmed) {
            failure_anchors.push(trimmed.to_string());
        } else if looks_critical_line(trimmed) {
            critical.push(trimmed.to_string());
        } else if fallback.is_empty() {
            fallback.push(trimmed.to_string());
        }
        if failure_anchors.len() >= 3 {
            break;
        }
    }
    let kept = if !failure_anchors.is_empty() {
        failure_anchors
    } else if !critical.is_empty() {
        critical
    } else {
        fallback
    };
    kept.first()
        .cloned()
        .unwrap_or_else(|| "diagnostic output omitted; see combined_ref".to_string())
}

fn compact_diagnostic_shell_capsule(
    input: &ShellRenderInput<'_>,
    status: &CommandStatus,
    body: &str,
) -> String {
    let mut visible = String::new();
    visible.push_str("# shell\n");
    visible.push_str(&format!("status: {}\n", status.status_label));
    visible.push_str(&format!(
        "exit_code: {}\n",
        status
            .exit_code
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string())
    ));
    if let Some(segment) = status.failed_segment.as_deref() {
        visible.push_str(&format!(
            "failed_segment: {}\n",
            mask_visible_secrets(segment)
        ));
    }
    if let Some(warning) = status.pipeline_masking_warning.as_deref() {
        let warning = if warning.contains("mask") {
            "inspect combined_ref".to_string()
        } else {
            mask_visible_secrets(warning)
        };
        visible.push_str(&format!("pipeline_masking_warning: {warning}\n"));
    }
    if let Some(command) = status.pipeline_rerun_command.as_deref() {
        visible.push_str(&format!(
            "pipeline_rerun_command: {}\n",
            mask_visible_secrets(command)
        ));
    }
    if input.stderr.trim().is_empty() && !input.stdout.trim().is_empty() {
        if let Some(stdout_ref) = input.stdout_ref {
            visible.push_str(&format!("stdout_ref: {stdout_ref}\n"));
        }
    }
    if !input.stderr.trim().is_empty() {
        if let Some(stderr_ref) = input.stderr_ref {
            visible.push_str(&format!("stderr_ref: {stderr_ref}\n"));
        }
    }
    if let Some(combined_ref) = input.combined_ref {
        visible.push_str(&format!("combined_ref: {combined_ref}\n"));
    }
    visible.push('\n');
    visible.push_str(body.trim_end());
    visible
}

fn is_shell_diagnostic_boilerplate(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("+ CategoryInfo")
        || trimmed.starts_with("+ FullyQualifiedErrorId")
        || trimmed.starts_with("At line:")
        || trimmed.starts_with("+ ~")
        || trimmed.starts_with("~~~~")
}

fn looks_failure_anchor_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "failed",
        "failure",
        "panic",
        "traceback",
        "exception",
        "assertion",
        "not ok",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_visible_secret_marker(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "authorization:",
        "bearer ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_protected_failure_context(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("assertion failed")
        || (lower.contains("left:") && lower.contains("right:"))
        || (lower.contains("test ") && lower.contains("failed") && lower.contains(".rs:"))
        || lower.contains("traceback")
}

pub fn diff_summary(text: &str, max_lines: usize) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        if line.starts_with("diff --git")
            || line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("@@")
            || line.starts_with("rename ")
            || line.starts_with("deleted file")
            || line.starts_with("new file")
            || line.starts_with('+')
            || line.starts_with('-')
        {
            out.push(line);
        }
        if out.len() >= max_lines {
            break;
        }
    }
    if out.is_empty() {
        summarize_lines(text, 18, 12, "")
    } else {
        out.join("\n")
    }
}

pub fn dedupe_lines(text: &str, context: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = lines[idx];
        let mut count = 1usize;
        while idx + count < lines.len() && lines[idx + count] == line {
            count += 1;
        }
        if count >= 3 {
            out.push(line.to_string());
            out.push(format!("... repeated {} more times ...", count - 1));
        } else {
            for _ in 0..count {
                out.push(line.to_string());
            }
        }
        idx += count;
    }
    if out.len() > context * 2 + 20 {
        let omitted = out.len().saturating_sub(context * 2);
        let mut compact = out[..context].to_vec();
        compact.push(format!(
            "... omitted {omitted} lines; exact ref available ..."
        ));
        compact.extend_from_slice(&out[out.len() - context..]);
        compact.join("\n")
    } else {
        out.join("\n")
    }
}

pub fn mask_visible_secrets(text: &str) -> String {
    text.lines()
        .map(mask_secret_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mask_secret_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for key in ["token=", "password=", "secret=", "api_key=", "apikey="] {
        if let Some(pos) = lower.find(key) {
            let keep = &line[..pos + key.len()];
            return format!("{keep}[masked]");
        }
    }
    line.split_whitespace()
        .map(|word| {
            if word.starts_with("sk-")
                || word.starts_with("sk-proj-")
                || word.starts_with("ghp_")
                || word.starts_with("AKIA")
            {
                "[masked]".to_string()
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn critical_lines(text: &str, radius: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![false; lines.len()];
    for (idx, line) in lines.iter().enumerate() {
        if looks_critical_line(line) {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(lines.len());
            for slot in keep.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }
    }
    lines
        .iter()
        .zip(keep)
        .filter_map(|(line, keep)| keep.then_some(*line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn error_block(text: &str, radius: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let re = regex_like_error;
    let mut keep = vec![false; lines.len()];
    for (idx, line) in lines.iter().enumerate() {
        if re(line) {
            let start = idx.saturating_sub(radius);
            let end = (idx + radius + 1).min(lines.len());
            for slot in keep.iter_mut().take(end).skip(start) {
                *slot = true;
            }
        }
    }
    lines
        .iter()
        .zip(keep)
        .filter_map(|(line, keep)| keep.then_some(*line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn regex_like_error(line: &&str) -> bool {
    let lower = line.to_ascii_lowercase();
    [
        "error",
        "exception",
        "traceback",
        "failed",
        "assertion",
        "panic",
        "expected",
        "actual",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub fn line_range(text: &str, start: usize, end: usize) -> String {
    let start = start.max(1);
    let end = end.max(start);
    text.lines()
        .skip(start - 1)
        .take(end - start + 1)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn symbol_block(text: &str, symbol: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let Some(hit) = lines.iter().position(|line| contains_word(line, symbol)) else {
        return String::new();
    };
    let mut start = hit;
    while start > 0 && lines[start - 1].starts_with([' ', '\t']) {
        start -= 1;
    }
    let base_indent = leading_ws(lines[hit]);
    let mut end = hit + 1;
    while end < lines.len() {
        let line = lines[end];
        if !line.trim().is_empty() && leading_ws(line) <= base_indent && end > hit + 1 {
            break;
        }
        end += 1;
    }
    lines[start..end].join("\n")
}

fn contains_word(line: &str, symbol: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|word| word == symbol)
}

fn leading_ws(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

pub fn id_for(prefix: char, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(17);
    out.push(prefix);
    for &byte in digest.iter().take(8) {
        push_hex_byte(&mut out, byte);
    }
    out
}

pub fn detect_content_type(text: &str, path: Option<&Path>) -> ContentType {
    if let Some(path) = path {
        match path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or_default()
        {
            "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "cc" | "cpp"
            | "h" | "hpp" => return ContentType::Code,
            "json" => return ContentType::JsonConfig,
            "md" | "markdown" => return ContentType::Markdown,
            "diff" | "patch" => return ContentType::Diff,
            "log" => return ContentType::Logs,
            _ => {}
        }
    }
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        ContentType::JsonConfig
    } else if trimmed.starts_with("diff --git") || trimmed.starts_with("@@") {
        ContentType::Diff
    } else if text.contains("Traceback") || text.contains("FAILED") || text.contains("error:") {
        ContentType::ShellOutput
    } else {
        ContentType::Unknown
    }
}

pub fn ref_record(kind: &str, ref_id: String, bytes: usize) -> RefRecord {
    RefRecord {
        kind: kind.to_string(),
        ref_id,
        bytes,
        live: true,
    }
}

mod render;
mod shell_display;
mod shell_family;
mod shell_parse;
mod shell_policy;
mod tokens;

use render::domain::*;
use render::noise::*;
use shell_display::*;
use shell_parse::*;
use tokens::*;

pub use render::domain::{
    diagnostic_shell_view, is_repo_inventory_command, repo_inventory_view, structured_shell_view,
};
pub use shell_display::{
    shell_display_command_from_argv, shell_display_command_from_argv_for_platform,
};
pub use shell_family::shell_family;
pub use shell_policy::{classify_command_status, decide_shell_policy, shell_combined_output};
pub use tokens::{
    count_tokens, enforce_token_budget, enforce_token_budget_with_ref, savings_ratio, sha256_hex,
};

#[cfg(test)]
mod tests;
