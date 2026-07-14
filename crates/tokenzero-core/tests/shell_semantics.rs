use tokenzero_core::{Mode, ShellRender, ShellRenderInput, render_shell};

fn render(command: &str, stdout: &str, stderr: &str, exit_code: Option<i32>) -> ShellRender {
    render_shell(ShellRenderInput {
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
    })
}

fn success(command: &str, stdout: &str, exit_code: i32) -> ShellRender {
    let rendered = render(command, stdout, "", Some(exit_code));
    assert!(rendered.command_status.command_success, "{command}: {rendered:?}");
    assert_eq!(rendered.command_status.status_label, "command_success", "{command}: {rendered:?}");
    assert_eq!(rendered.command_status.exit_code, Some(exit_code));
    assert!(rendered.command_status.failed_segment.is_none(), "{command}: {rendered:?}");
    rendered
}

fn failure(command: &str, stdout: &str, stderr: &str, exit_code: i32) -> ShellRender {
    let rendered = render(command, stdout, stderr, Some(exit_code));
    assert!(!rendered.command_status.command_success, "{command}: {rendered:?}");
    assert_eq!(rendered.command_status.status_label, "command_failed", "{command}: {rendered:?}");
    assert_eq!(rendered.command_status.exit_code, Some(exit_code));
    rendered
}

fn assert_no_masking(rendered: &ShellRender, command: &str) {
    assert!(rendered.command_status.pipeline_masking_warning.is_none(), "{command}: {rendered:?}");
    assert!(rendered.command_status.pipeline_rerun_command.is_none(), "{command}: {rendered:?}");
}

fn assert_masking(rendered: &ShellRender, command: &str, segment: &str) {
    assert_eq!(rendered.command_status.failed_segment.as_deref(), Some(segment), "{command}: {rendered:?}");
    assert!(rendered.command_status.pipeline_masking_warning.as_deref().is_some_and(|warning| warning.contains("mask")), "{command}: {rendered:?}");
}

#[test]
fn false_predicate_exit_one_is_not_a_command_failure() {
    for (command, stdout) in [
        ("test -f missing-file", ""),
        ("[ -f missing-file ]", ""),
        ("cmp -s left.txt right.txt", ""),
        ("cmp left.txt right.txt", "left.txt right.txt differ: byte 1, line 1\n"),
        ("diff --quiet left.txt right.txt", ""),
        ("diff left.txt right.txt", "1c1\n< left\n---\n> right\n"),
        ("git diff --quiet", ""),
        ("git diff --exit-code", "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"),
        ("git -C repo diff --quiet", ""),
        ("git -c color.ui=false diff --exit-code", "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"),
    ] {
        success(command, stdout, 1);
    }
    for (command, stdout, stderr, code) in [
        ("test -f", "", "test: missing argument\n", 2),
        ("[ -f missing-file", "", "[: missing `]'\n", 2),
        ("cmp -s missing-a missing-b", "", "cmp: missing-a: No such file or directory\n", 2),
        ("diff --quiet missing-a missing-b", "", "diff: missing-a: No such file or directory\n", 2),
        ("git diff --check", "", "error: trailing whitespace\n", 1),
        ("cargo test failing_filter", "", "error: test failed\n", 101),
    ] {
        failure(command, stdout, stderr, code);
    }
}

#[test]
fn masked_expected_false_or_list_is_not_a_failure_or_warning() {
    for (command, stdout) in [
        ("test -f missing-file || true", ""),
        ("[ -f missing-file ] || true", ""),
        ("rg definitely_absent_tokenzero_pattern . || true", ""),
        ("cmp left.txt right.txt || true", "left.txt right.txt differ: byte 1, line 1\n"),
        ("diff -q left.txt right.txt || true", "Files left.txt and right.txt differ\n"),
        ("git diff --quiet || true", ""),
        ("git diff --exit-code || true", "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"),
    ] {
        let rendered = success(command, stdout, 0);
        assert_no_masking(&rendered, command);
    }
}

#[test]
fn successful_stdout_diagnostic_words_are_not_failure_evidence() {
    for (command, stdout, family) in [
        ("printf 'not found\n'", "not found\n", "generic"),
        ("rg -n 'a|b' src", "src/lib.rs:1:error: command not found marker\n", "search"),
    ] {
        let rendered = success(command, stdout, 0);
        assert_no_masking(&rendered, command);
        assert_eq!(rendered.policy.family, family, "{command}");
        if family == "search" {
            assert_eq!(rendered.policy.policy, "structured", "{command}");
            assert_eq!(rendered.command_status.shell_syntax_summary, "argv/simple", "{command}: {rendered:?}");
        }
    }
}

#[test]
fn masked_hard_failures_stay_failures() {
    for (command, stdout, stderr, code, segment) in [
        ("missing-tool; true", "", "sh: missing-tool: command not found\n", 0, "missing-tool"),
        ("rg '[' 2>&1 | head", "rg: regex parse error:\n    (?:[)\n       ^\nerror: unclosed character class\n", "", 0, "rg '[' 2>&1"),
        ("cmp -s missing-a missing-b || true", "", "cmp: missing-a: No such file or directory\n", 0, "cmp -s missing-a missing-b"),
        ("diff --definitely-not-a-tokenzero-option || true", "", "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n", 0, "diff --definitely-not-a-tokenzero-option"),
        ("cargo test missing_filter || true", "", "error: test failed\n", 0, "cargo test missing_filter"),
        ("failing_tool 2>&1 | head", "error: something broke\n", "", 0, "failing_tool 2>&1"),
    ] {
        let rendered = failure(command, stdout, stderr, code);
        assert_masking(&rendered, command, segment);
    }
}

#[test]
fn expected_false_pipeline_segments_do_not_trigger_masking_warnings() {
    for (command, stdout) in [
        ("test -f missing-file | cat", ""),
        ("rg definitely_absent_tokenzero_pattern . | cat", ""),
        ("cmp left.txt right.txt | cat", "left.txt right.txt differ: byte 1, line 1\n"),
        ("diff -q left.txt right.txt | cat", "Files left.txt and right.txt differ\n"),
        ("git diff --exit-code | cat", "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"),
    ] {
        let rendered = success(command, stdout, 0);
        assert_no_masking(&rendered, command);
        assert_eq!(rendered.command_status.shell_syntax_summary, "pipeline", "{command}: {rendered:?}");
    }

    for (command, stdout, stderr, code, segment) in [
        ("cmp -s missing-a missing-b | cat", "", "cmp: missing-a: No such file or directory\n", 0, "cmp -s missing-a missing-b"),
        ("diff --definitely-not-a-tokenzero-option | cat", "", "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n", 0, "diff --definitely-not-a-tokenzero-option"),
        ("cargo test missing_filter | cat", "", "error: test failed\n", 0, "cargo test missing_filter"),
        ("test -f missing-file | false", "", "", 1, "false"),
        ("cmp left.txt right.txt | false", "left.txt right.txt differ: byte 1, line 1\n", "", 1, "false"),
        ("false | test -f missing-file", "", "", 1, "false"),
    ] {
        let rendered = failure(command, stdout, stderr, code);
        assert_masking(&rendered, command, segment);
        assert_eq!(rendered.command_status.pipeline_rerun_command.is_some(), !cfg!(windows), "{command}: {rendered:?}");
    }
}

#[test]
fn compound_command_status_evidence_is_preserved() {
    let command = "cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && git diff --check";
    let rendered = failure(command, "", "error: this `if` statement can be collapsed\n", 101);
    assert_eq!(rendered.command_status.failed_segment.as_deref(), Some("cargo clippy --workspace --all-targets -- -D warnings"), "{rendered:?}");

    let command = "cd /tmp/tokenzero && grep -rn \"failed_segment\" crates/tokenzero-core/src/shell_parse.rs | head";
    let rendered = success(command, "crates/tokenzero-core/src/shell_parse.rs:93:    if looks_masked_failure_evidence(stdout, stderr, Some(segment)) {\n", 0);
    assert_no_masking(&rendered, command);

    let command = "rg -n 'error:' src";
    let rendered = success(command, "src/lib.rs:10:error: legacy marker in comment\n", 0);
    assert_no_masking(&rendered, command);
}

#[test]
fn env_chdir_failure_is_not_reported_as_inner_shell_pipeline_failure() {
    for command in [
        "env -C missing-dir bash -lc 'false | true'",
        "env --chdir=missing-dir bash -lc 'false | true'",
    ] {
        let rendered = failure(command, "", "env: cannot change directory to 'missing-dir': No such file or directory\n", 125);
        assert_eq!(rendered.command_status.failed_segment.as_deref(), Some(command), "{command}: {rendered:?}");
        assert_no_masking(&rendered, command);
        assert_eq!(rendered.command_status.shell_syntax_summary, "argv/simple", "{command}: {rendered:?}");
    }
}
