use crate::render::domain::{
    is_expected_false_exit, is_repo_inventory_command, is_search_no_match,
};
use crate::shell_family::shell_family;
use crate::shell_parse::{
    failed_segment, looks_diagnostic, masking_warning, pipeline_rerun_command, repeated_line_count,
    shell_syntax_summary_for_status,
};
use crate::{CommandStatus, Mode, PolicyDecision};

/// Whether a shell command should be treated as successful for status purposes.
///
/// Contract:
/// - Exit code 0, search-no-match, and expected-false exits count as success
///   unless the command timed out.
/// - A detected `failed_segment` together with exit code 0 overrides success
///   (masked pipeline/OR-list failure semantics).
pub(crate) fn command_succeeded(
    exit_code: Option<i32>,
    search_no_match: bool,
    expected_false_exit: bool,
    timed_out: bool,
    failed_segment: Option<&str>,
) -> bool {
    if timed_out {
        return false;
    }
    if failed_segment.is_some() && exit_code == Some(0) {
        return false;
    }
    exit_code == Some(0) || search_no_match || expected_false_exit
}

fn command_status_label(
    exit_code: Option<i32>,
    timed_out: bool,
    command_success: bool,
) -> &'static str {
    if timed_out {
        "command_timeout"
    } else if command_success {
        "command_success"
    } else if exit_code.is_none() {
        "command_unknown"
    } else {
        "command_failed"
    }
}

fn is_diagnostic_shell_policy(
    family: &str,
    combined: &str,
    exit_code: Option<i32>,
    status_hazard: bool,
) -> bool {
    matches!(
        family,
        "test" | "build" | "lint" | "python-test" | "go-test"
    ) || status_hazard
        || exit_code.is_some_and(|code| code != 0)
        || looks_diagnostic(combined)
}

/// Auto-mode shell policy selection from command context.
pub(crate) fn auto_shell_policy(
    command: &str,
    family: &str,
    combined: &str,
    exit_code: Option<i32>,
    search_no_match: bool,
    expected_false_exit: bool,
    status_hazard: bool,
) -> (&'static str, &'static str) {
    if is_repo_inventory_command(command) {
        return ("structured", "repo inventory command");
    }
    if family == "diff" {
        return ("diff-aware", "diff-like output");
    }
    if family == "search" && !status_hazard && (exit_code == Some(0) || search_no_match) {
        return ("structured", "search output");
    }
    if expected_false_exit {
        return ("structured", "expected false predicate exit");
    }
    if is_diagnostic_shell_policy(family, combined, exit_code, status_hazard) {
        return ("diagnostic", "failure or diagnostic family");
    }
    if matches!(family, "search" | "structured" | "status") {
        return ("structured", "structured/status output");
    }
    if repeated_line_count(combined) >= 3 || combined.lines().count() > 120 {
        return ("dedupe", "repeated or long log");
    }
    ("passthrough", "small low-risk output")
}

pub fn decide_shell_policy(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    mode: Mode,
) -> PolicyDecision {
    let requested = mode.effective_policy();
    if requested != Mode::Auto {
        return PolicyDecision {
            policy: requested.to_string(),
            reason: "explicit user mode".to_string(),
            family: shell_family(command, stdout, stderr),
        };
    }
    let family = shell_family(command, stdout, stderr);
    let combined = format!("{stdout}\n{stderr}");
    let search_no_match = is_search_no_match(command, stdout, stderr, exit_code);
    let expected_false_exit = is_expected_false_exit(command, stdout, stderr, exit_code);
    let status_hazard = failed_segment(command, stdout, stderr, exit_code).is_some()
        || masking_warning(command, stdout, stderr, exit_code).is_some();
    let policy = auto_shell_policy(
        command,
        &family,
        &combined,
        exit_code,
        search_no_match,
        expected_false_exit,
        status_hazard,
    );
    PolicyDecision {
        policy: policy.0.to_string(),
        reason: policy.1.to_string(),
        family,
    }
}

pub fn classify_command_status(
    command: &str,
    stdout: &str,
    stderr: &str,
    exit_code: Option<i32>,
    timed_out: bool,
) -> CommandStatus {
    let search_no_match = is_search_no_match(command, stdout, stderr, exit_code);
    let expected_false_exit = is_expected_false_exit(command, stdout, stderr, exit_code);
    let failed_segment = failed_segment(command, stdout, stderr, exit_code);
    let pipeline_masking_warning = masking_warning(command, stdout, stderr, exit_code);
    let pipeline_rerun_command = pipeline_rerun_command(command, pipeline_masking_warning.as_ref());
    let command_success = command_succeeded(
        exit_code,
        search_no_match,
        expected_false_exit,
        timed_out,
        failed_segment.as_deref(),
    );
    let status_label = command_status_label(exit_code, timed_out, command_success).to_string();
    CommandStatus {
        transport_status: "ok".to_string(),
        command_success,
        exit_code,
        failed_segment,
        pipeline_masking_warning,
        pipeline_rerun_command,
        shell_syntax_summary: shell_syntax_summary_for_status(command, stdout, stderr, exit_code),
        status_label,
    }
}

pub fn shell_combined_output(
    command: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
) -> String {
    format!(
        "$ {command}\nexit_code: {}\nstdout:\n{stdout}{}{}",
        exit_code
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string()),
        if stderr.is_empty() { "" } else { "\nstderr:\n" },
        stderr
    )
}

#[cfg(test)]
mod tables;
