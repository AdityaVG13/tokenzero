//! Crash-only recovery unlock when the CodeMode expand/read surface is unhealthy.
//!
//! Field bug (wqw.9 / zerostackbug6): agents are told the primary surface is
//! healthy while `zero.token.expand` returns X0, and `tz_expand` stays locked
//! as a crash-only shim — catch-22 that drives them to native Read.
//!
//! Policy matrix (CodeMode surface):
//! | Tool class              | Healthy | Unhealthy (expand X0 / substrate_down) |
//! |-------------------------|---------|----------------------------------------|
//! | codemode execute/search | allow   | allow                                  |
//! | expand / read recovery  | **block** (crash-only) | **unlock** (audit + telemetry) |
//! | shell / edit / write    | block   | block (never unlocked by expand health)|
//!
//! Classic surface is not gated: per-op tools are the primary surface.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokenzero_core::McpToolSurface;

/// Default: one expand-surface failure opens recovery for the window.
const DEFAULT_FAIL_THRESHOLD: u32 = 1;
/// Default unlock window after the last failure (5 minutes).
const DEFAULT_WINDOW: Duration = Duration::from_secs(300);

/// Documented recovery ladder (docs + skill + close reasons).
pub const RECOVERY_LADDER: &str = "\
CodeMode recovery ladder (expand/read only):\n\
1. Prefer zero.token.expand / zero.token.read inside tz_execute_code (primary).\n\
2. If expand returns X0 or substrate_down, surface health opens crash-only recovery:\n\
   call tz_expand / tz_read (or CLI `tokenzero expand` / `tokenzero read`) — not native Read.\n\
3. Write/shell stay crash-only locked; do not permanently weaken mutation safety.\n\
4. After a successful expand/read, the primary surface is healthy again and recovery re-locks.\n\
Telemetry: resource://tokenzero/metrics → surface_health (blocked vs unlocked counts).";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashOnlyDecision {
    /// Primary surface healthy — recovery shim blocked.
    Blocked,
    /// Surface unhealthy — recovery path unlocked (count + audit).
    Unlocked,
    /// Mutation/shell: never unlocked by expand health on CodeMode.
    PermanentlyLocked,
    /// Not subject to crash-only gate (classic surface or codemode primary tools).
    NotGated,
}

/// Canonical tool class after stripping `tz_` / hyphen aliases.
///
/// Single source of truth for CodeMode membership + crash-only gating.
/// Catalog `tools/list`, JSON-RPC `tools/call`, and FastMCP registration all
/// consult this classification (via [`list_includes`] / [`admit_tools_call`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolClass {
    /// Always listed/callable on CodeMode (`execute_code`, report, …).
    Primary,
    /// Crash-only recovery shims (`expand` / `read`).
    Recovery,
    /// Never unlocked by expand health (shell/edit/write/…).
    Locked,
}

/// Whether a `tools/call` name is even a candidate on this surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallAdmission {
    /// Surface does not own this tool (e.g. Classic calling `tz_execute_code`).
    UnknownTool,
    /// Proceed to crash-only [`SurfaceHealth::allow_tool_call`].
    Proceed,
}

pub(crate) fn tool_class(tool_name: &str) -> ToolClass {
    let canonical = strip_tool_alias(tool_name);
    match canonical {
        "execute_code" | "codemode_search" | "codemode_describe" | "codemode"
        | "report_tool_issue" => ToolClass::Primary,
        "expand" | "read" => ToolClass::Recovery,
        // Everything else on CodeMode is permanently locked (shell/edit/write/…).
        _ => ToolClass::Locked,
    }
}

fn strip_tool_alias(name: &str) -> &str {
    let bare = name.strip_prefix("tz_").unwrap_or(name);
    match bare {
        "report-tool-issue" => "report_tool_issue",
        other => other,
    }
}

/// CodeMode-exclusive primaries (not listed/callable on Classic).
/// `report_tool_issue` is intentionally available on both surfaces.
fn is_codemode_exclusive(tool_name: &str) -> bool {
    matches!(
        strip_tool_alias(tool_name),
        "execute_code" | "codemode_search" | "codemode_describe" | "codemode"
    )
}

/// Whether `tools/list` (and FastMCP registration) should advertise `tool_name`.
pub(crate) fn tool_listed_on_surface(
    surface: McpToolSurface,
    tool_name: &str,
    recovery_unlocked: bool,
) -> bool {
    match surface {
        McpToolSurface::Classic => !is_codemode_exclusive(tool_name),
        McpToolSurface::CodeMode => match tool_class(tool_name) {
            ToolClass::Primary => true,
            ToolClass::Recovery => recovery_unlocked,
            ToolClass::Locked => false,
        },
    }
}

/// Static membership (healthy CodeMode = recovery hidden). Prefer
/// [`tool_listed_on_surface`] when health is known.
pub(crate) fn surface_includes(surface: McpToolSurface, tool_name: &str) -> bool {
    tool_listed_on_surface(surface, tool_name, false)
}

/// Admit a `tools/call` before the crash-only health gate.
pub(crate) fn admit_tools_call(surface: McpToolSurface, tool_name: &str) -> CallAdmission {
    match surface {
        McpToolSurface::Classic if is_codemode_exclusive(tool_name) => CallAdmission::UnknownTool,
        // CodeMode: Primary/Recovery/Locked all reach allow_tool_call so agents
        // get policy_refusal (ladder / never-unlocked) instead of unknown_tool.
        _ => CallAdmission::Proceed,
    }
}

#[derive(Debug, Clone)]
struct HealthInner {
    consecutive_failures: u32,
    last_failure_at: Option<Instant>,
    last_failure_kind: Option<String>,
    blocked_count: u64,
    unlocked_count: u64,
    fail_threshold: u32,
    window: Duration,
}

impl Default for HealthInner {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure_at: None,
            last_failure_kind: None,
            blocked_count: 0,
            unlocked_count: 0,
            fail_threshold: DEFAULT_FAIL_THRESHOLD,
            window: DEFAULT_WINDOW,
        }
    }
}

impl HealthInner {
    fn is_unhealthy(&self, now: Instant) -> bool {
        if self.consecutive_failures < self.fail_threshold {
            return false;
        }
        match self.last_failure_at {
            Some(at) => now.duration_since(at) < self.window,
            None => false,
        }
    }

    fn is_healthy(&self, now: Instant) -> bool {
        !self.is_unhealthy(now)
    }

    fn record_failure(&mut self, kind: &str, now: Instant) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_failure_at = Some(now);
        self.last_failure_kind = Some(kind.to_string());
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
    }
}

/// Session-scoped expand/read surface health + crash-only gate.
#[derive(Debug)]
pub struct SurfaceHealth {
    inner: Mutex<HealthInner>,
}

impl Default for SurfaceHealth {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HealthInner::default()),
        }
    }
}

impl SurfaceHealth {
    pub fn new() -> Self {
        Self::default()
    }

    /// Test / config hook for threshold and unlock window.
    #[allow(dead_code)]
    pub fn with_policy(fail_threshold: u32, window: Duration) -> Self {
        let mut inner = HealthInner::default();
        inner.fail_threshold = fail_threshold.max(1);
        inner.window = window;
        Self {
            inner: Mutex::new(inner),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HealthInner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn is_healthy(&self) -> bool {
        let inner = self.lock();
        inner.is_healthy(Instant::now())
    }

    /// True when recovery expand/read may be engaged on CodeMode.
    pub fn recovery_unlocked(&self) -> bool {
        !self.is_healthy()
    }

    /// Record an expand/read surface outcome. Client mistakes (`invalid_ref`)
    /// do not unlock. Success clears the failure window.
    pub fn record_expand_outcome(&self, ok: bool, code: Option<&str>) {
        let now = Instant::now();
        let mut inner = self.lock();
        if ok {
            inner.record_success();
            return;
        }
        let kind = code.unwrap_or("expand_failed");
        if kind == "invalid_ref" {
            return;
        }
        inner.record_failure(kind, now);
    }

    /// Codemode plan ended in X0 while expand/read was in the plan.
    pub fn record_codemode_expand_x0(&self) {
        self.record_expand_outcome(false, Some("expand_x0"));
    }

    pub fn record_substrate_down(&self) {
        self.record_expand_outcome(false, Some("substrate_down"));
    }

    /// Never claim "primary surface healthy" when unhealthy.
    pub fn primary_surface_healthy_claim(&self) -> bool {
        self.is_healthy()
    }

    pub fn decide(&self, surface: McpToolSurface, tool_name: &str) -> CrashOnlyDecision {
        decide_static(surface, tool_name, self.recovery_unlocked())
    }

    /// Gate a tools/call: Ok(decision) when allowed, Err(policy message) when refused.
    /// Updates blocked/unlocked telemetry on recovery decisions.
    pub fn allow_tool_call(
        &self,
        surface: McpToolSurface,
        tool_name: &str,
    ) -> Result<CrashOnlyDecision, String> {
        let decision = self.decide(surface, tool_name);
        match decision {
            CrashOnlyDecision::NotGated => Ok(decision),
            CrashOnlyDecision::Unlocked => {
                let mut inner = self.lock();
                inner.unlocked_count = inner.unlocked_count.saturating_add(1);
                Ok(CrashOnlyDecision::Unlocked)
            }
            CrashOnlyDecision::Blocked => {
                let mut inner = self.lock();
                inner.blocked_count = inner.blocked_count.saturating_add(1);
                Err(blocked_message(tool_name))
            }
            CrashOnlyDecision::PermanentlyLocked => Err(format!(
                "Policy: {tool_name} is not available on the CodeMode surface \
                 (write/shell safety is never unlocked by expand health). \
                 Use zero.token.* inside tz_execute_code."
            )),
        }
    }

    /// Whether tools/list should advertise `tool_name` given current health.
    pub fn list_includes(&self, surface: McpToolSurface, tool_name: &str) -> bool {
        tool_listed_on_surface(surface, tool_name, self.recovery_unlocked())
    }

    pub fn telemetry(&self) -> Value {
        let inner = self.lock();
        let now = Instant::now();
        let healthy = inner.is_healthy(now);
        json!({
            "schema_version": "tokenzero.surface_health.v1",
            "primary_surface_healthy": healthy,
            "recovery_unlocked": !healthy,
            "consecutive_failures": inner.consecutive_failures,
            "last_failure_kind": inner.last_failure_kind,
            "fail_threshold": inner.fail_threshold,
            "window_secs": inner.window.as_secs(),
            "telemetry": {
                "blocked_count": inner.blocked_count,
                "unlocked_count": inner.unlocked_count,
            },
            "recovery_ladder": RECOVERY_LADDER,
            "unlocks": ["expand", "read"],
            "never_unlocks": ["shell", "edit", "write"],
        })
    }
}

fn blocked_message(tool_name: &str) -> String {
    let short = strip_tool_alias(tool_name);
    format!(
        "Policy: tz_{short} is a crash-only recovery tool; the CodeMode primary surface is healthy. \
         Use zero.token.{short} via tz_execute_code. If expand fails with X0 or substrate_down, \
         recovery unlocks automatically for expand/read only (not write/shell). \
         Ladder: see resource://tokenzero/metrics surface_health."
    )
}

/// Pure policy helper (tests + call-path membership checks without engine state).
pub fn decide_static(
    surface: McpToolSurface,
    tool_name: &str,
    recovery_unlocked: bool,
) -> CrashOnlyDecision {
    if surface != McpToolSurface::CodeMode {
        return CrashOnlyDecision::NotGated;
    }
    match tool_class(tool_name) {
        ToolClass::Primary => CrashOnlyDecision::NotGated,
        ToolClass::Locked => CrashOnlyDecision::PermanentlyLocked,
        ToolClass::Recovery => {
            if recovery_unlocked {
                CrashOnlyDecision::Unlocked
            } else {
                CrashOnlyDecision::Blocked
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokenzero_core::McpToolSurface;

    #[test]
    fn healthy_blocks_expand_on_codemode() {
        let h = SurfaceHealth::new();
        assert!(h.is_healthy());
        assert!(h.primary_surface_healthy_claim());
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "expand"),
            CrashOnlyDecision::Blocked
        );
        let err = h
            .allow_tool_call(McpToolSurface::CodeMode, "tz_expand")
            .unwrap_err();
        assert!(
            err.contains("primary surface is healthy"),
            "blocked message must claim healthy only when healthy: {err}"
        );
        assert_eq!(h.telemetry()["telemetry"]["blocked_count"], 1);
    }

    #[test]
    fn expand_x0_unlocks_recovery_and_false_healthy_claim() {
        let h = SurfaceHealth::new();
        h.record_codemode_expand_x0();
        assert!(!h.is_healthy());
        assert!(!h.primary_surface_healthy_claim());
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "expand"),
            CrashOnlyDecision::Unlocked
        );
        assert!(
            h.allow_tool_call(McpToolSurface::CodeMode, "tz_expand")
                .is_ok()
        );
        assert_eq!(h.telemetry()["telemetry"]["unlocked_count"], 1);
        assert_eq!(h.telemetry()["primary_surface_healthy"], false);
    }

    #[test]
    fn substrate_down_unlocks_read_not_shell() {
        let h = SurfaceHealth::new();
        h.record_substrate_down();
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "read"),
            CrashOnlyDecision::Unlocked
        );
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "shell"),
            CrashOnlyDecision::PermanentlyLocked
        );
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "edit"),
            CrashOnlyDecision::PermanentlyLocked
        );
        assert!(
            h.allow_tool_call(McpToolSurface::CodeMode, "tz_shell")
                .unwrap_err()
                .contains("never unlocked")
        );
    }

    #[test]
    fn success_re_locks_recovery() {
        let h = SurfaceHealth::new();
        h.record_expand_outcome(false, Some("expand_failed"));
        assert!(h.recovery_unlocked());
        h.record_expand_outcome(true, None);
        assert!(h.is_healthy());
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "expand"),
            CrashOnlyDecision::Blocked
        );
    }

    #[test]
    fn invalid_ref_does_not_unlock() {
        let h = SurfaceHealth::new();
        h.record_expand_outcome(false, Some("invalid_ref"));
        assert!(h.is_healthy());
    }

    #[test]
    fn classic_surface_not_gated() {
        let h = SurfaceHealth::new();
        assert_eq!(
            h.decide(McpToolSurface::Classic, "expand"),
            CrashOnlyDecision::NotGated
        );
        assert!(h.allow_tool_call(McpToolSurface::Classic, "expand").is_ok());
    }

    #[test]
    fn static_policy_matrix() {
        use CrashOnlyDecision::*;
        assert_eq!(
            decide_static(McpToolSurface::CodeMode, "tz_execute_code", false),
            NotGated
        );
        assert_eq!(
            decide_static(McpToolSurface::CodeMode, "expand", false),
            Blocked
        );
        assert_eq!(
            decide_static(McpToolSurface::CodeMode, "expand", true),
            Unlocked
        );
        assert_eq!(
            decide_static(McpToolSurface::CodeMode, "shell", true),
            PermanentlyLocked
        );
        assert_eq!(
            decide_static(McpToolSurface::Classic, "shell", false),
            NotGated
        );
    }

    #[test]
    fn recovery_ladder_documented() {
        assert!(RECOVERY_LADDER.contains("zero.token.expand"));
        assert!(RECOVERY_LADDER.contains("tz_expand"));
        assert!(RECOVERY_LADDER.contains("not native Read"));
        assert!(RECOVERY_LADDER.contains("Write/shell"));
    }

    #[test]
    fn report_tool_issue_not_gated_on_codemode() {
        let h = SurfaceHealth::new();
        assert_eq!(
            h.decide(McpToolSurface::CodeMode, "tz_report_tool_issue"),
            CrashOnlyDecision::NotGated
        );
        assert!(
            h.allow_tool_call(McpToolSurface::CodeMode, "report_tool_issue")
                .is_ok()
        );
    }

    #[test]
    fn tool_class_uses_canonical_names() {
        assert_eq!(tool_class("tz_expand"), ToolClass::Recovery);
        assert_eq!(tool_class("expand"), ToolClass::Recovery);
        assert_eq!(tool_class("tz_shell"), ToolClass::Locked);
        assert_eq!(tool_class("report-tool-issue"), ToolClass::Primary);
        assert_eq!(tool_class("tz_execute_code"), ToolClass::Primary);
    }

    #[test]
    fn list_and_call_share_one_policy() {
        // Classic never lists CodeMode execute.
        assert!(!tool_listed_on_surface(
            McpToolSurface::Classic,
            "tz_execute_code",
            false
        ));
        assert_eq!(
            admit_tools_call(McpToolSurface::Classic, "tz_execute_code"),
            CallAdmission::UnknownTool
        );
        // CodeMode lists report always; recovery only when unlocked.
        assert!(tool_listed_on_surface(
            McpToolSurface::CodeMode,
            "tz_report_tool_issue",
            false
        ));
        assert!(!tool_listed_on_surface(
            McpToolSurface::CodeMode,
            "tz_expand",
            false
        ));
        assert!(tool_listed_on_surface(
            McpToolSurface::CodeMode,
            "tz_expand",
            true
        ));
        // Locked tools are never listed but still admit to the health gate.
        assert!(!tool_listed_on_surface(
            McpToolSurface::CodeMode,
            "tz_shell",
            true
        ));
        assert_eq!(
            admit_tools_call(McpToolSurface::CodeMode, "tz_shell"),
            CallAdmission::Proceed
        );
    }
}
