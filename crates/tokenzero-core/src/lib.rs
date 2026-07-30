#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

pub const CLI_SCHEMA_VERSION: &str = "tokenzero.cli.v1";
pub const MCP_SCHEMA_VERSION: &str = "tokenzero.mcp.v1";
pub const INSTALL_SCHEMA_VERSION: &str = "tokenzero.install_plan.v1";
pub const PULSE_SCHEMA_VERSION: &str = "tokenzero.pulse.v1";

macro_rules! string_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident, $as_vis:vis as_str {
        $($(#[$variant_meta:meta])* $variant:ident => $text:literal),+ $(,)?
    }) => {
        $(#[$meta])*
        $vis enum $name { $($(#[$variant_meta])* $variant),+ }

        impl $name {
            $as_vis fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

string_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Mode, as_str {
        #[default] Auto => "auto",
        Passthrough => "passthrough",
        Diagnostic => "diagnostic",
        Structured => "structured",
        Dedupe => "dedupe",
        DiffAware => "diff-aware",
        Exact => "exact",
        Lossy => "lossy",
        Hybrid => "hybrid",
        Critical => "critical",
        Fidelity => "fidelity",
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const MAP: &[(&[&str], Mode)] = &[
            (&["auto", "hybrid"], Mode::Auto),
            (&["passthrough"], Mode::Passthrough),
            (&["diagnostic", "critical"], Mode::Diagnostic),
            (&["structured", "fidelity"], Mode::Structured),
            (&["dedupe"], Mode::Dedupe),
            (&["diff-aware", "diff_aware", "diffaware"], Mode::DiffAware),
            (&["exact"], Mode::Exact),
            (&["lossy"], Mode::Lossy),
        ];
        MAP.iter()
            .find(|(aliases, _)| aliases.contains(&s))
            .map(|(_, m)| *m)
            .ok_or_else(|| format!("unsupported mode: {s}"))
    }
}

impl Mode {
    pub fn effective_policy(self) -> Self {
        match self {
            Self::Hybrid => Self::Auto,
            Self::Critical => Self::Diagnostic,
            Self::Fidelity | Self::Lossy => Self::Structured,
            other => other,
        }
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum McpToolSurface, pub as_str {
        #[default] Classic => "mcp",
        CodeMode => "codemode",
    }
}

impl McpToolSurface {
    pub const ENV: &'static str = "TOKENZERO_MCP_TOOL_SURFACE";
}

impl std::str::FromStr for McpToolSurface {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s
            .trim()
            .to_ascii_lowercase()
            .replace(['_', ' '], "-")
            .as_str()
        {
            "" | "mcp" | "classic" | "aliases" | "full" => Ok(Self::Classic),
            "codemode" | "code-mode" => Ok(Self::CodeMode),
            other => Err(format!(
                "unsupported MCP launch mode '{other}'; use mcp or codemode"
            )),
        }
    }
}

string_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    pub enum ContentType, as_str {
        Code => "code",
        ShellOutput => "shell_output",
        SearchResult => "search_result",
        Tree => "tree",
        Diff => "diff",
        JsonConfig => "json_config",
        Markdown => "markdown",
        Logs => "logs",
        Unknown => "unknown",
    }
}

/// Returns true if `haystack` starts with any pipe-delimited prefix.
pub(crate) fn starts_with_any(h: &str, p: &str) -> bool {
    p.split('|').any(|n| h.starts_with(n))
}

/// Returns true if `haystack` ends with any pipe-delimited suffix.
pub(crate) fn ends_with_any(h: &str, p: &str) -> bool {
    p.split('|').any(|n| h.ends_with(n))
}

/// Returns true if `haystack` contains any pipe-delimited needle.
pub(crate) fn contains_any(h: &str, p: &str) -> bool {
    p.split('|').any(|n| h.contains(n))
}

/// Returns true if `haystack` contains any whitespace-delimited needle.
pub(crate) fn contains_any_ws(h: &str, n: &str) -> bool {
    n.split_whitespace().any(|w| h.contains(w))
}

pub(crate) fn is_one_of(value: &str, choices: &str) -> bool {
    choices.split_whitespace().any(|choice| value == choice)
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
    /// Output tokens billed at the tool boundary. Defaults preserve older records.
    #[serde(default)]
    pub billed_tokens: usize,
    /// Billed output tokens satisfied by the measured cache source.
    #[serde(default)]
    pub cached_tokens: usize,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolResponse {
    pub schema_version: String,
    pub status: String,
    pub tool: String,
    /// ACK/2 one-token class atom. Pure mutation success is silent (None).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ack: Option<String>,
    /// Expandable detail ref for the response body when one is available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_ref: Option<String>,
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
    /// vz89.11 output channel separation: present only when the harness opted
    /// in (TOKENZERO_CHANNEL_SEPARATION). Absent means byte-identical default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<ChannelSeparation>,
    /// Recovery receipt marking terminal (do-not-recompact) exact-byte
    /// recovery. Present only on expand-family responses that return stored
    /// bytes verbatim; adapters must not re-compact or re-summarize the
    /// visible body of a response carrying this receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RecoveryReceipt>,
}

/// Terminal-recovery marker for adapter compaction pipelines (yevj).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryReceipt {
    /// True when this response IS the recovered content: re-running it
    /// through compaction would destroy the bytes the agent paid to recover.
    pub terminal: bool,
    /// Adapter contract: never re-compact the visible body.
    pub do_not_recompact: bool,
    /// True when the visible body is byte-exact recovered content.
    pub exact_bytes: bool,
}

/// Machine-action channel separated from user-facing prose (hub vz89.11).
/// The harness renders `status_line` deterministically at zero model-output
/// cost; `user_message` stays null between tool calls and may carry one brief
/// final explanation at completion.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChannelSeparation {
    /// Machine-readable action atom (canonical op name, e.g. "read").
    pub action: String,
    /// Deterministic status line derivable from the operation + receipt.
    pub status_line: String,
    /// Nullable by contract: None serializes as an explicit null.
    pub user_message: Option<String>,
}

/// Env var opting a harness into channel-separated responses.
pub const CHANNEL_SEPARATION_ENV: &str = "TOKENZERO_CHANNEL_SEPARATION";

/// How much of the channel contract the harness opted into (vz89.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelMode {
    /// No channel block; responses stay byte-identical to the pre-gate contract.
    Off,
    /// Machine action + deterministic status line, `user_message` always null.
    /// The between-tool-calls mode: no model narration is paid for.
    Action,
    /// Action mode plus one brief receipt-derived `user_message` on a terminal
    /// envelope. Still zero model-output cost: the text comes from receipts.
    Terminal,
}

impl ChannelMode {
    pub fn from_env_value(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "on" | "true" | "yes" | "action" => Self::Action,
            "terminal" | "final" => Self::Terminal,
            _ => Self::Off,
        }
    }

    pub fn enabled(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether a terminal envelope may carry a receipt-derived user message.
    pub fn emits_user_message(self) -> bool {
        matches!(self, Self::Terminal)
    }
}

/// The channel mode the harness opted into. Default `Off`.
pub fn channel_mode() -> ChannelMode {
    std::env::var(CHANNEL_SEPARATION_ENV)
        .map(|raw| ChannelMode::from_env_value(&raw))
        .unwrap_or(ChannelMode::Off)
}

/// Whether the harness opted into channel separation (vz89.11). Default off:
/// responses are byte-identical to the pre-gate contract.
pub fn channel_separation_enabled() -> bool {
    channel_mode().enabled()
}

impl ToolResponse {
    fn base(status: &str, tool: impl Into<String>) -> Self {
        Self {
            schema_version: CLI_SCHEMA_VERSION.to_string(),
            status: status.to_string(),
            tool: tool.into(),
            ..Self::default()
        }
    }

    pub fn ok(
        tool: impl Into<String>,
        mode: Mode,
        visible: String,
        refs: Vec<RefRecord>,
        accounting: Accounting,
    ) -> Self {
        Self {
            ack: Some(AckClass::Success.atom().to_string()),
            detail_ref: refs.first().map(|record| record.ref_id.clone()),
            mode: Some(mode.to_string()),
            visible: Some(Visible {
                kind: "capsule".to_string(),
                text: visible,
            }),
            refs,
            accounting: Some(accounting),
            ..Self::base("ok", tool)
        }
    }

    pub fn error(
        tool: impl Into<String>,
        code: impl Into<String>,
        message: impl Into<String>,
        repair: Option<String>,
    ) -> Self {
        let code = code.into();
        let ack = AckClass::from_error_kind(&code, false).atom().to_string();
        Self {
            ack: Some(ack),
            error: Some(CliError {
                code,
                message: message.into(),
                repair,
            }),
            ..Self::base("error", tool)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LossySpan {
    pub description: String,
    pub reason: String,
    pub recovery_may_be_needed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capsule {
    pub text: String,
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub omitted_lines: usize,
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_anchors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lossy_spans: Vec<LossySpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lossy_policy_id: Option<String>,
}

impl Capsule {
    /// Enforce RACC omission rule: transformed bytes require an exact selector,
    /// a visible protected anchor, or an explicit lossy declaration.
    pub fn validate_omission_rule(&self, original: &str) -> Result<(), String> {
        let original = original.trim_end();
        if original.is_empty() || self.text.contains(original) {
            return Ok(());
        }
        if self
            .protected_anchors
            .iter()
            .any(|anchor| !anchor.is_empty() && self.text.contains(&format!("[[anchor:{anchor}]]")))
        {
            return Ok(());
        }
        if self
            .exact_refs
            .iter()
            .any(|reference| exact_ref_has_selector(reference) && self.text.contains(reference))
        {
            return Ok(());
        }
        let lossy_declared = self.mode == Mode::Lossy
            && self
                .lossy_policy_id
                .as_ref()
                .is_some_and(|id| !id.is_empty())
            && !self.lossy_spans.is_empty()
            && self.lossy_spans.iter().all(|span| {
                !span.description.is_empty()
                    && !span.reason.is_empty()
                    && span.recovery_may_be_needed
            })
            && self.text.contains("mode=lossy")
            && self.text.contains("lossy_policy_id=");
        lossy_declared.then_some(()).ok_or_else(|| {
            "capsule omitted bytes without a protected anchor, exact tz:// selector, or explicit lossy declaration".to_string()
        })
    }
}

fn exact_ref_has_selector(reference: &str) -> bool {
    let Some((base, selector)) = reference.split_once('#') else {
        return false;
    };
    if !base.starts_with("tz://") || selector.is_empty() {
        return false;
    }
    if let Some(bytes) = selector.strip_prefix('B') {
        return bytes.split_once('-').is_some_and(|(start, len)| {
            start.parse::<usize>().is_ok() && len.parse::<usize>().is_ok()
        });
    }
    if let Some(lines) = selector.strip_prefix('L') {
        return lines.split_once("-L").is_some_and(|(start, end)| {
            start.parse::<usize>().is_ok() && end.parse::<usize>().is_ok()
        });
    }
    selector
        .strip_prefix("symbol=")
        .is_some_and(|symbol| !symbol.is_empty())
}

fn exact_recovery_ref(reference: &str, byte_len: usize) -> Option<String> {
    reference.starts_with("tz://").then(|| {
        if reference.contains('#') {
            reference.to_string()
        } else {
            format!("{reference}#B0-{byte_len}")
        }
    })
}

fn finalize_capsule_omission(
    mut capsule: Capsule,
    original: &str,
    max_visible_tokens: usize,
    exact_ref: Option<String>,
) -> Capsule {
    let original_trimmed = original.trim_end();
    let omitted = !original_trimmed.is_empty() && !capsule.text.contains(original_trimmed);
    if omitted {
        if let Some(reference) = exact_ref.filter(|value| exact_ref_has_selector(value)) {
            // validate_omission_rule requires the selector to be present in the
            // VISIBLE TEXT, not merely recorded in exact_refs: a ref an agent
            // cannot see is a ref it cannot expand, so recording it alone would
            // satisfy the struct while still stranding the omitted bytes.
            // Without this the branch panicked on any budgeted read whose
            // enforce_token_budget_with_ref marker had already been trimmed.
            if !capsule.text.contains(&reference) {
                capsule.text.push('\n');
                capsule.text.push_str(&format!(
                    "... omitted by visible budget; expand {reference} for the full output ..."
                ));
                capsule.visible_tokens = count_tokens(&capsule.text);
            }
            capsule.exact_refs.push(reference);
        } else {
            let mut declared = capsule.text.clone();
            if !declared.contains("mode=lossy") {
                declared.push('\n');
                declared.push_str(VISIBLE_BUDGET_LOSSY_DECLARATION);
            }
            let declared_tokens = count_tokens(&declared);
            let raw_full_tokens = count_tokens(original_trimmed);
            if declared_tokens >= raw_full_tokens && capsule.mode != Mode::Exact {
                // Inflation guard: with no exact ref to point at, the lossy
                // declaration plus summary can cost more tokens than the raw
                // bytes it replaces (tiny inputs / budgets). Emit the raw text
                // instead: nothing is omitted, no declaration is required, the
                // visible cost never exceeds the raw cost, and the decision is
                // budget-independent so visible cost stays monotone in budget.
                // Exact mode is exempt: its whole point is hiding the payload,
                // so falling back to raw text would break that contract.
                capsule.text = original_trimmed.to_string();
                capsule.visible_tokens = raw_full_tokens;
                capsule.omitted_lines = 0;
            } else {
                capsule.mode = Mode::Lossy;
                capsule.lossy_policy_id = Some("tokenzero.visible-compression.v1".to_string());
                capsule.lossy_spans.push(LossySpan {
                    description: "bytes omitted from the visible capsule".to_string(),
                    reason: "visible token budget or selected compression policy".to_string(),
                    recovery_may_be_needed: true,
                });
                capsule.text = enforce_token_budget_with_ref(&declared, max_visible_tokens, None);
                capsule.visible_tokens = count_tokens(&capsule.text);
            }
        }
    }
    capsule
        .validate_omission_rule(original)
        .expect("capsule emission violated the omission rule");
    capsule
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

/// Adds an inline exact-ref recovery cue to a token-budgeted capsule.
pub fn make_capsule_with_recovery_ref(
    text: &str,
    raw_tokens: usize,
    mode: Mode,
    max_tokens: usize,
    label: Option<&str>,
    recovery_ref: Option<&str>,
) -> Capsule {
    let prefix = capsule_prefix(label, max_tokens, raw_tokens);
    let exact_ref = recovery_ref.and_then(|reference| exact_recovery_ref(reference, text.len()));
    let policy = mode.effective_policy();
    let mut visible = match policy {
        Mode::Exact => format!("{prefix}[exact payload stored; use expand for raw bytes]"),
        Mode::Passthrough => format!("{prefix}{}", text.trim_end()),
        Mode::Diagnostic => match error_block(text, 3) {
            b if b.trim().is_empty() => summarize_lines(text, 8, 6, &prefix),
            b => format!("{prefix}{}", b.trim_end()),
        },
        Mode::Structured => summarize_lines(text, 24, 16, &prefix),
        Mode::Dedupe => format!("{prefix}{}", dedupe_lines(text, 8).trim_end()),
        Mode::DiffAware => format!("{prefix}{}", diff_summary(text, 120).trim_end()),
        Mode::Auto if max_tokens == 0 || raw_tokens <= max_tokens => {
            format!("{prefix}{}", text.trim_end())
        }
        Mode::Auto => summarize_lines(text, 18, 12, &prefix),
        _ => unreachable!(),
    };
    if policy != Mode::Passthrough {
        visible = enforce_token_budget_with_ref(&visible, max_tokens, exact_ref.as_deref());
    }
    let mut visible_tokens = count_tokens(&visible);
    if policy != Mode::Exact
        && (max_tokens == 0 || raw_tokens <= max_tokens)
        && visible_tokens > raw_tokens
    {
        let fallback = text.trim_end().to_string();
        let fallback_tokens = count_tokens(&fallback);
        if fallback_tokens < visible_tokens {
            visible_tokens = fallback_tokens;
            visible = fallback;
        }
    }
    finalize_capsule_omission(
        Capsule {
            visible_tokens,
            raw_tokens,
            omitted_lines: text.lines().count().saturating_sub(visible.lines().count()),
            text: visible,
            mode,
            protected_anchors: Vec::new(),
            exact_refs: Vec::new(),
            lossy_spans: Vec::new(),
            lossy_policy_id: None,
        },
        text,
        max_tokens,
        exact_ref,
    )
}

/// Creates a domain-aware summary with byte-exact recovery via `recovery_ref`.
pub fn make_capsule_content_aware(
    text: &str,
    raw_tokens: usize,
    content_type: ContentType,
    max_visible_tokens: usize,
    label: Option<&str>,
    recovery_ref: Option<&str>,
    aggressive: bool,
) -> Capsule {
    if !aggressive && (max_visible_tokens == 0 || raw_tokens <= max_visible_tokens) {
        return make_capsule_with_recovery_ref(
            text,
            raw_tokens,
            Mode::Auto,
            max_visible_tokens,
            label,
            recovery_ref,
        );
    }
    let prefix = capsule_prefix(label, max_visible_tokens, raw_tokens);
    let exact_ref = recovery_ref.and_then(|reference| exact_recovery_ref(reference, text.len()));
    let budget = if aggressive {
        max_visible_tokens / 3
    } else {
        max_visible_tokens
    };
    let visible = match content_type {
        ContentType::Code => summarize_code(text, budget, &prefix),
        ContentType::Logs | ContentType::ShellOutput => summarize_logs(text, budget, &prefix),
        ContentType::JsonConfig => summarize_json(text, budget, &prefix),
        ContentType::Diff => summarize_lines(text, 12, 8, &prefix),
        ContentType::SearchResult => summarize_lines(text, 20, 5, &prefix),
        _ => summarize_lines(text, 18, 12, &prefix),
    };
    let visible = enforce_token_budget_with_ref(&visible, max_visible_tokens, exact_ref.as_deref());
    let visible_tokens = count_tokens(&visible);
    finalize_capsule_omission(
        Capsule {
            omitted_lines: text.lines().count().saturating_sub(visible.lines().count()),
            text: visible,
            raw_tokens,
            visible_tokens,
            mode: if aggressive { Mode::Exact } else { Mode::Auto },
            protected_anchors: Vec::new(),
            exact_refs: Vec::new(),
            lossy_spans: Vec::new(),
            lossy_policy_id: None,
        },
        text,
        max_visible_tokens,
        exact_ref,
    )
}

/// Summarize code: show first N lines (imports/signatures) + last M lines.
const CODE_SIG_PREFIXES: &str =
    "pub |fn |struct |enum |impl |trait |class |def |function |export |import |use |#[";

fn push_labeled_lines(out: &mut String, label: &str, lines: &[&str], limit: usize) {
    if lines.is_empty() {
        return;
    }
    out.push_str(label);
    for line in lines.iter().take(limit) {
        out.push_str(line);
        out.push('\n');
    }
}

fn summarize_code(text: &str, budget_tokens: usize, prefix: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let total = lines.len();
    if total <= 30 {
        return format!("{prefix}{}", text.trim_end());
    }
    let sigs: Vec<&str> = lines
        .iter()
        .take(total.min(80))
        .filter(|l| starts_with_any(l.trim(), CODE_SIG_PREFIXES))
        .copied()
        .collect();
    let head = 8.min(total);
    let tail = 6.min(total.saturating_sub(head));
    let mut out = format!("{prefix}{}", lines[..head].join("\n"));
    push_labeled_lines(
        &mut out,
        "\n\n# declarations/signatures:\n",
        &sigs,
        budget_tokens / 8,
    );
    out.push_str(&omitted_lines_marker(total.saturating_sub(head + tail)));
    out + &lines[total - tail..].join("\n")
}

/// Summarize logs: prioritize errors/warnings, then head+tail.
const LOG_ERROR_NEEDLES: &str = "error fatal panic failed traceback";

fn summarize_logs(text: &str, budget_tokens: usize, prefix: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let (mut errs, mut warns) = (Vec::new(), Vec::new());
    for l in &lines {
        let low = l.to_ascii_lowercase();
        if contains_any_ws(&low, LOG_ERROR_NEEDLES) {
            errs.push(*l);
        } else if low.contains("warn") {
            warns.push(*l);
        }
    }
    let mut out = prefix.to_string();
    let limit = budget_tokens / 6;
    push_labeled_lines(
        &mut out,
        &format!("# {} error(s):\n", errs.len()),
        &errs,
        limit,
    );
    push_labeled_lines(
        &mut out,
        &format!("# {} warning(s):\n", warns.len()),
        &warns,
        limit / 2,
    );
    if errs.is_empty() && warns.is_empty() {
        let head = 6.min(lines.len());
        let tail = 4.min(lines.len().saturating_sub(head));
        out.push_str(&lines[..head].join("\n"));
        if lines.len() > head + tail {
            out.push_str(&format!(
                "\n... omitted {} lines ...\n",
                lines.len().saturating_sub(head + tail)
            ));
        }
        if tail > 0 {
            out.push_str(&lines[lines.len() - tail..].join("\n"));
        }
    } else {
        out.push_str(&format!(
            "# {} total lines; exact ref available",
            lines.len()
        ));
    }
    out
}

/// Summarize JSON: show schema shape (keys, types, array lengths).
fn summarize_json(text: &str, _budget_tokens: usize, prefix: &str) -> String {
    let mut out = prefix.to_string();
    match serde_json::from_str::<serde_json::Value>(text.trim()) {
        Ok(serde_json::Value::Object(map)) => {
            out.push_str(&format!("json_object: {} keys\n", map.len()));
            for (key, val) in map.iter().take(25) {
                let kind = match val {
                    serde_json::Value::String(s) if s.len() > 100 => "string(long)",
                    serde_json::Value::Array(a) if a.is_empty() => "array(0)",
                    serde_json::Value::Object(o) if o.is_empty() => "object(0)",
                    other => json_kind(other),
                };
                out.push_str(&format!("  {key}: {kind}\n"));
            }
        }
        Ok(serde_json::Value::Array(items)) => {
            out.push_str(&format!("json_array: {} items\n", items.len()));
            if let Some(first) = items.first() {
                let sample: String = serde_json::to_string(first)
                    .unwrap_or_default()
                    .chars()
                    .take(200)
                    .collect();
                out.push_str(&format!("  sample: {sample}\n"));
            }
        }
        _ => return summarize_lines(text, 12, 8, prefix),
    }
    out + "# exact ref available for full content"
}

pub fn summarize_lines(text: &str, head: usize, tail: usize, prefix: &str) -> String {
    let lines: Vec<_> = text.lines().collect();
    if lines.len() <= head + tail + 3 {
        return format!("{prefix}{}", text.trim_end());
    }
    format!(
        "{prefix}{}\n\n... omitted {} lines; exact ref available ...\n\n{}",
        lines[..head].join("\n"),
        lines.len().saturating_sub(head + tail),
        lines[lines.len() - tail..].join("\n"),
    )
}

fn capsule_prefix(label: Option<&str>, max_visible_tokens: usize, raw_tokens: usize) -> String {
    let Some(label) = label else {
        return String::new();
    };
    let full = format!("# {label}\n");
    if max_visible_tokens == 0 {
        return full;
    }
    let budget = max_visible_tokens.saturating_sub(raw_tokens).max(4);
    if count_tokens(&full) <= budget {
        return full;
    }
    let compact = format!("# {}\n", compact_label(label));
    if count_tokens(&compact) <= budget || count_tokens(&compact) < count_tokens(&full) {
        compact
    } else {
        "# source\n".to_string()
    }
}

fn compact_label(label: &str) -> String {
    if label.contains(['\\', '/']) {
        if let Some(name) = Path::new(label).file_name().and_then(|name| name.to_str()) {
            return format!(".../{name}");
        }
    }
    let mut chars = label.chars();
    let head: String = chars.by_ref().take(48).collect();
    chars
        .next()
        .map_or_else(|| label.to_string(), |_| format!("{head}..."))
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

fn shell_input_status(input: &ShellRenderInput<'_>) -> CommandStatus {
    classify_command_status(
        input.command,
        input.stdout,
        input.stderr,
        input.exit_code,
        input.timed_out,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellViewCase {
    CompactTiny,
    CompactDiagnostic,
    CompactInventory,
    PolicyBased,
}

struct ShellRenderContext<'a> {
    policy: &'a PolicyDecision,
    status: &'a CommandStatus,
    combined: &'a str,
    combined_tokens: usize,
    max_tokens: usize,
}

/// Token count of the full shell input against which a rendered diagnostic is
/// measured. Stream recovery bytes remain unchanged and separately referenced.
pub fn shell_raw_tokens(
    command: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> usize {
    count_tokens(&shell_policy::shell_raw_accounting_output(
        command, exit_code, stdout, stderr,
    ))
}

pub fn render_shell(input: ShellRenderInput<'_>) -> ShellRender {
    let status = shell_input_status(&input);
    let policy = decide_shell_policy(
        input.command,
        input.stdout,
        input.stderr,
        input.exit_code,
        input.mode,
    );
    let combined = shell_policy::shell_stream_output(input.exit_code, input.stdout, input.stderr);
    let combined_line_count = combined.lines().count();
    let combined_tokens =
        shell_raw_tokens(input.command, input.exit_code, input.stdout, input.stderr);
    let (mut minimal_envelope, mut success_compacted) = (false, false);
    let max_t = input.max_visible_tokens;
    let cp = should_compact_tiny_shell(&input, &policy, &status);
    let cd = should_compact_short_failure_shell(&input, &policy, &status, &combined);
    let ci = should_compact_repo_inventory_shell(&input, &policy, &status);
    let case = if cp {
        ShellViewCase::CompactTiny
    } else if cd {
        ShellViewCase::CompactDiagnostic
    } else if ci {
        ShellViewCase::CompactInventory
    } else {
        ShellViewCase::PolicyBased
    };
    let context = ShellRenderContext {
        policy: &policy,
        status: &status,
        combined: &combined,
        combined_tokens,
        max_tokens: max_t,
    };
    let body = build_shell_body(case, &input, &context, &mut success_compacted);
    let visible = finalize_shell_visible(
        case,
        &input,
        &context,
        &body,
        success_compacted,
        &mut minimal_envelope,
    );
    ShellRender {
        omitted_lines: combined_line_count.saturating_sub(visible.lines().count()),
        visible,
        policy,
        command_status: status,
        diagnostics: Vec::new(),
        output_strategy: shell_output_strategy(cp, cd, ci, success_compacted, minimal_envelope)
            .to_string(),
    }
}

fn build_shell_body(
    case: ShellViewCase,
    input: &ShellRenderInput<'_>,
    context: &ShellRenderContext<'_>,
    success_compacted: &mut bool,
) -> String {
    let ShellRenderContext {
        policy,
        status,
        combined,
        combined_tokens,
        max_tokens,
    } = context;
    let (combined_tokens, max_tokens) = (*combined_tokens, *max_tokens);
    match case {
        ShellViewCase::CompactTiny => compact_shell_view(input.stdout),
        ShellViewCase::CompactDiagnostic => {
            compact_diagnostic_shell_view(input.stdout, input.stderr)
        }
        ShellViewCase::CompactInventory => compact_repo_inventory_view(input.command, input.stdout),
        ShellViewCase::PolicyBased => {
            let mut body = if matches!(policy.policy.as_str(), "exact" | "passthrough") {
                combined.to_string()
            } else {
                match policy.policy.as_str() {
                    "diagnostic"
                        if input.exit_code == Some(0)
                            && status.pipeline_masking_warning.is_some() =>
                    {
                        diagnostic_shell_view_with_tail(input.stdout, input.stderr, max_tokens)
                    }
                    "diagnostic" => diagnostic_shell_view(input.stdout, input.stderr, max_tokens),
                    "structured" => {
                        structured_shell_view(input.command, input.stdout, input.stderr)
                    }
                    "dedupe" => dedupe_lines_impl(combined, 6, true),
                    "diff-aware" => diff_summary(combined, 160),
                    _ => summarize_lines(combined, 18, 12, ""),
                }
            };
            if should_compact_success_noise(input, status) && policy.policy != "exact" {
                let mut best_tokens = count_tokens(&body);
                if let Some(view) = success_noise_view(input.command, input.stdout, input.stderr) {
                    let view_tokens = count_tokens(&view);
                    if view_tokens < best_tokens
                        || (policy.policy == "diagnostic" && view_tokens * 2 <= combined_tokens)
                    {
                        body = view;
                        best_tokens = view_tokens;
                        *success_compacted = true;
                    }
                }
                if matches!(
                    policy.policy.as_str(),
                    "dedupe" | "passthrough" | "diagnostic"
                ) && best_tokens > shell_success_summary_budget(max_tokens)
                {
                    let squeezed =
                        summarize_tokens(&body, shell_success_summary_budget(max_tokens), "");
                    if count_tokens(&squeezed) < best_tokens {
                        body = squeezed;
                        *success_compacted = true;
                    }
                }
            }
            if policy.policy != "exact" && policy.policy != "passthrough" {
                body = mask_visible_secrets(&body);
            }
            body
        }
    }
}

fn finalize_shell_visible(
    case: ShellViewCase,
    input: &ShellRenderInput<'_>,
    context: &ShellRenderContext<'_>,
    body: &str,
    success_compacted: bool,
    minimal_envelope: &mut bool,
) -> String {
    let ShellRenderContext {
        policy,
        status,
        combined_tokens,
        max_tokens,
        ..
    } = context;
    let (combined_tokens, max_tokens) = (*combined_tokens, *max_tokens);
    match case {
        ShellViewCase::CompactTiny => body.to_string(),
        ShellViewCase::CompactDiagnostic => enforce_token_budget(
            &compact_diagnostic_shell_capsule(input, status, body),
            max_tokens,
        ),
        ShellViewCase::CompactInventory => enforce_token_budget(
            &compact_repo_inventory_shell_capsule(input, body),
            max_tokens,
        ),
        ShellViewCase::PolicyBased => {
            let mut vis = format_shell_status_header(input, policy, status, body);
            if (count_tokens(&vis) > combined_tokens || success_compacted)
                && safe_auto_success(input, status)
            {
                let minimal = format_minimal_shell_ok(input.combined_ref, body);
                if count_tokens(&minimal) < count_tokens(&vis) {
                    *minimal_envelope = true;
                    vis = minimal;
                }
            }
            enforce_token_budget(&vis, max_tokens)
        }
    }
}
fn shell_output_strategy(cp: bool, cd: bool, ci: bool, sc: bool, me: bool) -> &'static str {
    [
        (cp, "compact_adaptive_shell"),
        (cd, "compact_diagnostic_shell"),
        (ci, "compact_inventory_shell"),
        (sc, "compact_success_shell"),
        (me, "minimal_envelope_shell"),
    ]
    .into_iter()
    .find_map(|(active, strategy)| active.then_some(strategy))
    .unwrap_or("exact_first_adaptive_shell")
}

fn push_shell_kv(out: &mut String, k: &str, v: &str) {
    out.push_str(&format!("{k}: {v}\n"));
}

fn push_optional_shell_kv(out: &mut String, k: &str, v: Option<&str>) {
    if let Some(val) = v {
        push_shell_kv(out, k, val);
    }
}

fn push_shell_status(out: &mut String, status: &CommandStatus, compact: bool) {
    push_shell_kv(
        out,
        "exit_code",
        &status
            .exit_code
            .map_or("null".to_string(), |v| v.to_string()),
    );
    for (key, value, always_mask) in [
        ("failed_segment", status.failed_segment.as_deref(), false),
        (
            "pipeline_masking_warning",
            status.pipeline_masking_warning.as_deref(),
            false,
        ),
        (
            "pipeline_rerun_command",
            status.pipeline_rerun_command.as_deref(),
            true,
        ),
    ] {
        let Some(value) = value else { continue };
        let value = if compact && key == "pipeline_masking_warning" && value.contains("mask") {
            "inspect combined_ref".to_string()
        } else if compact || always_mask {
            mask_visible_secrets(value)
        } else {
            value.to_string()
        };
        push_shell_kv(out, key, &value);
    }
}

fn format_shell_status_header(
    input: &ShellRenderInput<'_>,
    policy: &PolicyDecision,
    status: &CommandStatus,
    body: &str,
) -> String {
    let cmd = if matches!(policy.policy.as_str(), "exact" | "passthrough") {
        input.command.to_string()
    } else {
        mask_visible_secrets(input.command)
    };
    let mut vis = "# shell\n".to_string();
    push_shell_kv(&mut vis, "command", &cmd);
    vis.push_str(&format!("policy: {} ({})\n", policy.policy, policy.reason));
    push_shell_kv(&mut vis, "status", &status.status_label);
    vis.push_str(&format!("command_success: {}\n", status.command_success));
    push_shell_status(&mut vis, status, false);
    // The combined payload is the single primary recovery anchor. Stream and
    // capture refs remain machine-visible in ToolResponse::refs, but repeating
    // them in the capsule made one shell action mint up to four visible refs.
    push_optional_shell_kv(&mut vis, "combined_ref", input.combined_ref);
    vis + "\n" + body.trim_end()
}

fn format_minimal_shell_ok(combined_ref: Option<&str>, body: &str) -> String {
    let mut min = "# shell ok".to_string();
    if let Some(r) = combined_ref {
        min += &format!("\ncombined_ref: {r}");
    }
    let trimmed = body.trim_end();
    if !trimmed.is_empty() {
        min = min + "\n" + trimmed;
    }
    min
}

/// Compacts verified byte-identical successful repeats; all other runs render normally.
pub fn render_shell_repeat(input: ShellRenderInput<'_>, repeat_seen: u32) -> ShellRender {
    let status = shell_input_status(&input);
    if repeat_seen >= 2 && safe_auto_success(&input, &status) && input.combined_ref.is_some() {
        let combined =
            shell_policy::shell_stream_output(input.exit_code, input.stdout, input.stderr);
        let raw_tokens =
            shell_raw_tokens(input.command, input.exit_code, input.stdout, input.stderr);
        let mut visible = format!("# shell ok (unchanged; run {repeat_seen})");
        if let Some(r) = input.combined_ref {
            visible += &format!("\ncombined_ref: {r}");
        }
        if count_tokens(&visible) < raw_tokens {
            let visible = enforce_token_budget(&visible, input.max_visible_tokens);
            return ShellRender {
                omitted_lines: combined
                    .lines()
                    .count()
                    .saturating_sub(visible.lines().count()),
                visible,
                policy: PolicyDecision {
                    policy: "passthrough".to_string(),
                    reason: "verified unchanged repeat".to_string(),
                    family: shell_family(input.command, input.stdout, input.stderr),
                },
                command_status: status,
                diagnostics: Vec::new(),
                output_strategy: "repeat_unchanged_shell".to_string(),
            };
        }
    }
    render_shell(input)
}

/// Recognizes compiler/test diagnostic continuation lines.
fn is_critical_continuation_line(line: &str) -> bool {
    if line.starts_with([' ', '\t']) {
        return true;
    }
    let t = line.trim_start();
    let is_num = |n: &str| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    starts_with_any(t, "-->|^|=|note:|help:")
        || t.split_once(' ')
            .is_some_and(|(n, r)| is_num(n) && r.trim_start().starts_with('|'))
}

const CARGO_TEST_OK_SUFFIXES: &str = "... ok|... ignored";

fn is_cargo_test_ok_line(trimmed: &str) -> bool {
    trimmed.starts_with("test ")
        && ends_with_any(trimmed, CARGO_TEST_OK_SUFFIXES)
        && !trimmed.contains("FAILED")
}

fn is_pytest_pass_marker(trimmed: &str) -> bool {
    trimmed.contains("::")
        && ends_with_any(trimmed, "PASSED|XPASS|SKIPPED")
        && !contains_any(trimmed, "FAILED|ERROR")
}

fn is_pytest_summary_line(trimmed: &str) -> bool {
    trimmed.starts_with("==")
        && trimmed.ends_with("==")
        && contains_any(trimmed, " passed| skipped")
        && !contains_any(trimmed, " failed| error")
}

const PYTEST_NOISE_PREFIXES: &str =
    "platform |rootdir:|configfile:|cachedir:|plugins:|collected |collecting ";

fn is_pytest_noise_line(t: &str) -> bool {
    starts_with_any(t, PYTEST_NOISE_PREFIXES)
        || (!t.is_empty() && t.chars().all(|c| matches!(c, '.' | 's' | 'x' | 'X')))
        || (t.starts_with("==") && t.ends_with("==") && t.contains("session starts"))
        || (t.contains("::") && (ends_with_any(t, "PASSED|SKIPPED") || t.contains(" PASSED ")))
        || ends_with_any(t, "XPASS|SKIPPED")
        || t.strip_suffix(']')
            .map(|s| s.trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, '%' | '[')))
            .is_some_and(|b| {
                !b.is_empty() && b.trim().chars().all(|c| matches!(c, '.' | 's' | 'x' | 'X'))
            })
}

const NPM_SUMMARY_PREFIXES: &str =
    "added |removed |changed |audited |found 0 vulnerabilities|up to date";

fn is_npm_summary_line(t: &str) -> bool {
    starts_with_any(t, NPM_SUMMARY_PREFIXES)
}

const NPM_NOISE_PREFIXES: &str =
    "npm http|npm timing|npm verb|npm sill|npm info|run `npm fund`|run \"npm fund\"";

fn is_npm_noise_line(t: &str) -> bool {
    starts_with_any(t, NPM_NOISE_PREFIXES) || t.contains("packages are looking for funding")
}

const GIT_PROGRESS_PREFIXES: &str = "remote: Enumerating objects|remote: Counting objects|remote: Compressing objects|remote: Total|Receiving objects|Resolving deltas|Counting objects|Compressing objects|Writing objects|Unpacking objects";

fn git_progress_prefix(t: &str) -> Option<&'static str> {
    GIT_PROGRESS_PREFIXES.split('|').find(|p| t.starts_with(p))
}

/// Selects information-dense lines within a soft budget while always retaining criticals.
pub fn summarize_tokens(text: &str, max_tokens: usize, prefix: &str) -> String {
    if max_tokens == 0 {
        return format!("{prefix}{}", text.trim_end());
    }
    let lines: Vec<&str> = text.lines().collect();
    if count_tokens(text) <= max_tokens || lines.len() <= 4 {
        return format!("{prefix}{}", text.trim_end());
    }
    let n = lines.len();
    let line_tokens: Vec<usize> = lines.iter().map(|l| count_tokens(l)).collect();
    let mut order: Vec<usize> = (0..n).collect();
    let scores: Vec<u32> = (0..n)
        .map(|idx| {
            let line = lines[idx];
            if looks_critical_line(line) {
                100
            } else if line.trim().is_empty() {
                0
            } else {
                let mut s = if idx < 3 || idx + 3 >= n { 60 } else { 0 };
                if line_information_density(line) {
                    s += 30;
                }
                s.max(1)
            }
        })
        .collect();
    order.sort_by(|a, b| scores[*b].cmp(&scores[*a]).then(a.cmp(b)));
    let mut selected = vec![false; n];
    let mut spent = 0usize;
    for &idx in &order {
        let cost = line_tokens[idx];
        if scores[idx] >= 100 || (scores[idx] != 0 && spent + cost + 13 <= max_tokens) {
            selected[idx] = true;
            spent = if scores[idx] >= 100 {
                spent.saturating_add(cost)
            } else {
                spent + cost
            };
        }
    }
    for idx in 1..n.saturating_sub(1) {
        if !selected[idx] && selected[idx - 1] && selected[idx + 1] && line_tokens[idx] <= 13 {
            selected[idx] = true;
        }
    }
    if !selected.iter().any(|v| *v) {
        return summarize_lines(text, 8, 6, prefix);
    }
    let mut out = prefix.to_string();
    let mut omitted = 0;
    for idx in 0..n {
        if !selected[idx] {
            omitted += 1;
            continue;
        }
        if omitted > 0 {
            push_summary_line(
                &mut out,
                &format!("... +{omitted} lines; exact ref available ..."),
            );
            omitted = 0;
        }
        push_summary_line(&mut out, lines[idx]);
    }
    if omitted > 0 {
        push_summary_line(
            &mut out,
            &format!("... +{omitted} lines; exact ref available ..."),
        );
    }
    out
}

fn push_summary_line(out: &mut String, line: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
}

/// Detects artifact identifiers, paths, line references, numbers, or hashes.
fn line_information_density(line: &str) -> bool {
    let (digits, paths) = line.chars().fold((0, 0), |(d, p), c| {
        (
            d + usize::from(c.is_ascii_digit()),
            p + usize::from(c == '/' || c == '\\'),
        )
    });
    digits >= 3 || paths >= 2 || line.contains(".rs:") || line.contains(".py:")
}

/// Shell-only dedupe also collapses digit-varying runs while preserving critical lines.
fn dedupe_lines_impl(text: &str, context: usize, structural: bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let norm = structural.then(|| {
        lines
            .iter()
            .map(|l| normalize_digit_runs(l))
            .collect::<Vec<_>>()
    });
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < lines.len() {
        let line = lines[idx];
        if structural && looks_critical_line(line) {
            out.push(line.to_string());
            idx += 1;
            continue;
        }
        let exact = lines[idx..]
            .iter()
            .take_while(|candidate| **candidate == line)
            .count();
        if exact >= 3 {
            out.push(line.to_string());
            out.push(format!("... repeated {} more times ...", exact - 1));
            idx += exact;
            continue;
        }
        if let Some(n) = norm.as_ref() {
            let similar = (idx..lines.len())
                .take_while(|&i| !looks_critical_line(lines[i]) && n[i] == n[idx])
                .count();
            if similar >= 4 {
                out.push(line.to_string());
                out.push(format!(
                    "... {} similar lines collapsed (digits vary); exact ref available ...",
                    similar - 1
                ));
                idx += similar;
                continue;
            }
        }
        out.extend(lines[idx..idx + exact].iter().map(|l| l.to_string()));
        idx += exact;
    }
    compact_head_tail(out, context)
}

fn compact_head_tail(out: Vec<String>, context: usize) -> String {
    if out.len() <= context * 2 + 20 {
        return out.join("\n");
    }
    format!(
        "{}\n... omitted {} lines; exact ref available ...\n{}",
        out[..context].join("\n"),
        out.len().saturating_sub(context * 2),
        out[out.len() - context..].join("\n")
    )
}

fn normalize_digit_runs(line: &str) -> String {
    let (mut out, mut in_d) = (String::with_capacity(line.len()), false);
    for c in line.chars() {
        if c.is_ascii_digit() {
            if !in_d {
                out.push('#');
                in_d = true;
            }
        } else {
            in_d = false;
            out.push(c);
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
    input.mode.effective_policy() == Mode::Auto
        && policy.policy == "diagnostic"
        && !status.command_success
        && input.exit_code.is_some()
        && !input.timed_out
        && !(input.exit_code == Some(0) && status.pipeline_masking_warning.is_some())
        && input.combined_ref.is_some()
        && (!input.stdout.trim().is_empty() || !input.stderr.trim().is_empty())
        && !has_visible_secret_marker(combined)
        && !has_protected_failure_context(combined)
        && count_tokens(combined) <= 160
        && combined.lines().count() <= 20
        && (looks_diagnostic(combined) || status.failed_segment.is_some())
}

fn compact_diagnostic_shell_view(stdout: &str, stderr: &str) -> String {
    let (mut critical, mut fallback) = (None, None);
    for line in stdout.lines().chain(stderr.lines()) {
        let line = line.trim();
        if line.is_empty() || is_shell_diagnostic_boilerplate(line) {
            continue;
        }
        if looks_failure_anchor_line(line) {
            return line.to_string();
        }
        let slot = if looks_critical_line(line) {
            &mut critical
        } else {
            &mut fallback
        };
        slot.get_or_insert_with(|| line.to_string());
    }
    critical
        .or(fallback)
        .unwrap_or_else(|| "diagnostic output omitted; see combined_ref".to_string())
}

fn compact_diagnostic_shell_capsule(
    input: &ShellRenderInput<'_>,
    status: &CommandStatus,
    body: &str,
) -> String {
    let mut visible = "# shell\n".to_string();
    push_shell_kv(&mut visible, "status", &status.status_label);
    push_shell_status(&mut visible, status, true);
    if input.stderr.is_empty() {
        if let Some(stdout_ref) = input.stdout_ref.filter(|_| !input.stdout.is_empty()) {
            push_shell_kv(&mut visible, "stdout_ref", stdout_ref);
        }
    } else if let Some(stderr_ref) = input.stderr_ref.filter(|_| !input.stderr.is_empty()) {
        push_shell_kv(&mut visible, "stderr_ref", stderr_ref);
    }
    push_optional_shell_kv(&mut visible, "combined_ref", input.combined_ref);
    visible + "\n" + body.trim_end()
}

const SHELL_DIAG_BOILERPLATE_PREFIXES: &str =
    "+ CategoryInfo|+ FullyQualifiedErrorId|At line:|+ ~|~~~~";
const FAILURE_ANCHOR_NEEDLES: &str =
    "error|failure|failed|panic|traceback|exception|assertion|not ok";
const SECRET_MARKERS: &str = "token=|password=|secret=|api_key=|authorization:|bearer ";

fn is_shell_diagnostic_boilerplate(line: &str) -> bool {
    starts_with_any(line.trim_start(), SHELL_DIAG_BOILERPLATE_PREFIXES)
}

fn looks_failure_anchor_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    FAILURE_ANCHOR_NEEDLES
        .split('|')
        .any(|needle| lower.contains(needle))
}

fn has_visible_secret_marker(text: &str) -> bool {
    contains_any(&text.to_ascii_lowercase(), SECRET_MARKERS)
}

fn has_protected_failure_context(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(&lower, "assertion failed|traceback")
        || (lower.contains("left:") && lower.contains("right:"))
        || (lower.contains("test ") && lower.contains("failed") && lower.contains(".rs:"))
}

const DIFF_LINE_PREFIXES: &str = "diff --git|index |--- |+++ |@@|rename |deleted file|new file|+|-";

pub fn diff_summary(text: &str, max_lines: usize) -> String {
    let out: Vec<_> = text
        .lines()
        .filter(|l| starts_with_any(l, DIFF_LINE_PREFIXES))
        .take(max_lines.max(1))
        .collect();
    if out.is_empty() {
        summarize_lines(text, 18, 12, "")
    } else {
        out.join("\n")
    }
}

pub fn dedupe_lines(text: &str, context: usize) -> String {
    dedupe_lines_impl(text, context, false)
}

pub fn mask_visible_secrets(text: &str) -> String {
    text.lines()
        .map(mask_secret_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mask_secret_line(line: &str) -> String {
    let low = line.to_ascii_lowercase();
    // Keep trailing space on "bearer " so the marker matches SECRET_MARKERS and
    // the mask lands after the separator (not glued as "bearer[masked]").
    if let Some((key, pos)) = [
        "token=",
        "password=",
        "secret=",
        "api_key=",
        "apikey=",
        "authorization:",
        "bearer ",
    ]
    .into_iter()
    .find_map(|key| low.find(key).map(|pos| (key, pos)))
    {
        return format!("{}[masked]", &line[..pos + key.len()]);
    }
    line.split_whitespace()
        .map(|word| {
            if starts_with_any(word, "sk-|sk-proj-|ghp_|AKIA") {
                "[masked]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn critical_lines(text: &str, radius: usize) -> String {
    keyword_window_view(text, radius, looks_critical_line)
}

pub fn error_block(text: &str, radius: usize) -> String {
    keyword_window_view(text, radius, |line| regex_like_error(&line))
}

/// Keeps radius windows around hits and marks every omitted gap explicitly.
fn omitted_lines_marker(n: usize) -> String {
    format!("... omitted {n} lines; exact ref available ...")
}
fn keyword_window_view(text: &str, radius: usize, is_hit: impl Fn(&str) -> bool) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![false; lines.len()];
    for (idx, line) in lines.iter().enumerate() {
        if is_hit(line) {
            let (start, end) = (
                idx.saturating_sub(radius),
                (idx + radius + 1).min(lines.len()),
            );
            keep[start..end].fill(true);
        }
    }
    if !keep.iter().any(|&k| k) {
        return String::new();
    }
    let (mut out, mut idx) = (Vec::new(), 0);
    while idx < lines.len() {
        if keep[idx] {
            out.push(lines[idx].to_string());
            idx += 1;
        } else {
            let start = idx;
            while idx < lines.len() && !keep[idx] {
                idx += 1;
            }
            out.push(omitted_lines_marker(idx - start));
        }
    }
    out.join("\n")
}

const ERROR_NEEDLES: &str = "error exception traceback failed assertion panic expected actual";

fn regex_like_error(line: &&str) -> bool {
    contains_any_ws(&line.to_ascii_lowercase(), ERROR_NEEDLES)
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
    let indent = leading_ws(lines[hit]);
    let mut end = hit + 1;
    while end < lines.len() {
        let line = lines[end];
        if !line.trim().is_empty() && leading_ws(line) <= indent && end > hit + 1 {
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

/// Generates a legacy short ref: `<prefix>` plus the first eight SHA-256 bytes.
pub fn id_for(prefix: char, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let mut out = format!("{prefix}");
    for byte in &hasher.finalize()[..8] {
        push_hex_byte(&mut out, *byte);
    }
    out
}

const CODE_EXTENSIONS: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "c", "cc", "cpp", "h", "hpp",
];

fn looks_like_logs(text: &str) -> bool {
    let s: Vec<&str> = text.lines().take(20).collect();
    s.len() >= 5
        && s.iter()
            .filter(|l| {
                contains_any_ws(&l.to_ascii_uppercase(), "DEBUG INFO WARN ERROR FATAL TRACE")
            })
            .count()
            > s.len() / 3
}
pub fn detect_content_type(text: &str, path: Option<&Path>) -> ContentType {
    if let Some(ext) = path.and_then(|p| p.extension()).and_then(|v| v.to_str()) {
        match ext {
            ext if CODE_EXTENSIONS.contains(&ext) => return ContentType::Code,
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
    } else if looks_like_logs(text) {
        ContentType::Logs
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

pub mod operation_abi;
mod protocol_atoms;
mod render;
mod shell_display;
mod shell_family;
mod shell_parse;
mod shell_policy;
mod shell_quote;
pub mod token_classes;
mod tokens;

use render::domain::*;
use render::noise::*;
use shell_display::*;
use shell_parse::*;
use tokens::*;

pub use protocol_atoms::{
    AckClass, PORTABLE_ONE_TOKEN_ATOMS, ProtocolTokenizer, is_verified_one_token_atom,
    portable_one_token_atoms, render_ack,
};
pub use render::domain::{
    diagnostic_shell_view, diagnostic_shell_view_with_tail, is_repo_inventory_command,
    repo_inventory_view, structured_shell_view,
};
pub use shell_display::{
    shell_display_command_from_argv, shell_display_command_from_argv_for_platform,
};
pub use shell_family::shell_family;
pub use shell_policy::{classify_command_status, decide_shell_policy, shell_combined_output};
pub use shell_quote::{
    argv_has_shell_operator_tokens, contains_platform_shell_syntax, contains_shell_syntax,
    host_shell_platform, is_shell_operator_token, is_windows_shell_builtin, is_windows_shell_host,
    looks_like_powershell_syntax, quote_for, quote_posix, quote_powershell, quote_windows_cmd,
    split_command_string, split_command_string_for_platform,
};
pub use tokens::{
    TokenizerFamily, TokenizerMetadata, active_model_id, active_tokenizer_metadata, count_tokens,
    count_tokens_for_model, enforce_token_budget, enforce_token_budget_with_ref,
    pack_to_token_boundary, pack_to_token_boundary_for_model,
    pack_to_token_boundary_with_char_limit, savings_ratio, sha256_hex, tokenizer_metadata,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_anchor_needles_preserve_multiword_not_ok() {
        assert!(looks_failure_anchor_line("not ok 12 - parser"));
        assert!(looks_failure_anchor_line("panic: parser failed"));
        assert!(!looks_failure_anchor_line("ok 12 - parser"));
        assert!(!looks_failure_anchor_line("not ready yet"));
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod semantic_tests;

#[cfg(test)]
mod channel_mode_tests {
    use super::*;

    #[test]
    fn legacy_truthy_values_stay_action_only() {
        for raw in ["1", "on", "TRUE", " yes ", "action"] {
            let mode = ChannelMode::from_env_value(raw);
            assert_eq!(mode, ChannelMode::Action, "{raw}");
            assert!(mode.enabled());
            assert!(!mode.emits_user_message(), "{raw} must stay action-only");
        }
    }

    #[test]
    fn terminal_mode_opts_into_receipt_user_message() {
        for raw in ["terminal", "Final"] {
            let mode = ChannelMode::from_env_value(raw);
            assert_eq!(mode, ChannelMode::Terminal, "{raw}");
            assert!(mode.enabled());
            assert!(mode.emits_user_message());
        }
    }

    #[test]
    fn unknown_and_falsy_values_are_off() {
        for raw in ["", "0", "off", "nonsense"] {
            let mode = ChannelMode::from_env_value(raw);
            assert_eq!(mode, ChannelMode::Off, "{raw}");
            assert!(!mode.enabled());
            assert!(!mode.emits_user_message());
        }
    }
}

#[cfg(test)]
mod core_safety_regressions {
    use super::*;

    #[test]
    fn equals_prefixed_diagnostic_is_a_continuation() {
        assert!(is_critical_continuation_line("= short test summary info ="));
    }
}

#[cfg(test)]
mod capsule_omission_exact_ref {
    use super::*;

    fn big_original() -> String {
        (0..4000)
            .map(|i| format!("line {i} alpha beta gamma delta epsilon token content sample\n"))
            .collect()
    }

    fn truncated_capsule() -> Capsule {
        Capsule {
            visible_tokens: 10,
            raw_tokens: 100,
            omitted_lines: 3990,
            text: "line 0 alpha beta gamma delta epsilon token content sample".to_string(),
            mode: Mode::Auto,
            protected_anchors: Vec::new(),
            exact_refs: Vec::new(),
            lossy_spans: Vec::new(),
            lossy_policy_id: None,
        }
    }

    /// tokenzero-kt7z: `tokenzero read --json` aborted with exit 101 on any file
    /// large enough to be budgeted, because this branch recorded the recovery ref
    /// in `exact_refs` but never put it in the visible text -- and
    /// `validate_omission_rule` requires it in the text. The struct field alone
    /// satisfied nobody: an agent cannot expand a ref it cannot see.
    #[test]
    fn exact_ref_branch_puts_the_ref_where_an_agent_can_see_it() {
        let original = big_original();
        let capsule = finalize_capsule_omission(
            truncated_capsule(),
            &original,
            0,
            Some("tz://blob/abc#B0-100".to_string()),
        );
        assert!(
            capsule.text.contains("tz://blob/abc#B0-100"),
            "recovery ref must be visible, not merely recorded: {}",
            capsule.text
        );
        assert!(
            capsule
                .exact_refs
                .iter()
                .any(|r| r == "tz://blob/abc#B0-100")
        );
        capsule
            .validate_omission_rule(&original)
            .expect("omission rule must hold");
    }

    /// The panic was reached through the ordinary read path, so guard the whole
    /// path and not just the helper: a ref without a selector must fall through
    /// to the lossy declaration rather than claiming exact recovery.
    #[test]
    fn ref_without_a_selector_falls_back_to_a_declared_lossy_capsule() {
        let original = big_original();
        let capsule = finalize_capsule_omission(
            truncated_capsule(),
            &original,
            0,
            Some("tz://blob/abc".to_string()),
        );
        assert_eq!(capsule.mode, Mode::Lossy);
        assert!(capsule.text.contains("mode=lossy"), "{}", capsule.text);
        assert!(capsule.exact_refs.is_empty());
        capsule
            .validate_omission_rule(&original)
            .expect("omission rule must hold");
    }
}
