//! Write/edit recovery ladder when CodeMode mutation fails (wqw.12).
//!
//! Field bug: harness blocks native Write with "use CodeMode" only, so when
//! `zero.edit` / `zero.fs` mutate is broken agents have no next step.
//!
//! Ladder (TokenZero-owned surface):
//! 1. Prefer CodeMode `zero.edit` / `tz_edit` (find/replace hunks) or full-file
//!    write APIs when available on the FS substrate.
//! 2. Retry with explicit create/dry_run options; check allowed roots / doctor.
//! 3. If the write substrate is **down** (session surface unhealthy /
//!    `TOKENZERO_WRITE_ESCAPE=1` with explicit ack), harnesses may use a
//!    **bounded native Write** for that failure only — never as the default.
//! 4. Report the failure with `tz_report_tool_issue` (`tool=zero_execute` or
//!    `tz_edit`) so the field issue is recorded.

/// Documented write ladder text appended to mutation failures.
pub const WRITE_RECOVERY_LADDER: &str = "\
Write recovery ladder (wqw.12):\n\
1. Prefer CodeMode zero.edit / tz_edit (find/replace hunks) under allowed roots.\n\
2. Confirm path is under effective allowed roots (doctor / resource://tokenzero/roots).\n\
3. If the CodeMode write substrate is down, harnesses may use a bounded native Write \
only for this failure (explicit ack); do not default to native Write while CodeMode works.\n\
4. Record the field issue: tz_report_tool_issue tool=zero_execute|tz_edit summary=<error>.\n\
Env escape (optional): TOKENZERO_WRITE_ESCAPE=1 acknowledges substrate-down native Write is allowed for this session.";

/// Env that enables the optional write-escape acknowledgment in error text.
pub const WRITE_ESCAPE_ENV: &str = "TOKENZERO_WRITE_ESCAPE";

pub fn write_escape_enabled() -> bool {
    match std::env::var(WRITE_ESCAPE_ENV) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        ),
        Err(_) => false,
    }
}

/// Annotate a mutation failure message with the recovery ladder.
pub fn annotate_write_failure(message: &str, substrate_down: bool) -> String {
    let mut out = message.trim().to_string();
    if out.is_empty() {
        out = "CodeMode write/edit failed".to_string();
    }
    // Avoid double-appending.
    if out.contains("Write recovery ladder") {
        return out;
    }
    out.push_str("\n\n");
    out.push_str(WRITE_RECOVERY_LADDER);
    if substrate_down || write_escape_enabled() {
        out.push_str(
            "\n\nwrite_escape_ack: substrate-down or TOKENZERO_WRITE_ESCAPE=1 — \
bounded native Write is allowed for this failure only (not default routing).",
        );
    }
    out
}

/// True when surface health says recovery is unlocked (expand/read unhealthy)
/// or explicit write-escape env is set — used only for ladder ack text, not to
/// permanently weaken write crash-only policy.
pub fn write_escape_ack_active(surface_recovery_unlocked: bool) -> bool {
    surface_recovery_unlocked || write_escape_enabled()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_appended_once() {
        let msg = annotate_write_failure("path is outside allowed roots: /x", false);
        assert!(msg.contains("outside allowed roots"));
        assert!(msg.contains("Write recovery ladder"));
        assert!(msg.contains("tz_report_tool_issue"));
        assert!(msg.contains("zero.edit"));
        let again = annotate_write_failure(&msg, false);
        assert_eq!(
            again.matches("Write recovery ladder").count(),
            1,
            "must not double-append"
        );
    }

    #[test]
    fn substrate_down_adds_escape_ack() {
        let msg = annotate_write_failure("edit failed: EACCES", true);
        assert!(msg.contains("write_escape_ack"));
        assert!(msg.contains("bounded native Write"));
    }

    #[test]
    fn ladder_mentions_not_only_use_codemode() {
        let msg = annotate_write_failure("use CodeMode", false);
        assert!(msg.contains("native Write"));
        assert!(
            msg.contains("only if")
                || msg.contains("only for this failure")
                || msg.contains("substrate")
        );
    }
}
