use tokenzero_core::{Mode, ShellRender, ShellRenderInput, render_shell};

fn render(command: &str, stdout: &str, stderr: &str, code: i32, success: bool) -> ShellRender {
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout,
        stderr,
        exit_code: Some(code),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });
    let status = &rendered.command_status;
    assert_eq!(status.command_success, success, "{command}: {rendered:?}");
    let expected_label = if success {
        "command_success"
    } else {
        "command_failed"
    };
    assert_eq!(
        status.status_label, expected_label,
        "{command}: {rendered:?}"
    );
    assert_eq!(status.exit_code, Some(code));
    if success {
        assert!(status.failed_segment.is_none(), "{command}: {rendered:?}");
    }
    rendered
}
fn success(command: &str, stdout: &str, code: i32) -> ShellRender {
    render(command, stdout, "", code, true)
}
fn failure(command: &str, stdout: &str, stderr: &str, code: i32) -> ShellRender {
    render(command, stdout, stderr, code, false)
}
fn no_masking(rendered: &ShellRender, command: &str) {
    let status = &rendered.command_status;
    for diagnostic in [
        &status.pipeline_masking_warning,
        &status.pipeline_rerun_command,
    ] {
        assert!(diagnostic.is_none(), "{command}: {rendered:?}");
    }
}
fn masking(rendered: &ShellRender, command: &str, segment: &str) {
    let status = &rendered.command_status;
    assert_eq!(
        status.failed_segment.as_deref(),
        Some(segment),
        "{command}: {rendered:?}"
    );
    assert!(
        status
            .pipeline_masking_warning
            .as_deref()
            .is_some_and(|warning| warning.contains("mask")),
        "{command}: {rendered:?}"
    );
}
macro_rules! tests { ($($name:ident $body:block)*) => { $(#[test] fn $name() $body)* }; }
macro_rules! ok { ($($command:expr => ($stdout:expr, $code:expr);)*) => { $(success($command, $stdout, $code);)* }; }
macro_rules! err { ($($command:expr => ($stdout:expr, $stderr:expr, $code:expr);)*) => { $(failure($command, $stdout, $stderr, $code);)* }; }
macro_rules! clean { ($($command:expr => $stdout:expr;)*) => { $(let rendered = success($command, $stdout, 0); no_masking(&rendered, $command);)* }; }
macro_rules! masked { ($($command:expr => ($stdout:expr, $stderr:expr, $code:expr, $segment:expr);)*) => { $(let rendered = failure($command, $stdout, $stderr, $code); masking(&rendered, $command, $segment);)* }; }

tests! {
false_predicate_exit_one_is_not_a_command_failure {
    ok! { "test -f missing-file" => ("", 1); "[ -f missing-file ]" => ("", 1); "cmp -s left.txt right.txt" => ("", 1); "cmp left.txt right.txt" => ("left.txt right.txt differ: byte 1, line 1\n", 1); "diff --quiet left.txt right.txt" => ("", 1); "diff left.txt right.txt" => ("1c1\n< left\n---\n> right\n", 1); "git diff --quiet" => ("", 1); "git diff --exit-code" => ("diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n", 1); "git -C repo diff --quiet" => ("", 1); "git -c color.ui=false diff --exit-code" => ("diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n", 1); }
    err! { "test -f" => ("", "test: missing argument\n", 2); "[ -f missing-file" => ("", "[: missing `]`'\n", 2); "cmp -s missing-a missing-b" => ("", "cmp: missing-a: No such file or directory\n", 2); "diff --quiet missing-a missing-b" => ("", "diff: missing-a: No such file or directory\n", 2); "git diff --check" => ("", "error: trailing whitespace\n", 1); "cargo test failing_filter" => ("", "error: test failed\n", 101); }
}
masked_expected_false_or_list_is_not_a_failure_or_warning {
    clean! { "test -f missing-file || true" => ""; "[ -f missing-file ] || true" => ""; "rg definitely_absent_tokenzero_pattern . || true" => ""; "cmp left.txt right.txt || true" => "left.txt right.txt differ: byte 1, line 1\n"; "diff -q left.txt right.txt || true" => "Files left.txt and right.txt differ\n"; "git diff --quiet || true" => ""; "git diff --exit-code || true" => "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"; }
}
successful_stdout_diagnostic_words_are_not_failure_evidence {
    for (command, stdout, family) in [("printf 'not found\n'", "not found\n", "generic"), ("rg -n 'a|b' src", "src/lib.rs:1:error: command not found marker\n", "search")] { let rendered = success(command, stdout, 0); no_masking(&rendered, command); assert_eq!(rendered.policy.family, family, "{command}"); if family == "search" { assert_eq!(rendered.policy.policy, "structured"); assert_eq!(rendered.command_status.shell_syntax_summary, "argv/simple"); } }
}
masked_hard_failures_stay_failures {
    masked! { "missing-tool; true" => ("", "sh: missing-tool: command not found\n", 0, "missing-tool"); "rg '[' 2>&1 | head" => ("rg: regex parse error:\n    (?:[)\n       ^\nerror: unclosed character class\n", "", 0, "rg '[' 2>&1"); "cmp -s missing-a missing-b || true" => ("", "cmp: missing-a: No such file or directory\n", 0, "cmp -s missing-a missing-b"); "diff --definitely-not-a-tokenzero-option || true" => ("", "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n", 0, "diff --definitely-not-a-tokenzero-option"); "cargo test missing_filter || true" => ("", "error: test failed\n", 0, "cargo test missing_filter"); "failing_tool 2>&1 | head" => ("error: something broke\n", "", 0, "failing_tool 2>&1"); }
}
expected_false_pipeline_segments_do_not_trigger_masking_warnings {
    macro_rules! expected { ($($command:expr => $stdout:expr;)*) => { $(let rendered = success($command, $stdout, 0); no_masking(&rendered, $command); assert_eq!(rendered.command_status.shell_syntax_summary, "pipeline", "{}: {:?}", $command, rendered);)* }; }
    expected! { "test -f missing-file | cat" => ""; "rg definitely_absent_tokenzero_pattern . | cat" => ""; "cmp left.txt right.txt | cat" => "left.txt right.txt differ: byte 1, line 1\n"; "diff -q left.txt right.txt | cat" => "Files left.txt and right.txt differ\n"; "git diff --exit-code | cat" => "diff --git a/file b/file\n@@ -1 +1 @@\n-left\n+right\n"; }
    macro_rules! hard { ($($command:expr => ($stdout:expr, $stderr:expr, $code:expr, $segment:expr);)*) => { $(let rendered = failure($command, $stdout, $stderr, $code); masking(&rendered, $command, $segment); assert_eq!(rendered.command_status.pipeline_rerun_command.is_some(), !cfg!(windows), "{}: {:?}", $command, rendered);)* }; }
    hard! { "cmp -s missing-a missing-b | cat" => ("", "cmp: missing-a: No such file or directory\n", 0, "cmp -s missing-a missing-b"); "diff --definitely-not-a-tokenzero-option | cat" => ("", "diff: unrecognized option `--definitely-not-a-tokenzero-option'\nusage: diff [options] file1 file2\n", 0, "diff --definitely-not-a-tokenzero-option"); "cargo test missing_filter | cat" => ("", "error: test failed\n", 0, "cargo test missing_filter"); "test -f missing-file | false" => ("", "", 1, "false"); "cmp left.txt right.txt | false" => ("left.txt right.txt differ: byte 1, line 1\n", "", 1, "false"); "false | test -f missing-file" => ("", "", 1, "false"); }
}
handled_command_lookup_does_not_poison_later_help_output {
    let command = "command -v herdr || true; herdr --help 2>&1 | sed -n '1,180p'";
    let stdout = "/opt/homebrew/bin/herdr\nUsage: herdr [options]\n";
    let rendered = success(command, stdout, 0);
    no_masking(&rendered, command);

    let command = "command --definitely-invalid herdr || true; herdr --help 2>&1 | sed -n '1,180p'";
    let rendered = failure(command, "", "command: --definitely-invalid: invalid option\n", 0);
    masking(&rendered, command, "command --definitely-invalid herdr");

    let command = "command -v -x herdr || true; herdr --help 2>&1 | sed -n '1,180p'";
    let rendered = failure(command, "Usage: command [-pVv] command [arg ...]\n", "", 0);
    masking(&rendered, command, "command -v -x herdr");
}
compound_command_status_evidence_is_preserved {
    let command = "cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check && git diff --check"; let rendered = failure(command, "", "error: this `if` statement can be collapsed\n", 101); assert_eq!(rendered.command_status.failed_segment.as_deref(), Some("cargo clippy --workspace --all-targets -- -D warnings"), "{rendered:?}");
    let command = "cd /tmp/tokenzero && grep -rn \"failed_segment\" crates/tokenzero-core/src/shell_parse.rs | head"; let rendered = success(command, "crates/tokenzero-core/src/shell_parse.rs:93:    if looks_masked_failure_evidence(stdout, stderr, Some(segment)) {\n", 0); no_masking(&rendered, command);
    let command = "rg -n 'error:' src"; let rendered = success(command, "src/lib.rs:10:error: legacy marker in comment\n", 0); no_masking(&rendered, command);
}
env_chdir_failure_is_not_reported_as_inner_shell_pipeline_failure {
    for command in ["env -C missing-dir bash -lc 'false | true'", "env --chdir=missing-dir bash -lc 'false | true'"] { let rendered = failure(command, "", "env: cannot change directory to 'missing-dir': No such file or directory\n", 125); assert_eq!(rendered.command_status.failed_segment.as_deref(), Some(command), "{command}: {rendered:?}"); no_masking(&rendered, command); assert_eq!(rendered.command_status.shell_syntax_summary, "argv/simple", "{command}: {rendered:?}"); }
}
}
