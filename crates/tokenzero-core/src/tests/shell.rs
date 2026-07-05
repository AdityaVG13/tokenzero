use super::*;

use super::support::*;

#[test]
fn shell_minimal_header_when_telemetry_dominates_small_success() {
    let stdout = "";
    let stderr = "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.21s\n";
    let rendered = render_shell(ShellRenderInput {
        command: "cargo build -p tokenzero-mcp",
        stdout,
        stderr,
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/bstdout"),
        stderr_ref: Some("tz://blob/bstderr"),
        combined_ref: Some("tz://blob/bcombined"),
    });

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(rendered.visible.starts_with("# shell ok"));
    assert!(
        rendered.visible.contains("cargo ok in 0.21s"),
        "success header should carry the Finished timing: {}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/bcombined"),
        "recovery anchor must survive the minimal envelope: {}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("policy:") && !rendered.visible.contains("command_success:"),
        "telemetry header should be dropped: {}",
        rendered.visible
    );
    let combined = shell_combined_output("cargo build -p tokenzero-mcp", Some(0), stdout, stderr);
    let overhead = count_tokens(&rendered.visible).saturating_sub(count_tokens(&combined));
    assert!(
        overhead <= 16,
        "minimal envelope overhead must stay bounded: visible={} raw={}",
        count_tokens(&rendered.visible),
        count_tokens(&combined)
    );
}

#[test]
fn shell_failures_keep_full_diagnostic_header() {
    let rendered = render_shell(ShellRenderInput {
        command: "cargo build -p nope",
        stdout: "",
        stderr: "error: package nope not found\n",
        exit_code: Some(101),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: None,
        stderr_ref: None,
        combined_ref: None,
    });

    assert_ne!(rendered.output_strategy, "minimal_envelope_shell");
    assert!(rendered.visible.contains("exit_code: 101") || rendered.visible.contains("101"));
}

#[test]
fn argv_display_command_quotes_shell_metacharacters() {
    let argv = vec![
        "rg".to_string(),
        "error|warning".to_string(),
        "don't panic".to_string(),
    ];

    assert_eq!(
        shell_display_command_from_argv(&argv),
        "rg 'error|warning' 'don'\\''t panic'"
    );
    assert_eq!(
        shell_display_command_from_argv_for_platform(&argv, "cmd"),
        "rg \"error|warning\" \"don't panic\""
    );
    assert_eq!(
        shell_display_command_from_argv_for_platform(&argv, "powershell"),
        "rg 'error|warning' 'don''t panic'"
    );

    let windows_path = vec![
        "findstr".to_string(),
        "error|warning".to_string(),
        "src\\sample.txt".to_string(),
    ];
    assert_eq!(
        shell_display_command_from_argv_for_platform(&windows_path, "windows"),
        "findstr \"error|warning\" src\\sample.txt"
    );
}

#[test]
fn shell_render_exposes_status_truth_and_refs() {
    let rendered = render_shell(ShellRenderInput {
        command: "false | true",
        stdout: "",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.policy, "diagnostic");
    assert!(!rendered.command_status.command_success);
    assert_eq!(rendered.command_status.status_label, "command_failed");
    assert_eq!(
        rendered.command_status.failed_segment.as_deref(),
        Some("false")
    );
    assert!(
        rendered
            .command_status
            .pipeline_masking_warning
            .as_deref()
            .unwrap()
            .contains("mask")
    );
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined")
    );
}

#[test]
fn tiny_success_shell_uses_compact_adaptive_view() {
    let rendered = render_shell(ShellRenderInput {
        command: "npm --version",
        stdout: "11.12.1\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.policy, "passthrough");
    assert_eq!(rendered.output_strategy, "compact_adaptive_shell");
    assert_eq!(rendered.visible, "11.12.1");
    assert!(!rendered.visible.contains("# shell"));
    assert!(
        count_tokens(&rendered.visible)
            <= count_tokens(&shell_combined_output(
                "npm --version",
                Some(0),
                "11.12.1\n",
                ""
            ))
    );
}

#[test]
fn rg_pcre2_search_output_gets_search_summary() {
    let rendered = render_shell(ShellRenderInput {
        command: "rg -P '(?=tokenzero)' crates",
        stdout: "crates/tokenzero-core/src/lib.rs:42:error: tokenzero\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.family, "search");
    assert_eq!(rendered.policy.policy, "structured");
    assert_eq!(rendered.command_status.status_label, "command_success");
    assert!(rendered.visible.contains("search_summary"));
    assert!(rendered.visible.contains("matches_seen: 1"));
    assert!(
        rendered
            .visible
            .contains("crates/tokenzero-core/src/lib.rs:42:error: tokenzero")
    );
}

#[test]
fn rg_quoted_regex_metacharacters_do_not_trigger_shell_warnings() {
    let rendered = render_shell(ShellRenderInput {
        command: "rg 'error|warning' crates",
        stdout: "crates/tokenzero-core/src/lib.rs:42:error: tokenzero\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.family, "search");
    assert_eq!(rendered.policy.policy, "structured");
    assert_eq!(rendered.command_status.shell_syntax_summary, "argv/simple");
    assert!(rendered.command_status.pipeline_masking_warning.is_none());
    assert!(rendered.visible.contains("search_summary"));

    let lookbehind = classify_command_status(
        "rg -P '(?<=token)zero' crates",
        "src/lib.rs:1:zero\n",
        "",
        Some(0),
        false,
    );
    assert_eq!(lookbehind.shell_syntax_summary, "argv/simple");
    assert!(lookbehind.pipeline_masking_warning.is_none());
}

#[test]
fn shell_wrapped_rg_search_keeps_search_family_and_summary() {
    let rendered = render_shell(ShellRenderInput {
        command: "bash -lc 'rg -P \"(?=tokenzero)\" crates'",
        stdout: "crates/tokenzero-core/src/lib.rs:42:error: tokenzero\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.family, "search");
    assert_eq!(rendered.policy.policy, "structured");
    assert_eq!(rendered.command_status.status_label, "command_success");
    assert!(rendered.visible.contains("search_summary"));
    assert!(rendered.visible.contains("matches_seen: 1"));
    assert!(
        rendered
            .visible
            .contains("crates/tokenzero-core/src/lib.rs:42:error: tokenzero")
    );
}

#[test]
fn shell_c_wrappers_detect_masked_inner_pipeline_failures() {
    for command in [
        "sh -c 'false | true'",
        "bash -lc 'false | true'",
        "zsh -euc 'false | true'",
        "bash --login -o pipefail -c 'false | true'",
    ] {
        let status = classify_command_status(command, "", "", Some(0), false);
        assert!(
            !status.command_success,
            "{command}: expected masked pipeline failure, got {status:?}"
        );
        assert_eq!(status.failed_segment.as_deref(), Some("false"), "{command}");
        assert_eq!(status.shell_syntax_summary, "pipeline", "{command}");
        assert!(
            status
                .pipeline_masking_warning
                .as_deref()
                .is_some_and(|warning| warning.contains("mask")),
            "{command}: {status:?}"
        );
        // pipeline_rerun_command suggests a bash rerun, which is suppressed
        // on Windows where bash is not assumed to exist.
        let expected_rerun = if cfg!(windows) {
            None
        } else {
            Some("bash -o pipefail -c 'false | true'")
        };
        assert_eq!(
            status.pipeline_rerun_command.as_deref(),
            expected_rerun,
            "{command}"
        );
    }
}

#[test]
fn rg_no_match_exit_one_is_not_a_command_failure() {
    let rendered = render_shell(ShellRenderInput {
        command: "rg -P '(?=missing)' crates",
        stdout: "",
        stderr: "",
        exit_code: Some(1),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert_eq!(rendered.policy.family, "search");
    assert_eq!(rendered.policy.policy, "structured");
    assert!(rendered.command_status.command_success);
    assert_eq!(rendered.command_status.status_label, "command_success");
    assert!(rendered.command_status.failed_segment.is_none());
    assert!(rendered.visible.contains("matches_seen: 0"));
}

#[test]
fn path_qualified_rg_keeps_search_classification() {
    for command in [
        "/opt/homebrew/bin/rg -P '(?=tokenzero)' crates",
        "C:\\Tools\\ripgrep\\rg.exe -P '(?=tokenzero)' crates",
        "C:\\Windows\\System32\\findstr.exe \"error\" sample.txt",
    ] {
        let decision =
            decide_shell_policy(command, "src/lib.rs:1:tokenzero\n", "", Some(0), Mode::Auto);

        assert_eq!(decision.family, "search", "{command}");
        assert_eq!(decision.policy, "structured", "{command}");
    }
}

#[test]
fn explicit_or_failing_shell_keeps_status_capsule() {
    let explicit = render_shell(ShellRenderInput {
        command: "npm --version",
        stdout: "11.12.1\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Exact,
        max_visible_tokens: 4000,
        stdout_ref: None,
        stderr_ref: None,
        combined_ref: None,
    });
    assert_eq!(explicit.output_strategy, "exact_first_adaptive_shell");
    assert!(explicit.visible.contains("# shell"));
    assert!(explicit.visible.contains("exit_code: 0"));

    let failing = render_shell(ShellRenderInput {
        command: "npm --version",
        stdout: "",
        stderr: "npm error\n",
        exit_code: Some(1),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });
    assert_eq!(failing.output_strategy, "compact_diagnostic_shell");
    assert!(failing.visible.contains("# shell"));
    assert!(failing.visible.contains("status: command_failed"));
    assert!(failing.visible.contains("npm error"));
    assert!(failing.visible.contains("stderr_ref: tz://blob/stderr"));
    assert!(failing.visible.contains("combined_ref: tz://blob/combined"));
}

#[test]
fn short_failing_shell_uses_compact_diagnostic_view_below_raw_tokens() {
    let command = "powershell -NoProfile -Command Write-Error boom; exit 7";
    let stderr = "Write-Error boom; exit 7 : boom\n\
                      + CategoryInfo          : NotSpecified: (:) [Write-Error], WriteErrorException\n\
                      + FullyQualifiedErrorId : Microsoft.PowerShell.Commands.WriteErrorException\n";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout: "",
        stderr,
        exit_code: Some(7),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });
    let raw_tokens = count_tokens(&shell_combined_output(command, Some(7), "", stderr));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.policy.policy, "diagnostic");
    assert_eq!(rendered.output_strategy, "compact_diagnostic_shell");
    assert!(rendered.visible.contains("status: command_failed"));
    assert!(rendered.visible.contains("exit_code: 7"));
    assert!(rendered.visible.contains("failed_segment: exit 7"));
    assert!(rendered.visible.contains("pipeline_masking_warning"));
    assert!(rendered.visible.contains("stderr_ref: tz://blob/stderr"));
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined")
    );
    assert!(rendered.visible.contains("boom"), "{}", rendered.visible);
    assert!(!rendered.visible.contains("FullyQualifiedErrorId"));
    assert!(
        visible_tokens < raw_tokens,
        "visible_tokens={visible_tokens} raw_tokens={raw_tokens}\n{}",
        rendered.visible
    );
}

#[test]
fn short_mixed_failure_prioritizes_error_anchor_below_raw_tokens() {
    let command = "powershell -NoProfile -Command Write-Output 'warning: note'; [Console]::Error.WriteLine('error: fail'); exit 3";
    let stdout = "warning: note\n";
    let stderr = "error: fail\n";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout,
        stderr,
        exit_code: Some(3),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });
    let raw_tokens = count_tokens(&shell_combined_output(command, Some(3), stdout, stderr));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_diagnostic_shell");
    assert!(rendered.visible.contains("status: command_failed"));
    assert!(rendered.visible.contains("error: fail"));
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined")
    );
    assert!(
        !rendered.visible.contains("warning: note"),
        "{}",
        rendered.visible
    );
    assert!(
        visible_tokens < raw_tokens,
        "visible_tokens={visible_tokens} raw_tokens={raw_tokens}\n{}",
        rendered.visible
    );
}

#[test]
fn short_repo_inventory_shell_view_stays_below_raw_tokens_with_ref() {
    let command = "powershell -NoProfile -Command Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName";
    let stdout = "C:\\repo\\.tokenzero\\bench-cache.json\nC:\\repo\\sample.txt\n";
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
    let raw_tokens = count_tokens(&shell_combined_output(command, Some(0), stdout, ""));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.policy.policy, "structured");
    assert_eq!(rendered.output_strategy, "compact_inventory_shell");
    assert!(rendered.visible.contains("repo_inventory"));
    assert!(rendered.visible.contains("files_seen: 2"));
    assert!(rendered.visible.contains("sample.txt"));
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined")
    );
    assert!(
        visible_tokens < raw_tokens,
        "visible_tokens={visible_tokens} raw_tokens={raw_tokens}\n{}",
        rendered.visible
    );
}

#[test]
fn noisy_shell_output_compresses_below_raw_tokens() {
    let stdout = (0..200)
        .map(|idx| format!("line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = render_shell(ShellRenderInput {
        command: "python -c print-lines",
        stdout: &stdout,
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });
    let raw_tokens = count_tokens(&shell_combined_output(
        "python -c print-lines",
        Some(0),
        &stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.policy.policy, "dedupe");
    assert!(visible_tokens < raw_tokens / 2);
    assert!(rendered.visible.contains("exact ref available"));
}

#[test]
fn failing_cargo_test_is_never_success_compacted() {
    let stdout = "running 3 tests\n\
test tests::passes_one ... ok\n\
test tests::fails_boundary ... FAILED\n\
\n\
failures:\n\
\n\
---- tests::fails_boundary stdout ----\n\
assertion `left == right` failed: boundary mismatch on even-length list\n\
  left: [1, 2]\n\
 right: [1, 2, 3]\n\
\n\
failures:\n\
    tests::fails_boundary\n\
\n\
test result: FAILED. 2 passed; 1 failed; 0 ignored\n";
    let mut input = success_input("cargo test -p demo", stdout, "");
    input.exit_code = Some(101);
    let rendered = render_shell(input);

    assert_ne!(rendered.output_strategy, "compact_success_shell");
    for anchor in [
        "assertion `left == right` failed: boundary mismatch on even-length list",
        "left: [1, 2]",
        "right: [1, 2, 3]",
    ] {
        assert!(
            rendered.visible.contains(anchor),
            "failure evidence lost: {anchor}\n{}",
            rendered.visible
        );
    }
}

#[test]
fn long_success_listing_is_collapsed_far_below_raw() {
    let stdout = (0..400)
        .map(|idx| format!("/usr/lib/system/libsystem_{idx:03}_{}.dylib", idx * 7919))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = render_shell(success_input("find /usr/lib -name *.dylib", &stdout, ""));
    let raw = count_tokens(&shell_combined_output(
        "find /usr/lib -name *.dylib",
        Some(0),
        &stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert!(
        visible_tokens < raw / 5,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("exact ref available"),
        "{}",
        rendered.visible
    );
}

#[test]
fn wide_success_passthrough_gets_token_squeeze() {
    let stdout = (0..100)
        .map(|row| {
            (0..30)
                .map(|col| format!("cell{row}x{col}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = render_shell(success_input("describe-table widgets", &stdout, ""));
    let raw = count_tokens(&shell_combined_output(
        "describe-table widgets",
        Some(0),
        &stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        visible_tokens < raw / 4,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("exact ref available"),
        "{}",
        rendered.visible
    );
}
