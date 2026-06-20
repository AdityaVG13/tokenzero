use super::{auto_shell_policy, classify_command_status, command_succeeded, decide_shell_policy};
use crate::Mode;
use crate::shell_family::shell_family;
use crate::shell_parse::{failed_segment, masking_warning};

struct CommandSucceededCase {
    exit_code: Option<i32>,
    search_no_match: bool,
    expected_false_exit: bool,
    timed_out: bool,
    failed_segment: Option<&'static str>,
    want: bool,
}

struct ClassifyStatusCase {
    command: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    exit_code: Option<i32>,
    timed_out: bool,
    want_success: bool,
    want_label: &'static str,
    want_failed_segment: Option<&'static str>,
}

struct AutoPolicyCase {
    command: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    exit_code: Option<i32>,
    family: &'static str,
    want_policy: &'static str,
    want_reason: &'static str,
}

struct DecidePolicyCase {
    command: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    exit_code: Option<i32>,
    mode: Mode,
    want_policy: &'static str,
    want_family: &'static str,
}

struct ShellFamilyCase {
    command: &'static str,
    stdout: &'static str,
    stderr: &'static str,
    want: &'static str,
}

#[test]
fn command_succeeded_table() {
    let cases = [
        CommandSucceededCase {
            exit_code: Some(0),
            search_no_match: false,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: None,
            want: true,
        },
        CommandSucceededCase {
            exit_code: Some(0),
            search_no_match: false,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: Some("false"),
            want: false,
        },
        CommandSucceededCase {
            exit_code: Some(1),
            search_no_match: true,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: Some("rg"),
            want: true,
        },
        CommandSucceededCase {
            exit_code: Some(0),
            search_no_match: true,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: None,
            want: true,
        },
        CommandSucceededCase {
            exit_code: Some(1),
            search_no_match: false,
            expected_false_exit: true,
            timed_out: false,
            failed_segment: None,
            want: true,
        },
        CommandSucceededCase {
            exit_code: Some(1),
            search_no_match: false,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: None,
            want: false,
        },
        CommandSucceededCase {
            exit_code: Some(0),
            search_no_match: false,
            expected_false_exit: false,
            timed_out: true,
            failed_segment: None,
            want: false,
        },
        CommandSucceededCase {
            exit_code: None,
            search_no_match: false,
            expected_false_exit: false,
            timed_out: false,
            failed_segment: None,
            want: false,
        },
    ];
    for case in cases {
        assert_eq!(
            command_succeeded(
                case.exit_code,
                case.search_no_match,
                case.expected_false_exit,
                case.timed_out,
                case.failed_segment,
            ),
            case.want,
            "exit={:?} search_no_match={} expected_false={} timed_out={} failed_segment={:?}",
            case.exit_code,
            case.search_no_match,
            case.expected_false_exit,
            case.timed_out,
            case.failed_segment,
        );
    }
}

#[test]
fn classify_command_status_table() {
    let cases = [
        ClassifyStatusCase {
            command: "true",
            stdout: "ok\n",
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            want_success: true,
            want_label: "command_success",
            want_failed_segment: None,
        },
        ClassifyStatusCase {
            command: "false",
            stdout: "",
            stderr: "",
            exit_code: Some(1),
            timed_out: false,
            want_success: false,
            want_label: "command_failed",
            want_failed_segment: Some("false"),
        },
        ClassifyStatusCase {
            command: "false | true",
            stdout: "",
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            want_success: false,
            want_label: "command_failed",
            want_failed_segment: Some("false"),
        },
        ClassifyStatusCase {
            command: "rg -n missing crates/",
            stdout: "",
            stderr: "",
            exit_code: Some(1),
            timed_out: false,
            want_success: true,
            want_label: "command_success",
            want_failed_segment: None,
        },
        ClassifyStatusCase {
            command: "sleep 60",
            stdout: "",
            stderr: "",
            exit_code: None,
            timed_out: true,
            want_success: false,
            want_label: "command_timeout",
            want_failed_segment: None,
        },
    ];
    for case in cases {
        let status = classify_command_status(
            case.command,
            case.stdout,
            case.stderr,
            case.exit_code,
            case.timed_out,
        );
        assert_eq!(
            status.command_success, case.want_success,
            "{}: {:?}",
            case.command, status
        );
        assert_eq!(
            status.status_label, case.want_label,
            "{}: {:?}",
            case.command, status
        );
        assert_eq!(
            status.failed_segment.as_deref(),
            case.want_failed_segment,
            "{}: {:?}",
            case.command,
            status
        );
    }
}

#[test]
fn auto_shell_policy_table() {
    let cases = [
        AutoPolicyCase {
            command: "find . -type f | sort | wc -l",
            stdout: "2\n",
            stderr: "",
            exit_code: Some(0),
            family: "repo-inventory",
            want_policy: "structured",
            want_reason: "repo inventory command",
        },
        AutoPolicyCase {
            command: "git diff",
            stdout: "diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n",
            stderr: "",
            exit_code: Some(0),
            family: "diff",
            want_policy: "diff-aware",
            want_reason: "diff-like output",
        },
        AutoPolicyCase {
            command: "rg tokenzero crates/",
            stdout: "src/lib.rs:1:tokenzero\n",
            stderr: "",
            exit_code: Some(0),
            family: "search",
            want_policy: "structured",
            want_reason: "search output",
        },
        AutoPolicyCase {
            command: "cargo test",
            stdout: "",
            stderr: "error: test failed",
            exit_code: Some(1),
            family: "test",
            want_policy: "diagnostic",
            want_reason: "failure or diagnostic family",
        },
        AutoPolicyCase {
            command: "printf 'tick\\ntick\\ntick\\ntick\\n'",
            stdout: "tick\ntick\ntick\ntick\n",
            stderr: "",
            exit_code: Some(0),
            family: "generic",
            want_policy: "dedupe",
            want_reason: "repeated or long log",
        },
        AutoPolicyCase {
            command: "printf ok",
            stdout: "ok\n",
            stderr: "",
            exit_code: Some(0),
            family: "generic",
            want_policy: "passthrough",
            want_reason: "small low-risk output",
        },
    ];
    for case in cases {
        let combined = format!("{}\n{}", case.stdout, case.stderr);
        let search_no_match = false;
        let expected_false_exit = false;
        let status_hazard = failed_segment(case.command, case.stdout, case.stderr, case.exit_code)
            .is_some()
            || masking_warning(case.command, case.stdout, case.stderr, case.exit_code).is_some();
        let (policy, reason) = auto_shell_policy(
            case.command,
            case.family,
            &combined,
            case.exit_code,
            search_no_match,
            expected_false_exit,
            status_hazard,
        );
        assert_eq!(policy, case.want_policy, "{}", case.command);
        assert_eq!(reason, case.want_reason, "{}", case.command);
    }
}

#[test]
fn decide_shell_policy_table() {
    let cases = [
        DecidePolicyCase {
            command: "cargo test",
            stdout: "",
            stderr: "error: failed",
            exit_code: Some(1),
            mode: Mode::Auto,
            want_policy: "diagnostic",
            want_family: "test",
        },
        DecidePolicyCase {
            command: "rg tokenzero crates/",
            stdout: "src/lib.rs:1:tokenzero\n",
            stderr: "",
            exit_code: Some(0),
            mode: Mode::Auto,
            want_policy: "structured",
            want_family: "search",
        },
        DecidePolicyCase {
            command: "printf ok",
            stdout: "ok\n",
            stderr: "",
            exit_code: Some(0),
            mode: Mode::Exact,
            want_policy: "exact",
            want_family: "generic",
        },
    ];
    for case in cases {
        let decision = decide_shell_policy(
            case.command,
            case.stdout,
            case.stderr,
            case.exit_code,
            case.mode,
        );
        assert_eq!(decision.policy, case.want_policy, "{}", case.command);
        assert_eq!(decision.family, case.want_family, "{}", case.command);
    }
}

#[test]
fn shell_family_table() {
    let cases = [
        ShellFamilyCase {
            command: "cargo test",
            stdout: "",
            stderr: "",
            want: "test",
        },
        ShellFamilyCase {
            command: "cargo build",
            stdout: "",
            stderr: "",
            want: "build",
        },
        ShellFamilyCase {
            command: "rg tokenzero",
            stdout: "",
            stderr: "",
            want: "search",
        },
        ShellFamilyCase {
            command: "git diff",
            stdout: "diff --git a/a b/a\n",
            stderr: "",
            want: "diff",
        },
        ShellFamilyCase {
            command: "pytest",
            stdout: "",
            stderr: "",
            want: "python-test",
        },
        ShellFamilyCase {
            command: "go test ./...",
            stdout: "",
            stderr: "",
            want: "go-test",
        },
        ShellFamilyCase {
            command: "docker ps",
            stdout: "",
            stderr: "",
            want: "status",
        },
        ShellFamilyCase {
            command: "printf ok",
            stdout: "ok\n",
            stderr: "",
            want: "generic",
        },
    ];
    for case in cases {
        assert_eq!(
            shell_family(case.command, case.stdout, case.stderr),
            case.want,
            "{}",
            case.command
        );
    }
}
