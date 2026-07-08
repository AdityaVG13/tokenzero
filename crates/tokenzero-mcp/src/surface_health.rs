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

/// Recovery tools that may unlock when expand/read health is bad.
const RECOVERY_TOOLS: &[&str] = &["expand", "tz_expand", "read", "tz_read"];

/// Mutation / shell tools — never unlocked by expand health.
const PERMANENTLY_LOCKED_CODEMODE: &[&str] = &[
    "shell",
    "tz_shell",
    "edit",
    "tz_edit",
    "write",
    "find",
    "tz_find",
    "grep",
    "tz_grep",
    "glob",
    "tz_glob",
    "tree",
    "tz_tree",
    "ingest",
    "tz_ingest",
    "fetch",
    "tz_fetch",
    "batch",
    "tz_batch",
    "mem",
    "tz_mem",
    "cache_pack",
    "tz_cache_pack",
    "rewrite",
    "tz_rewrite",
    "discover",
    "tz_discover",
    "recall",
    "tz_recall",
];

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
        // Keep last_failure_* for forensics; window no longer applies once cleared.
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

    /// Record an expand-path failure that indicates surface unhealth.
    /// Client mistakes (`invalid_ref`) do not unlock.
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
        let now = Instant::now();
        self.lock().record_failure("expand_x0", now);
    }

    pub fn record_substrate_down(&self) {
        let now = Instant::now();
        self.lock().record_failure("substrate_down", now);
    }

    #[allow(dead_code)]
    pub fn record_read_outcome(&self, ok: bool, code: Option<&str>) {
        // Read failures contribute to the same recovery unlock ladder.
        self.record_expand_outcome(
            ok,
            code.map(|c| if c.is_empty() { "read_failed" } else { c }),
        );
    }

    /// Never claim "primary surface healthy" when unhealthy.
    pub fn primary_surface_healthy_claim(&self) -> bool {
        self.is_healthy()
    }

    pub fn decide(&self, surface: McpToolSurface, tool_name: &str) -> CrashOnlyDecision {
        if surface != McpToolSurface::CodeMode {
            return CrashOnlyDecision::NotGated;
        }
        let canonical = strip_tz_prefix(tool_name);
        if is_codemode_primary(canonical) || is_codemode_primary(tool_name) {
            return CrashOnlyDecision::NotGated;
        }
        if is_permanently_locked(canonical) || is_permanently_locked(tool_name) {
            return CrashOnlyDecision::PermanentlyLocked;
        }
        if is_recovery_tool(canonical) || is_recovery_tool(tool_name) {
            if self.recovery_unlocked() {
                return CrashOnlyDecision::Unlocked;
            }
            return CrashOnlyDecision::Blocked;
        }
        // Unknown non-primary tools on CodeMode stay locked.
        CrashOnlyDecision::PermanentlyLocked
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
    #[allow(dead_code)]
    pub fn list_includes(&self, surface: McpToolSurface, tool_name: &str) -> bool {
        match self.decide(surface, tool_name) {
            CrashOnlyDecision::NotGated | CrashOnlyDecision::Unlocked => true,
            CrashOnlyDecision::Blocked | CrashOnlyDecision::PermanentlyLocked => false,
        }
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

fn strip_tz_prefix(name: &str) -> &str {
    name.strip_prefix("tz_").unwrap_or(name)
}

fn is_recovery_tool(name: &str) -> bool {
    RECOVERY_TOOLS.iter().any(|t| *t == name)
}

fn is_permanently_locked(name: &str) -> bool {
    PERMANENTLY_LOCKED_CODEMODE.iter().any(|t| *t == name)
}

fn is_codemode_primary(name: &str) -> bool {
    matches!(
        name,
        "tz_execute_code"
            | "execute_code"
            | "tz_codemode_search"
            | "codemode_search"
            | "tz_codemode_describe"
            | "codemode_describe"
            | "tz_codemode"
            | "codemode"
    )
}

fn blocked_message(tool_name: &str) -> String {
    let short = strip_tz_prefix(tool_name);
    format!(
        "Policy: tz_{short} is a crash-only recovery tool; the CodeMode primary surface is healthy. \
         Use zero.token.{short} via tz_execute_code. If expand fails with X0 or substrate_down, \
         recovery unlocks automatically for expand/read only (not write/shell). \
         Ladder: see resource://tokenzero/metrics surface_health."
    )
}

/// Pure policy helper for unit tests without engine state.
pub fn decide_static(
    surface: McpToolSurface,
    tool_name: &str,
    recovery_unlocked: bool,
) -> CrashOnlyDecision {
    if surface != McpToolSurface::CodeMode {
        return CrashOnlyDecision::NotGated;
    }
    let canonical = strip_tz_prefix(tool_name);
    if is_codemode_primary(canonical) || is_codemode_primary(tool_name) {
        return CrashOnlyDecision::NotGated;
    }
    if is_permanently_locked(canonical) || is_permanently_locked(tool_name) {
        return CrashOnlyDecision::PermanentlyLocked;
    }
    if is_recovery_tool(canonical) || is_recovery_tool(tool_name) {
        if recovery_unlocked {
            return CrashOnlyDecision::Unlocked;
        }
        return CrashOnlyDecision::Blocked;
    }
    CrashOnlyDecision::PermanentlyLocked
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
        // Refusal path must not claim healthy after X0.
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
}
