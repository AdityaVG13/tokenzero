//! Conformance for compound shell failure attribution: `failed_segment` must
//! name the segment that produced the controlling nonzero status, and
//! `command_success`/`exit_code` must never disagree without an explanation.

use tokenzero_core::{CommandStatus, Mode, ShellRenderInput, render_shell};

fn status(command: &str, stdout: &str, stderr: &str, code: i32) -> CommandStatus {
    let status = render_shell(ShellRenderInput {
        command,
        stdout,
        stderr,
        exit_code: Some(code),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: None,
        stderr_ref: None,
        combined_ref: None,
    })
    .command_status;
    assert_eq!(status.exit_code, Some(code), "{command}: {status:?}");
    // exit_code and command_success must stay mutually consistent: a zero exit is
    // only a failure when a named segment and a masking warning explain why.
    if code == 0 && !status.command_success {
        assert!(
            status.failed_segment.is_some() && status.pipeline_masking_warning.is_some(),
            "{command}: exit 0 failure needs a segment and a masking warning: {status:?}"
        );
    }
    if status.command_success {
        assert!(status.failed_segment.is_none(), "{command}: {status:?}");
    }
    assert_eq!(
        status.status_label,
        if status.command_success {
            "command_success"
        } else {
            "command_failed"
        },
        "{command}: {status:?}"
    );
    status
}

fn assert_attribution(
    command: &str,
    stdout: &str,
    stderr: &str,
    code: i32,
    expected_segment: Option<&str>,
) {
    let status = status(command, stdout, stderr, code);
    assert_eq!(
        status.failed_segment.as_deref(),
        expected_segment,
        "{command}: {status:?}"
    );
    assert_eq!(
        status.command_success,
        expected_segment.is_none(),
        "{command}: {status:?}"
    );
}

const TEST_FAILURE: &str = "error: test failed, to rerun pass --lib\n";
const NOT_FOUND: &str = "sh: missing-tool: command not found\n";

#[test]
fn and_list_failure_mid_chain_names_the_failing_segment() {
    assert_attribution(
        "git diff --stat && npm install && cargo test",
        "",
        TEST_FAILURE,
        101,
        Some("cargo test"),
    );
    assert_attribution(
        "git diff --stat && missing-tool",
        "",
        NOT_FOUND,
        127,
        Some("missing-tool"),
    );
    assert_attribution(
        "cargo build && cargo test",
        "",
        "test result: FAILED. 1 failed\n",
        101,
        Some("cargo test"),
    );
}

#[test]
fn sequence_with_late_failure_does_not_blame_the_earlier_success() {
    assert_attribution(
        "git diff --stat; cargo test",
        "",
        TEST_FAILURE,
        101,
        Some("cargo test"),
    );
    assert_attribution(
        "npm install; cargo test --workspace",
        "",
        "test result: FAILED. 1 failed\n",
        101,
        Some("cargo test --workspace"),
    );
    assert_attribution("true; missing-tool", "", NOT_FOUND, 0, Some("missing-tool"));
}

#[test]
fn negated_segment_owns_the_status_it_inverts() {
    // A negated grep exiting 1 means the pattern WAS found: the negation produced
    // the failure, so it must be named rather than an earlier successful segment.
    assert_attribution(
        "git diff --stat && ! grep -rn TODO src",
        "src/a.rs:1:TODO\n",
        "",
        1,
        Some("! grep -rn TODO src"),
    );
    assert_attribution(
        "cargo build && ! grep -rn TODO src",
        "src/a.rs:1:TODO\n",
        "",
        1,
        Some("! grep -rn TODO src"),
    );
    // A negated command that succeeds (pattern absent, exit 0) is a success.
    assert_attribution("! grep -q pattern file.txt", "", "", 0, None);
    assert_attribution("! cargo test", "", TEST_FAILURE, 0, None);
}

#[test]
fn pipefail_attribution_names_the_failing_stage() {
    assert_attribution(
        "set -o pipefail; cargo test | tail -5",
        "",
        TEST_FAILURE,
        101,
        Some("cargo test"),
    );
    assert_attribution(
        "bash -o pipefail -c 'cargo test | tail -5'",
        "",
        TEST_FAILURE,
        101,
        Some("cargo test"),
    );
    assert_attribution(
        "set -o pipefail; npm install | tee log.txt",
        "",
        "npm ERR! code E404\n",
        1,
        Some("npm install"),
    );
    // The stage that actually emitted the diagnostic wins, even when it is last.
    assert_attribution(
        "cargo test | tail -5",
        "",
        "tail: cannot open for reading\n",
        1,
        Some("tail -5"),
    );
    // A masked pipeline failure is attributed inside the last list element only.
    assert_attribution(
        "git diff --stat && npm install; cargo test | tail -5",
        "",
        TEST_FAILURE,
        0,
        Some("cargo test"),
    );
    assert_attribution(
        "git diff --stat | npm install",
        "",
        "npm ERR! code E404\n",
        0,
        Some("npm install"),
    );
}

#[test]
fn empty_pipefail_search_names_the_search_segment() {
    assert_attribution(
        "git diff -- src/core/raw_worker_v2.rs src/core/fs_ops.rs src/core/dispatcher.rs | grep -n -E 'timing|settle|telemetry|WorkerResult|WorkerError|details|duration|elapsed|stage' | head -260",
        "",
        "",
        1,
        Some(
            "grep -n -E 'timing|settle|telemetry|WorkerResult|WorkerError|details|duration|elapsed|stage'",
        ),
    );
    assert_attribution(
        "printf x | rg alpha | grep beta | head -1",
        "",
        "",
        1,
        Some("grep beta"),
    );
    assert_attribution(
        "printf x | grep -q x | sh -c 'exit 1'",
        "",
        "",
        1,
        Some("sh -c 'exit 1'"),
    );
}

#[test]
fn successful_compounds_report_no_failed_segment() {
    assert_attribution(
        "git diff --stat && npm install && cargo test",
        "",
        "",
        0,
        None,
    );
    assert_attribution("git diff --stat; npm install", "ok\n", "", 0, None);
    assert_attribution("cargo test | tail -5", "test result: ok\n", "", 0, None);
    // An or-list whose left side legitimately failed is recovered by the right.
    assert_attribution(
        "git diff --stat || npm ci",
        "",
        "npm ERR! code E404\n",
        0,
        None,
    );
    assert_attribution("test -f missing-file || true", "", "", 0, None);
}
