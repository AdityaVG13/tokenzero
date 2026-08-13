use super::*;

pub(crate) fn success_input<'a>(
    command: &'a str,
    stdout: &'a str,
    stderr: &'a str,
) -> ShellRenderInput<'a> {
    ShellRenderInput {
        command,
        stdout,
        stderr,
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    }
}

/// Assert the rendered shell's `command_status` matches expectations.
/// Pass `None` for fields you don't want to check.
pub(crate) fn assert_shell_status(
    rendered: &ShellRender,
    expected_success: bool,
    expected_exit_code: Option<i32>,
    expected_label: Option<&str>,
    expected_failed_segment: Option<Option<&str>>,
) {
    assert_eq!(
        rendered.command_status.command_success, expected_success,
        "command_success mismatch: status={:?}",
        rendered.command_status,
    );
    assert_eq!(
        rendered.command_status.exit_code, expected_exit_code,
        "exit_code mismatch: status={:?}",
        rendered.command_status,
    );
    if let Some(label) = expected_label {
        assert_eq!(
            rendered.command_status.status_label, label,
            "status_label mismatch: status={:?}",
            rendered.command_status,
        );
    }
    if let Some(segment) = expected_failed_segment {
        assert_eq!(
            rendered.command_status.failed_segment.as_deref(),
            segment,
            "failed_segment mismatch: status={:?}",
            rendered.command_status,
        );
    }
}
