use tokenzero_core::{Mode, ShellRenderInput, render_shell};

#[test]
fn false_predicate_exit_one_is_not_a_command_failure() {
    for (command, stdout) in [
        ("test -f missing-file", ""),
        ("[ -f missing-file ]", ""),
        ("cmp -s left.txt right.txt", ""),
        (
            "cmp left.txt right.txt",
            "left.txt right.txt differ: byte 1, line 1\n",
        ),
        ("diff --quiet left.txt right.txt", ""),
        ("diff left.txt right.txt", "1c1\n< left\n---\n> right\n"),
        ("git diff --quiet", ""),
        (
            "git diff --exit-code",
            "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n",
        ),
        ("git -C repo diff --quiet", ""),
        (
            "git -c color.ui=false diff --exit-code",
            "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n",
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr: "",
            exit_code: Some(1),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_success",
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.failed_segment.is_none(),
            "{command}: {rendered:?}"
        );
        assert_eq!(rendered.command_status.exit_code, Some(1));
    }

    for (command, stdout, stderr, exit_code) in [
        ("test -f", "", "test: missing argument\n", Some(2)),
        ("[ -f missing-file", "", "[: missing `]'\n", Some(2)),
        (
            "cmp -s missing-a missing-b",
            "",
            "cmp: missing-a: No such file or directory\n",
            Some(2),
        ),
        (
            "diff --quiet missing-a missing-b",
            "",
            "diff: missing-a: No such file or directory\n",
            Some(2),
        ),
        (
            "git diff --check",
            "",
            "error: trailing whitespace\n",
            Some(1),
        ),
        (
            "cargo test failing_filter",
            "",
            "error: test failed\n",
            Some(101),
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr,
            exit_code,
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            !rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_failed",
            "{command}: {rendered:?}"
        );
    }
}

#[test]
fn masked_expected_false_or_list_is_not_a_failure_or_warning() {
    for (command, stdout) in [
        ("test -f missing-file || true", ""),
        ("[ -f missing-file ] || true", ""),
        ("rg definitely_absent_tokenzero_pattern . || true", ""),
        (
            "cmp left.txt right.txt || true",
            "left.txt right.txt differ: byte 1, line 1\n",
        ),
        (
            "diff -q left.txt right.txt || true",
            "Files left.txt and right.txt differ\n",
        ),
        ("git diff --quiet || true", ""),
        (
            "git diff --exit-code || true",
            "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n",
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_success",
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.failed_segment.is_none(),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_masking_warning.is_none(),
            "{command}: {rendered:?}"
        );
        assert_eq!(rendered.command_status.exit_code, Some(0));
    }
}

#[test]
fn successful_stdout_diagnostic_words_are_not_failure_evidence() {
    for (command, stdout, expected_family) in [
        ("printf 'not found\n'", "not found\n", "generic"),
        (
            "rg -n 'a|b' src",
            "src/lib.rs:1:error: command not found marker\n",
            "search",
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_success",
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.failed_segment.is_none(),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_masking_warning.is_none(),
            "{command}: {rendered:?}"
        );
        assert_eq!(rendered.policy.family, expected_family, "{command}");
        if expected_family == "search" {
            assert_eq!(rendered.policy.policy, "structured", "{command}");
            assert_eq!(
                rendered.command_status.shell_syntax_summary, "argv/simple",
                "{command}: {rendered:?}"
            );
        }
    }
}

#[test]
fn successful_shell_with_stderr_command_not_found_still_reports_failure() {
    let rendered = render_shell(ShellRenderInput {
        command: "missing-tool; true",
        stdout: "",
        stderr: "sh: missing-tool: command not found\n",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert!(!rendered.command_status.command_success);
    assert_eq!(
        rendered.command_status.failed_segment.as_deref(),
        Some("missing-tool")
    );
    assert!(rendered.command_status.pipeline_masking_warning.is_some());
}

#[test]
fn masked_hard_failures_stay_failures_even_when_or_true_exits_zero() {
    for (command, stderr, expected_segment) in [
        (
            "cmp -s missing-a missing-b || true",
            "cmp: missing-a: No such file or directory\n",
            "cmp -s missing-a missing-b",
        ),
        (
            "diff --definitely-not-a-tokenzero-option || true",
            "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n",
            "diff --definitely-not-a-tokenzero-option",
        ),
        (
            "cargo test missing_filter || true",
            "error: test failed\n",
            "cargo test missing_filter",
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout: "",
            stderr,
            exit_code: Some(0),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            !rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_failed",
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.failed_segment.as_deref(),
            Some(expected_segment),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered
                .command_status
                .pipeline_masking_warning
                .as_deref()
                .is_some_and(|warning| warning.contains("mask")),
            "{command}: {rendered:?}"
        );
    }
}

#[test]
fn expected_false_pipeline_segments_do_not_trigger_masking_warnings() {
    for (command, stdout) in [
        ("test -f missing-file | cat", ""),
        ("rg definitely_absent_tokenzero_pattern . | cat", ""),
        (
            "cmp left.txt right.txt | cat",
            "left.txt right.txt differ: byte 1, line 1\n",
        ),
        (
            "diff -q left.txt right.txt | cat",
            "Files left.txt and right.txt differ\n",
        ),
        (
            "git diff --exit-code | cat",
            "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n",
        ),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_success",
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.failed_segment.is_none(),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_masking_warning.is_none(),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_rerun_command.is_none(),
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.shell_syntax_summary, "pipeline",
            "{command}: {rendered:?}"
        );
    }

    for (command, stdout, stderr, exit_code, expected_segment) in [
        (
            "cmp -s missing-a missing-b | cat",
            "",
            "cmp: missing-a: No such file or directory\n",
            Some(0),
            "cmp -s missing-a missing-b",
        ),
        (
            "diff --definitely-not-a-tokenzero-option | cat",
            "",
            "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n",
            Some(0),
            "diff --definitely-not-a-tokenzero-option",
        ),
        (
            "cargo test missing_filter | cat",
            "",
            "error: test failed\n",
            Some(0),
            "cargo test missing_filter",
        ),
        ("test -f missing-file | false", "", "", Some(1), "false"),
        (
            "cmp left.txt right.txt | false",
            "left.txt right.txt differ: byte 1, line 1\n",
            "",
            Some(1),
            "false",
        ),
        ("false | test -f missing-file", "", "", Some(1), "false"),
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout,
            stderr,
            exit_code,
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            !rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_failed",
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.failed_segment.as_deref(),
            Some(expected_segment),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered
                .command_status
                .pipeline_masking_warning
                .as_deref()
                .is_some_and(|warning| warning.contains("mask")),
            "{command}: {rendered:?}"
        );
        // The bash pipefail rerun suggestion is suppressed on Windows.
        assert_eq!(
            rendered.command_status.pipeline_rerun_command.is_some(),
            !cfg!(windows),
            "{command}: {rendered:?}"
        );
    }
}

#[test]
fn and_list_failure_reports_diagnostic_segment_not_unreached_tail() {
    let command = "cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && git diff --check";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout: "",
        stderr: "error: this `if` statement can be collapsed\n",
        exit_code: Some(101),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert!(!rendered.command_status.command_success);
    assert_eq!(
        rendered.command_status.failed_segment.as_deref(),
        Some("cargo clippy --workspace --all-targets -- -D warnings"),
        "{rendered:?}"
    );
}

#[test]
fn env_chdir_failure_is_not_reported_as_inner_shell_pipeline_failure() {
    for command in [
        "env -C missing-dir bash -lc 'false | true'",
        "env --chdir=missing-dir bash -lc 'false | true'",
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout: "",
            stderr: "env: cannot change directory to 'missing-dir': No such file or directory\n",
            exit_code: Some(125),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert!(
            !rendered.command_status.command_success,
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.status_label, "command_failed",
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.failed_segment.as_deref(),
            Some(command),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_masking_warning.is_none(),
            "{command}: {rendered:?}"
        );
        assert!(
            rendered.command_status.pipeline_rerun_command.is_none(),
            "{command}: {rendered:?}"
        );
        assert_eq!(
            rendered.command_status.shell_syntax_summary, "argv/simple",
            "{command}: {rendered:?}"
        );
    }
}
