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
    assert_shell_status(&rendered, true, Some(0), None, None);
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
    assert_shell_status(&rendered, false, Some(101), Some("command_failed"), None);
    assert!(
        rendered.visible.contains("package nope not found"),
        "error evidence lost: {}",
        rendered.visible
    );
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
    assert_shell_status(
        &rendered,
        true,
        Some(0),
        Some("pipeline_masked"),
        Some(Some("false")),
    );
    assert!(rendered
        .command_status
        .pipeline_masking_warning
        .as_deref()
        .unwrap()
        .contains("mask"));
    assert!(rendered
        .visible
        .contains("combined_ref: tz://blob/combined"));
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
    assert!(rendered
        .visible
        .contains("crates/tokenzero-core/src/lib.rs:42:error: tokenzero"));
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
    assert!(rendered
        .visible
        .contains("crates/tokenzero-core/src/lib.rs:42:error: tokenzero"));
}

#[test]
fn shell_c_wrappers_detect_masked_inner_pipeline_failures() {
    for command in [
        "sh -c 'false | true'",
        "bash -lc 'false | true'",
        "zsh -euc 'false | true'",
        "zsh -lic 'false | true'",
        "bash --login -o pipefail -c 'false | true'",
    ] {
        let status = classify_command_status(command, "", "", Some(0), false);
        assert!(
            status.command_success,
            "{command}: exit 0 must agree with command_success, got {status:?}"
        );
        assert_eq!(
            status.status_label, "pipeline_masked",
            "{command}: expected pipeline_masked, got {status:?}"
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
fn tz3ry6_help_piped_to_head_is_not_command_failed() {
    let stdout = "Usage: am file_reservations [options]\n  --help  Show this help\n";
    let status = classify_command_status(
        "am file_reservations --help 2>&1 | head -40",
        stdout,
        "",
        Some(0),
        false,
    );
    assert!(
        status.command_success,
        "exit 0 must agree with command_success: {status:?}"
    );
    assert_ne!(
        status.status_label, "command_failed",
        "head closing a help pipe is not command_failed: {status:?}"
    );
    assert_eq!(status.exit_code, Some(0));
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
    let raw_tokens = count_tokens(&shell_raw_accounting_output(
        command,
        Some(7),
        "",
        stderr,
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.policy.policy, "diagnostic");
    assert_eq!(rendered.output_strategy, "compact_diagnostic_shell");
    assert!(rendered.visible.contains("status: command_failed"));
    assert!(rendered.visible.contains("exit_code: 7"));
    assert!(rendered.visible.contains("failed_segment: exit 7"));
    assert!(rendered.visible.contains("pipeline_masking_warning"));
    assert!(rendered.visible.contains("stderr_ref: tz://blob/stderr"));
    assert!(rendered
        .visible
        .contains("combined_ref: tz://blob/combined"));
    assert!(rendered.visible.contains("boom"), "{}", rendered.visible);
    assert!(!rendered.visible.contains("FullyQualifiedErrorId"));
    assert!(
        visible_tokens < raw_tokens,
        "visible_tokens={visible_tokens} raw_tokens={raw_tokens}\n{}",
        rendered.visible
    );
}

#[test]
fn masked_zero_exit_preserves_final_stdout_and_exit_marker() {
    let command = "cargo build 2>&1 | tail -1; ./target/debug/demo; echo EXIT=$?";
    let stdout = "error: stale build diagnostic\nREPRO_STDOUT={hello: 2}\nEXIT=0\n";
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

    assert_eq!(rendered.policy.policy, "diagnostic");
    assert_eq!(rendered.output_strategy, "exact_first_adaptive_shell");
    assert!(rendered.visible.contains("exit_code: 0"));
    assert!(rendered.visible.contains("pipeline_masking_warning"));
    assert!(rendered.visible.contains("# final stdout:"));
    assert!(rendered.visible.contains("REPRO_STDOUT={hello: 2}"));
    assert!(rendered.visible.contains("EXIT=0"));
    assert!(rendered
        .visible
        .contains("combined_ref: tz://blob/combined"));
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
    let raw_tokens = count_tokens(&shell_raw_accounting_output(
        command,
        Some(3),
        stdout,
        stderr,
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_diagnostic_shell");
    assert!(rendered.visible.contains("status: command_failed"));
    assert!(rendered.visible.contains("error: fail"));
    assert!(rendered
        .visible
        .contains("combined_ref: tz://blob/combined"));
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

/// `cat` output must never be rendered as an inventory.
///
/// `ls dir && cat report.json` used to classify as repo inventory because
/// `cat` was in INVENTORY_COMMANDS. Every JSON line contains `.` or `/`, so
/// the inventory view counted the file body as "paths" and emitted it as
/// sample_paths entries. The payload the caller actually asked for was
/// destroyed in the visible output.
#[test]
fn cat_output_is_not_rendered_as_repo_inventory() {
    let command = "ls /tmp/reports/ && cat /tmp/reports/gz.json";
    let stdout = "gz.json
{
  \"contract_version\": \"1.0\",
  \"passed\": false,
  \"name\": \"ctx.step\"
}
";
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
        !rendered.visible.contains("repo_inventory"),
        "cat output must not be classified as an inventory listing:\n{}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("sample_paths"),
        "file content must not be shredded into sample_paths:\n{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("contract_version"),
        "the payload the caller asked for must survive:\n{}",
        rendered.visible
    );
}

/// stderr must not reach sample_paths. An `ls` over a missing path emits
/// "ls: /nope: No such file or directory" on stderr; folded into the inventory
/// it appeared as a sample_paths entry indistinguishable from a real path.
#[test]
fn inventory_view_keeps_stderr_out_of_sample_paths() {
    let command = "ls /repo/a.txt /repo/missing.txt";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout: "/repo/a.txt
",
        stderr: "ls: /repo/missing.txt: No such file or directory
",
        exit_code: Some(1),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert!(rendered.visible.contains("repo_inventory"));
    assert!(
        !rendered.visible.contains("No such file or directory"),
        "stderr must not be rendered as inventory paths:\n{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("files_seen: 1"),
        "only the real stdout entry counts:\n{}",
        rendered.visible
    );
}

/// Every listing entry must survive the inventory view.
///
/// `inventory_stats` classified a line as a dir only on a trailing slash and as
/// a file only via the caller's predicate, with no else arm. A bare `ls` prints
/// dotless relative names like `src` or `alpha`, which matched neither, so they
/// were dropped from the counts AND from sample_paths with no marker. The view
/// then made a confident but false claim about the size of the listing.
#[test]
fn inventory_view_never_silently_drops_dotless_entries() {
    let command = "ls /repo; ls /repo/missing";
    let stdout = "alpha\nbeta\ngamma\nnotes.txt\n";
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

    assert!(rendered.visible.contains("repo_inventory"));
    for entry in ["alpha", "beta", "gamma", "notes.txt"] {
        assert!(
            rendered.visible.contains(entry),
            "entry {entry} vanished from the inventory view:\n{}",
            rendered.visible
        );
    }
    // The three dotless names are accounted for rather than discarded.
    assert!(
        rendered.visible.contains("other_entries_seen: 3"),
        "unclassified entries must be counted, not dropped:\n{}",
        rendered.visible
    );
}

/// A cleanly classified listing must not grow the extra counter, so the common
/// case stays exactly as cheap as it was.
#[test]
fn inventory_view_omits_other_counter_when_everything_classifies() {
    let command = "ls /repo/a.txt /repo/b.txt";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout: "/repo/a.txt\n/repo/b.txt\n",
        stderr: "",
        exit_code: Some(0),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert!(rendered.visible.contains("repo_inventory"));
    assert!(
        !rendered.visible.contains("other_entries_seen"),
        "no unclassified entries means no extra line:\n{}",
        rendered.visible
    );
}

/// stderr exclusion (zerostack-je4) must keep working now that unclassified
/// lines are retained: a diagnostic must not sneak in through the new arm.
#[test]
fn inventory_other_bucket_still_excludes_stderr() {
    let command = "ls /repo/a.txt /repo/missing.txt";
    let rendered = render_shell(ShellRenderInput {
        command,
        stdout: "/repo/a.txt\n",
        stderr: "ls: /repo/missing.txt: No such file or directory\n",
        exit_code: Some(1),
        timed_out: false,
        mode: Mode::Auto,
        max_visible_tokens: 4000,
        stdout_ref: Some("tz://blob/stdout"),
        stderr_ref: Some("tz://blob/stderr"),
        combined_ref: Some("tz://blob/combined"),
    });

    assert!(
        !rendered.visible.contains("No such file or directory"),
        "stderr must stay out of the inventory view:\n{}",
        rendered.visible
    );
    assert!(rendered.visible.contains("files_seen: 1"));
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
    let raw_tokens = count_tokens(&shell_raw_accounting_output(
        command,
        Some(0),
        stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.policy.policy, "structured");
    assert_eq!(rendered.output_strategy, "compact_inventory_shell");
    assert!(rendered.visible.contains("repo_inventory"));
    assert!(rendered.visible.contains("files_seen: 2"));
    assert!(rendered.visible.contains("sample.txt"));
    assert!(rendered
        .visible
        .contains("combined_ref: tz://blob/combined"));
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

    assert_shell_status(&rendered, true, Some(0), None, None);
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
    // At least the first few list entries must survive compaction.
    assert!(
        rendered.visible.contains("libsystem_000"),
        "first list item lost after compaction: {}",
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
    assert_shell_status(&rendered, true, Some(0), None, None);
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
    // First row of the grid must survive compaction.
    assert!(
        rendered.visible.contains("cell0x0"),
        "first grid cell lost after compaction: {}",
        rendered.visible
    );
}
