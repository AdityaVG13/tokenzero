use super::*;
use proptest::prelude::*;


pub(crate) fn success_input<'a>(command: &'a str, stdout: &'a str, stderr: &'a str) -> ShellRenderInput<'a> {
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
