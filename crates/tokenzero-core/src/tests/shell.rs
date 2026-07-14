use super::*;

use super::support::*;

fn auto<'a>(cmd: &'a str, out: &'a str, err: &'a str, code: Option<i32>) -> ShellRenderInput<'a> {
    let mut input = success_input(cmd, out, err);
    input.exit_code = code;
    input
}

fn below_raw(cmd: &str, out: &str, err: &str, code: Option<i32>, visible: &str) {
    let raw = count_tokens(&shell_combined_output(cmd, code, out, err));
    let vis = count_tokens(visible);
    assert!(vis < raw, "visible={vis} raw={raw}\n{visible}");
}

fn has_all(visible: &str, needles: &[&str]) {
    for n in needles {
        assert!(visible.contains(n), "missing {n:?}\n{visible}");
    }
}

fn has_none(visible: &str, needles: &[&str]) {
    for n in needles {
        assert!(!visible.contains(n), "unexpected {n:?}\n{visible}");
    }
}

#[test]
fn shell_minimal_header_when_telemetry_dominates_small_success() {
    let stdout = "";
    let stderr = "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s\n";
    let command = "cargo build -p tokenzero-mcp";
    let rendered = render_shell(ShellRenderInput {
        command, stdout, stderr, exit_code: Some(0), timed_out: false, mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/bstdout"),
        stderr_ref: Some("tz://blob/bstderr"),
        combined_ref: Some("tz://blob/bcombined"),
    });
    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert_shell_status(&rendered, true, Some(0), None, None);
    assert!(rendered.visible.starts_with("# shell ok"));
    has_all(&rendered.visible, &["cargo ok in 0.21s", "combined_ref: tz://blob/bcombined"]);
    has_none(&rendered.visible, &["policy:", "command_success:"]);
    let combined = shell_combined_output(command, Some(0), stdout, stderr);
    let overhead = count_tokens(&rendered.visible).saturating_sub(count_tokens(&combined));
    assert!(overhead <= 16, "overhead={overhead}");
}

#[test]
fn shell_failures_keep_full_diagnostic_header() {
    let rendered = render_shell(ShellRenderInput {
        command: "cargo build -p nope", stdout: "", stderr: "error: package nope not found\n",
        exit_code: Some(101), timed_out: false, mode: Mode::Auto, max_visible_tokens: 4000,
        stdout_ref: None, stderr_ref: None, combined_ref: None,
    });
    assert_ne!(rendered.output_strategy, "minimal_envelope_shell");
    assert_shell_status(&rendered, false, Some(101), Some("command_failed"), None);
    assert!(rendered.visible.contains("package nope not found"));
}

#[test]
fn argv_display_command_quotes_shell_metacharacters() {
    let argv = vec!["rg".into(), "error|warning".into(), "don't panic".into()];
    let cases = [
        ("posix", "rg 'error|warning' 'don'\\''t panic'"),
        ("cmd", "rg \"error|warning\" \"don't panic\""),
        ("powershell", "rg 'error|warning' 'don''t panic'"),
    ];
    for (platform, expected) in cases {
        let actual = if platform == "posix" {
            shell_display_command_from_argv(&argv)
        } else {
            shell_display_command_from_argv_for_platform(&argv, platform)
        };
        assert_eq!(actual, expected, "{platform}");
    }
    let win = vec!["findstr".into(), "error|warning".into(), "src\\sample.txt".into()];
    assert_eq!(
        shell_display_command_from_argv_for_platform(&win, "windows"),
        "findstr \"error|warning\" src\\sample.txt"
    );
}

#[test]
fn shell_render_exposes_status_truth_and_refs() {
    let rendered = render_shell(auto("false | true", "", "", Some(0)));
    assert_eq!(rendered.policy.policy, "diagnostic");
    assert_shell_status(&rendered, false, Some(0), Some("command_failed"), Some(Some("false")));
    assert!(rendered.command_status.pipeline_masking_warning.as_deref().unwrap().contains("mask"));
    assert!(rendered.visible.contains("combined_ref: tz://blob/combined"));
}

#[test]
fn tiny_success_shell_uses_compact_adaptive_view() {
    let rendered = render_shell(auto("npm --version", "11.12.1\n", "", Some(0)));
    assert_eq!(rendered.policy.policy, "passthrough");
    assert_eq!(rendered.output_strategy, "compact_adaptive_shell");
    assert_eq!(rendered.visible, "11.12.1");
    assert!(!rendered.visible.contains("# shell"));
    assert!(count_tokens(&rendered.visible) <= count_tokens(&shell_combined_output("npm --version", Some(0), "11.12.1\n", "")));
}

#[test]
fn search_shell_family_matrix() {
    const HIT: &str = "crates/tokenzero-core/src/lib.rs:42:error: tokenzero\n";
    let cases: &[(&str, &str, i32, &str, bool)] = &[
        ("rg -P '(?=tokenzero)' crates", HIT, 0, "matches_seen: 1", true),
        ("bash -lc 'rg -P \"(?=tokenzero)\" crates'", HIT, 0, "matches_seen: 1", true),
        ("rg -P '(?=missing)' crates", "", 1, "matches_seen: 0", false),
    ];
    for &(command, stdout, code, matches_seen, keep_hit) in cases {
        let r = render_shell(auto(command, stdout, "", Some(code)));
        assert_eq!(r.policy.family, "search", "{command}");
        assert_eq!(r.policy.policy, "structured", "{command}");
        assert!(r.command_status.command_success, "{command}");
        assert_eq!(r.command_status.status_label, "command_success", "{command}");
        has_all(&r.visible, &["search_summary", matches_seen]);
        if keep_hit { assert!(r.visible.contains(HIT.trim_end()), "{command}"); }
        else { assert!(r.command_status.failed_segment.is_none()); }
    }
}

#[test]
fn rg_quoted_regex_metacharacters_do_not_trigger_shell_warnings() {
    let r = render_shell(auto("rg 'error|warning' crates", "crates/tokenzero-core/src/lib.rs:42:error: tokenzero\n", "", Some(0)));
    assert_eq!(r.policy.family, "search");
    assert_eq!(r.policy.policy, "structured");
    assert_eq!(r.command_status.shell_syntax_summary, "argv/simple");
    assert!(r.command_status.pipeline_masking_warning.is_none());
    assert!(r.visible.contains("search_summary"));
    let lookbehind = classify_command_status("rg -P '(?<=token)zero' crates", "src/lib.rs:1:zero\n", "", Some(0), false);
    assert_eq!(lookbehind.shell_syntax_summary, "argv/simple");
    assert!(lookbehind.pipeline_masking_warning.is_none());
}

#[test]
fn shell_c_wrappers_detect_masked_inner_pipeline_failures() {
    let expected_rerun = if cfg!(windows) { None } else { Some("bash -o pipefail -c 'false | true'") };
    for command in ["sh -c 'false | true'", "bash -lc 'false | true'", "zsh -euc 'false | true'", "bash --login -o pipefail -c 'false | true'"] {
        let status = classify_command_status(command, "", "", Some(0), false);
        assert!(!status.command_success, "{command}: {status:?}");
        assert_eq!(status.failed_segment.as_deref(), Some("false"), "{command}");
        assert_eq!(status.shell_syntax_summary, "pipeline", "{command}");
        assert!(status.pipeline_masking_warning.as_deref().is_some_and(|w| w.contains("mask")), "{command}: {status:?}");
        assert_eq!(status.pipeline_rerun_command.as_deref(), expected_rerun, "{command}");
    }
}

#[test]
fn path_qualified_rg_keeps_search_classification() {
    for command in ["/opt/homebrew/bin/rg -P '(?=tokenzero)' crates", "C:\\Tools\\ripgrep\\rg.exe -P '(?=tokenzero)' crates", "C:\\Windows\\System32\\findstr.exe \"error\" sample.txt"] {
        let d = decide_shell_policy(command, "src/lib.rs:1:tokenzero\n", "", Some(0), Mode::Auto);
        assert_eq!(d.family, "search", "{command}");
        assert_eq!(d.policy, "structured", "{command}");
    }
}

#[test]
fn explicit_or_failing_shell_keeps_status_capsule() {
    let explicit = render_shell(ShellRenderInput {
        command: "npm --version", stdout: "11.12.1\n", stderr: "", exit_code: Some(0),
        timed_out: false, mode: Mode::Exact, max_visible_tokens: 4000,
        stdout_ref: None, stderr_ref: None, combined_ref: None,
    });
    assert_eq!(explicit.output_strategy, "exact_first_adaptive_shell");
    has_all(&explicit.visible, &["# shell", "exit_code: 0"]);
    let failing = render_shell(auto("npm --version", "", "npm error\n", Some(1)));
    assert_eq!(failing.output_strategy, "compact_diagnostic_shell");
    has_all(&failing.visible, &["# shell", "status: command_failed", "npm error", "stderr_ref: tz://blob/stderr", "combined_ref: tz://blob/combined"]);
}

#[test]
fn compact_diagnostic_and_inventory_matrix() {
    struct Case { name: &'static str, command: &'static str, stdout: &'static str, stderr: &'static str, exit_code: i32, require: &'static [&'static str], forbid: &'static [&'static str], policy: Option<&'static str>, strategy: &'static str }
    let cases = [
        Case { name: "short_failing_powershell", command: "powershell -NoProfile -Command Write-Error boom; exit 7", stdout: "", stderr: "Write-Error boom; exit 7 : boom\n                      + CategoryInfo          : NotSpecified: (:) [Write-Error], WriteErrorException\n                      + FullyQualifiedErrorId : Microsoft.PowerShell.Commands.WriteErrorException\n", exit_code: 7, require: &["status: command_failed", "exit_code: 7", "failed_segment: exit 7", "pipeline_masking_warning", "stderr_ref: tz://blob/stderr", "combined_ref: tz://blob/combined", "boom"], forbid: &["FullyQualifiedErrorId"], policy: Some("diagnostic"), strategy: "compact_diagnostic_shell" },
        Case { name: "short_mixed_failure", command: "powershell -NoProfile -Command Write-Output 'warning: note'; [Console]::Error.WriteLine('error: fail'); exit 3", stdout: "warning: note\n", stderr: "error: fail\n", exit_code: 3, require: &["status: command_failed", "error: fail", "combined_ref: tz://blob/combined"], forbid: &["warning: note"], policy: None, strategy: "compact_diagnostic_shell" },
        Case { name: "repo_inventory", command: "powershell -NoProfile -Command Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName", stdout: "C:\\repo\\.tokenzero\\bench-cache.json\nC:\\repo\\sample.txt\n", stderr: "", exit_code: 0, require: &["repo_inventory", "files_seen: 2", "sample.txt", "combined_ref: tz://blob/combined"], forbid: &[], policy: Some("structured"), strategy: "compact_inventory_shell" },
    ];
    for case in cases {
        let r = render_shell(auto(case.command, case.stdout, case.stderr, Some(case.exit_code)));
        assert_eq!(r.output_strategy, case.strategy, "{}", case.name);
        if let Some(policy) = case.policy { assert_eq!(r.policy.policy, policy, "{}", case.name); }
        has_all(&r.visible, case.require);
        has_none(&r.visible, case.forbid);
        below_raw(case.command, case.stdout, case.stderr, Some(case.exit_code), &r.visible);
    }
}

#[test]
fn noisy_shell_output_compresses_below_raw_tokens() {
    let stdout = (0..200).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let command = "python -c print-lines";
    let r = render_shell(auto(command, &stdout, "", Some(0)));
    let raw = count_tokens(&shell_combined_output(command, Some(0), &stdout, ""));
    assert_eq!(r.policy.policy, "dedupe");
    assert!(count_tokens(&r.visible) < raw / 2);
    assert!(r.visible.contains("exact ref available"));
}

#[test]
fn failing_cargo_test_is_never_success_compacted() {
    let stdout = "running 3 tests\ntest tests::passes_one ... ok\ntest tests::fails_boundary ... FAILED\n\nfailures:\n\n---- tests::fails_boundary stdout ----\nassertion `left == right` failed: boundary mismatch on even-length list\n  left: [1, 2]\n right: [1, 2, 3]\n\nfailures:\n    tests::fails_boundary\n\ntest result: FAILED. 2 passed; 1 failed; 0 ignored\n";
    let mut input = success_input("cargo test -p demo", stdout, "");
    input.exit_code = Some(101);
    let r = render_shell(input);
    assert_ne!(r.output_strategy, "compact_success_shell");
    has_all(&r.visible, &["assertion `left == right` failed: boundary mismatch on even-length list", "left: [1, 2]", "right: [1, 2, 3]"]);
}

#[test]
fn long_success_collapse_matrix() {
    let listing = (0..400).map(|i| format!("/usr/lib/system/libsystem_{i:03}_{}.dylib", i * 7919)).collect::<Vec<_>>().join("\n");
    let r = render_shell(success_input("find /usr/lib -name *.dylib", &listing, ""));
    let raw = count_tokens(&shell_combined_output("find /usr/lib -name *.dylib", Some(0), &listing, ""));
    assert_shell_status(&r, true, Some(0), None, None);
    assert!(count_tokens(&r.visible) < raw / 5);
    has_all(&r.visible, &["exact ref available", "libsystem_000"]);
    let table = (0..100).map(|row| (0..30).map(|col| format!("cell{row}x{col}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join("\n");
    let r = render_shell(success_input("describe-table widgets", &table, ""));
    let raw = count_tokens(&shell_combined_output("describe-table widgets", Some(0), &table, ""));
    assert_eq!(r.output_strategy, "compact_success_shell");
    assert_shell_status(&r, true, Some(0), None, None);
    assert!(count_tokens(&r.visible) < raw / 4);
    has_all(&r.visible, &["exact ref available", "cell0x0"]);
}
