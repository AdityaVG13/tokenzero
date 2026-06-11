//! Passthrough conformance for `tokenzero hook claude-code`.
//!
//! For every wrapped case the suite asserts:
//! (a) the hook emits an allow+updatedInput rewrite (or stays silent for
//!     skip cases),
//! (b) executing the REWRITTEN command through `sh -c` yields the SAME exit
//!     code as executing the original through `sh -c`, and
//! (c) where stdout matters, the original bytes are recoverable by expanding
//!     the wrapped run's `combined_ref` with `tokenzero expand`.

use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::tempdir;

fn hook_output(payload: &str, mode: Option<&str>, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::cargo_bin("tokenzero").unwrap();
    command.args(["hook", "claude-code"]);
    if let Some(mode) = mode {
        command.args(["--mode", mode]);
    }
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .env("TERM", "dumb")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn bash_payload(command: &str) -> String {
    json!({
        "session_id": "conformance",
        "cwd": "/tmp",
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": command, "description": "conformance case"}
    })
    .to_string()
}

/// The hook must emit an allow rewrite, preserve sibling tool_input keys,
/// and exit 0.
fn rewritten_command(original: &str) -> String {
    let output = hook_output(&bash_payload(original), None, &[]);
    assert!(
        output.status.success(),
        "hook failed for {original:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let decision: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|err| panic!("non-JSON hook output for {original:?}: {err}"));
    let hook_output = &decision["hookSpecificOutput"];
    assert_eq!(hook_output["hookEventName"], "PreToolUse");
    assert_eq!(hook_output["permissionDecision"], "allow");
    let updated = &hook_output["updatedInput"];
    assert_eq!(
        updated["description"], "conformance case",
        "sibling tool_input keys must survive the rewrite"
    );
    updated["command"].as_str().unwrap().to_string()
}

fn assert_passthrough(original: &str) {
    let output = hook_output(&bash_payload(original), None, &[]);
    assert!(output.status.success(), "hook must exit 0 for {original:?}");
    assert!(
        output.stdout.is_empty(),
        "expected no decision for {original:?}, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn run_sh(command: &str, cwd: &Path) -> Output {
    Command::new("sh")
        .args(["-c", command])
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .env("CI", "true")
        .env("TERM", "dumb")
        .output()
        .unwrap()
}

/// Runs the original and the rewritten command through `sh -c` in the same
/// temp dir and asserts exit-code parity. Returns (original, wrapped, dir)
/// for content checks.
fn exit_parity(original: &str) -> (Output, Output, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let rewritten = rewritten_command(original);
    let original_output = run_sh(original, dir.path());
    let wrapped_output = run_sh(&rewritten, dir.path());
    assert_eq!(
        original_output.status.code(),
        wrapped_output.status.code(),
        "exit-code parity broken for {original:?}\nrewritten: {rewritten}\nwrapped stdout:\n{}\nwrapped stderr:\n{}",
        String::from_utf8_lossy(&wrapped_output.stdout),
        String::from_utf8_lossy(&wrapped_output.stderr)
    );
    (original_output, wrapped_output, dir)
}

fn combined_ref(capsule: &str) -> Option<String> {
    capsule
        .lines()
        .find_map(|line| line.split("combined_ref:").nth(1))
        .map(|reference| reference.trim().to_string())
}

fn expand_ref(reference: &str, cwd: &Path) -> String {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["expand", reference, "--raw"])
        .current_dir(cwd)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expand {reference} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Content recovery: expand the capsule's combined_ref when present. The
/// token-adaptive render emits raw bytes with no capsule for small successful
/// runs (a capsule would cost more than the output itself); in that case the
/// wrapped stdout must carry the original bytes directly.
fn assert_bytes_recoverable(expected: &str, wrapped: &Output, cwd: &Path) {
    let capsule = String::from_utf8_lossy(&wrapped.stdout).to_string();
    match combined_ref(&capsule) {
        Some(reference) => {
            let recovered = expand_ref(&reference, cwd);
            assert!(
                recovered.contains(expected),
                "original bytes not recoverable\nexpected:\n{expected}\nrecovered:\n{recovered}\ncapsule:\n{capsule}"
            );
        }
        // Capsule-less passthrough must be byte-exact, not merely a
        // substring: the raw render IS the original stdout (modulo the
        // trailing newline the render trims).
        None => {
            assert_eq!(
                capsule.trim_end_matches('\n'),
                expected.trim_end_matches('\n'),
                "passthrough render diverged from original stdout"
            );
        }
    }
}

/// Full conformance for output-bearing commands: exit parity plus original
/// stdout bytes recoverable from the wrapped run.
fn assert_output_parity(original: &str) {
    let (original_output, wrapped_output, dir) = exit_parity(original);
    let expected = String::from_utf8_lossy(&original_output.stdout).to_string();
    assert!(
        !expected.is_empty(),
        "output-parity case {original:?} produced no stdout to compare"
    );
    assert_bytes_recoverable(&expected, &wrapped_output, dir.path());
}

// --- exit codes ---

#[test]
fn exit_code_parity_for_true_false_and_explicit_codes() {
    for command in ["true", "false", "sh -c 'exit 3'"] {
        let (original, _, _) = exit_parity(command);
        let expected = match command {
            "true" => 0,
            "false" => 1,
            _ => 3,
        };
        assert_eq!(original.status.code(), Some(expected));
    }
}

// --- pipes ---

#[test]
fn pipe_output_and_exit_parity() {
    assert_output_parity("printf 'a\\nb\\n' | grep a");
}

#[test]
fn failing_pipe_keeps_masked_exit_zero() {
    // sh reports the last segment's status; the wrapper must mirror that, not
    // resurface the masked upstream failure as a nonzero exit.
    let (original, _, _) = exit_parity("false | cat");
    assert_eq!(original.status.code(), Some(0));
}

// --- && and ; ---

#[test]
fn and_chain_parity() {
    assert_output_parity("echo one && echo two");
}

#[test]
fn semicolon_sequence_parity() {
    assert_output_parity("echo a; echo b");
}

// --- quoting hells ---

#[test]
fn single_quote_parity() {
    assert_output_parity("echo 'single quoted text'");
}

#[test]
fn double_quote_with_embedded_single_quote_parity() {
    assert_output_parity("echo \"it's quoted\"");
}

#[test]
fn dollar_home_expands_at_run_time_not_earlier() {
    assert_output_parity("echo \"$HOME\"");
}

#[test]
fn shell_variables_resolve_inside_the_wrapper_not_before() {
    // If any layer expanded variables before the inner sh, $TZ_CONF would be
    // empty and the recovered output would not contain "inner-value".
    assert_output_parity("TZ_CONF=inner-value; echo \"$TZ_CONF\"");
}

#[test]
fn backticks_in_single_quotes_stay_literal() {
    let (original_output, wrapped_output, dir) = exit_parity("echo 'tick `date` tock'");
    assert_eq!(
        String::from_utf8_lossy(&original_output.stdout),
        "tick `date` tock\n"
    );
    assert_bytes_recoverable("tick `date` tock", &wrapped_output, dir.path());
}

#[test]
fn unicode_parity() {
    assert_output_parity("echo '— 日本語'");
}

#[test]
fn embedded_newline_runs_both_lines() {
    let (original_output, wrapped_output, dir) = exit_parity("echo first\necho second");
    assert_eq!(
        String::from_utf8_lossy(&original_output.stdout),
        "first\nsecond\n"
    );
    assert_bytes_recoverable("first\nsecond", &wrapped_output, dir.path());
}

/// Forces the capsule render (output large enough that raw passthrough loses)
/// and proves the exact bytes come back through `tokenzero expand` of the
/// combined_ref.
#[test]
fn large_output_capsule_recovers_exact_bytes_via_combined_ref() {
    let (original_output, wrapped_output, dir) = exit_parity("seq 1 5000");
    assert_eq!(original_output.status.code(), Some(0));
    let capsule = String::from_utf8_lossy(&wrapped_output.stdout).to_string();
    let reference = combined_ref(&capsule)
        .unwrap_or_else(|| panic!("expected a capsule with refs for large output:\n{capsule}"));
    let recovered = expand_ref(&reference, dir.path());
    assert!(recovered.contains("1\n2\n3\n"), "{recovered}");
    assert!(recovered.contains("4999\n5000"), "{recovered}");
}

// --- stderr ---

#[test]
fn stderr_and_exit_code_parity() {
    let (original_output, wrapped_output, dir) = exit_parity("sh -c 'echo err >&2; exit 4'");
    assert_eq!(original_output.status.code(), Some(4));
    assert_bytes_recoverable("err", &wrapped_output, dir.path());
}

// --- skip cases stay unwrapped ---

#[test]
fn skip_cases_pass_through_unwrapped() {
    for command in [
        "cd /tmp && ls",
        // Persistent-shell state in ANY top-level segment desyncs the
        // harness's cwd/env tracking if wrapped.
        "make && cd build && ctest",
        "git clone x && cd x && npm install",
        "export FOO=1 && make",
        "echo bg &",
        // Mid-command background jobs: the wrapper's pipe readers would
        // block until the shell deadline, then kill the background child.
        "server -d & sleep 1 && curl localhost:8080",
        "cat <<EOF\nheredoc body\nEOF",
        "vim file",
        "make && vim notes.txt",
        "tokenzero doctor --json",
        "echo this mentions tokenzero",
        "TOKENZERO_NO_WRAP=1 npm test",
        "",
        "   ",
    ] {
        assert_passthrough(command);
    }
}

#[test]
fn quoted_operators_and_redirects_still_wrap() {
    for command in [
        "echo 'a & b'",
        "echo \"a & b\"",
        "cargo test 2>&1",
        "cdk deploy && ls",
    ] {
        let output = hook_output(&bash_payload(command), None, &[]);
        assert!(output.status.success());
        assert!(
            !output.stdout.is_empty(),
            "expected a rewrite decision for {command:?}"
        );
    }
}

// --- hook robustness ---

#[test]
fn malformed_json_exits_zero_with_no_output() {
    let output = hook_output("{this is not json", None, &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn non_bash_tool_exits_zero_with_no_output() {
    let payload = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Read",
        "tool_input": {"file_path": "/tmp/x"}
    })
    .to_string();
    let output = hook_output(&payload, None, &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn missing_tool_input_exits_zero_with_no_output() {
    let payload = json!({"tool_name": "Bash"}).to_string();
    let output = hook_output(&payload, None, &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn no_wrap_env_disables_rewrites() {
    let output = hook_output(&bash_payload("true"), None, &[("TOKENZERO_NO_WRAP", "1")]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn no_wrap_env_zero_keeps_wrapping_on() {
    let output = hook_output(&bash_payload("true"), None, &[("TOKENZERO_NO_WRAP", "0")]);
    assert!(output.status.success());
    assert!(
        !output.stdout.is_empty(),
        "TOKENZERO_NO_WRAP=0 must keep wrapping enabled"
    );
}

// --- modes ---

#[test]
fn guide_mode_denies_with_tokenzero_steer() {
    let output = hook_output(&bash_payload("true"), Some("guide"), &[]);
    assert!(output.status.success());
    let decision: Value = serde_json::from_slice(&output.stdout).unwrap();
    let hook_output = &decision["hookSpecificOutput"];
    assert_eq!(hook_output["hookEventName"], "PreToolUse");
    assert_eq!(hook_output["permissionDecision"], "deny");
    assert!(
        hook_output["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("TokenZero")
    );
    assert!(hook_output.get("updatedInput").is_none());
}

#[test]
fn off_mode_always_passes_through() {
    let output = hook_output(&bash_payload("true"), Some("off"), &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn unknown_mode_fails_open_to_passthrough() {
    let output = hook_output(&bash_payload("true"), Some("rewirte"), &[]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}
