use super::*;
use proptest::prelude::*;

#[test]
fn token_count_is_nonzero_for_text() {
    assert!(count_tokens("hello world") >= 2);
    assert_eq!(count_tokens(""), 0);
}

#[test]
fn capsule_exact_hides_payload() {
    let c = make_capsule("secret payload", Mode::Exact, 10, Some("file"));
    assert!(!c.text.contains("secret payload"));
    assert!(c.raw_tokens > 0);
}

#[test]
fn capsule_never_costs_more_than_raw_text() {
    let text = "short documented answer that already fits the budget\n";
    let capsule = make_capsule(
        text,
        Mode::Auto,
        4000,
        Some("docs/some/deeply/nested/path/readme.md"),
    );

    assert!(
        capsule.visible_tokens <= capsule.raw_tokens,
        "framing must never exceed raw cost: visible={} raw={}",
        capsule.visible_tokens,
        capsule.raw_tokens
    );
    assert_eq!(capsule.text, text.trim_end());
}

proptest! {
    #[test]
    fn capsule_visible_never_exceeds_raw_when_raw_fits_budget(text in ".{1,512}") {
        prop_assume!(count_tokens(&text) <= 4000);
        for mode in [Mode::Auto, Mode::Structured, Mode::Dedupe, Mode::DiffAware] {
            let capsule = make_capsule(&text, mode, 4000, Some("a/long/label/for/overhead.txt"));
            prop_assert!(
                capsule.visible_tokens <= capsule.raw_tokens.max(1),
                "mode {:?}: visible={} raw={}",
                mode,
                capsule.visible_tokens,
                capsule.raw_tokens
            );
        }
    }
}

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
fn capsule_can_reuse_known_raw_token_count() {
    let text = "alpha beta gamma\n";
    let counted = make_capsule(text, Mode::Auto, 4000, Some("file"));
    let reused =
        make_capsule_with_raw_tokens(text, counted.raw_tokens, Mode::Auto, 4000, Some("file"));

    assert_eq!(reused, counted);
}

proptest! {
    #[test]
    fn capsule_with_known_raw_tokens_matches_counted_capsule(text in ".{0,256}") {
        let raw_tokens = count_tokens(&text);

        prop_assert_eq!(
            make_capsule_with_raw_tokens(&text, raw_tokens, Mode::Auto, 64, Some("source")),
            make_capsule(&text, Mode::Auto, 64, Some("source"))
        );
    }
}

#[test]
fn auto_capsule_honors_visible_budget_for_short_token_heavy_files() {
    let text = (0..40)
        .map(|line| {
            format!(
                "line {line}: {}",
                (0..30)
                    .map(|word| format!("token{line}_{word}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let c = make_capsule(&text, Mode::Auto, 120, Some("state.md"));

    assert!(c.raw_tokens > 120);
    assert!(
        c.visible_tokens <= 120,
        "visible_tokens={} text={}",
        c.visible_tokens,
        c.text
    );
    assert!(c.text.contains("omitted"));
}

#[test]
fn budget_truncation_marker_names_recovery_ref_when_known() {
    let text = (0..500)
        .map(|line| format!("line {line} with some content to count"))
        .collect::<Vec<_>>()
        .join("\n");

    let with_ref = enforce_token_budget_with_ref(&text, 100, Some("tz://file/fabc123"));
    assert!(
        with_ref.contains("expand tz://file/fabc123 for the full output"),
        "{with_ref}"
    );
    assert!(
        count_tokens(&with_ref) <= 100,
        "{}",
        count_tokens(&with_ref)
    );

    let without_ref = enforce_token_budget(&text, 100);
    assert!(
        without_ref.contains("exact refs available"),
        "{without_ref}"
    );

    let capsule = make_capsule_with_recovery_ref(
        &text,
        count_tokens(&text),
        Mode::Structured,
        60,
        Some("big.txt"),
        Some("tz://file/fabc123"),
    );
    assert!(
        capsule.text.contains("tz://file/fabc123"),
        "{}",
        capsule.text
    );
}

#[test]
fn long_labels_do_not_crowd_out_tiny_payloads() {
    let c = make_capsule(
        "ok\n",
        Mode::Auto,
        20,
        Some("C:\\Users\\Ada\\AppData\\Local\\Temp\\tokenzero-long-label\\tiny.md"),
    );

    assert!(c.text.contains("ok"), "{}", c.text);
    assert!(!c.text.contains("omitted"), "{}", c.text);
    assert!(
        c.visible_tokens <= 20,
        "{} tokens in {}",
        c.visible_tokens,
        c.text
    );
}

#[test]
fn line_and_symbol_selectors_work() {
    let text = "fn alpha() {\n  call();\n}\n\nfn beta() {}\n";
    assert_eq!(line_range(text, 1, 2), "fn alpha() {\n  call();");
    assert!(symbol_block(text, "alpha").contains("call"));
}

#[test]
fn mode_aliases_map_to_new_policy_names() {
    assert_eq!("auto".parse::<Mode>().unwrap(), Mode::Auto);
    assert_eq!("diagnostic".parse::<Mode>().unwrap(), Mode::Diagnostic);
    assert_eq!("diff-aware".parse::<Mode>().unwrap(), Mode::DiffAware);
    assert_eq!("hybrid".parse::<Mode>().unwrap(), Mode::Auto);
    assert_eq!("critical".parse::<Mode>().unwrap(), Mode::Diagnostic);
    assert_eq!("fidelity".parse::<Mode>().unwrap(), Mode::Structured);
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
fn windows_shell_wrapped_search_commands_keep_search_summary() {
    for command in [
        "powershell.exe -NoProfile -ExecutionPolicy Bypass -Command rg -P '(?=tokenzero)' sample.txt",
        "pwsh -NoLogo -ExecutionPolicy:Bypass -c rg 'error|warning' sample.txt",
        "cmd.exe /S /C findstr \"error|warning\" sample.txt",
    ] {
        let rendered = render_shell(ShellRenderInput {
            command,
            stdout: "sample.txt:1:error: tokenzero\n",
            stderr: "",
            exit_code: Some(0),
            timed_out: false,
            mode: Mode::Auto,
            max_visible_tokens: 4000,
            stdout_ref: Some("tz://blob/stdout"),
            stderr_ref: Some("tz://blob/stderr"),
            combined_ref: Some("tz://blob/combined"),
        });

        assert_eq!(rendered.policy.family, "search", "{command}");
        assert_eq!(rendered.policy.policy, "structured", "{command}");
        assert_eq!(
            rendered.command_status.shell_syntax_summary, "argv/simple",
            "{command}"
        );
        assert!(
            rendered.command_status.pipeline_masking_warning.is_none(),
            "{command}: {:?}",
            rendered.command_status
        );
        assert!(rendered.visible.contains("search_summary"), "{command}");
        assert!(rendered.visible.contains("matches_seen: 1"), "{command}");
        assert!(
            rendered.visible.contains("sample.txt:1:error: tokenzero"),
            "{command}: {}",
            rendered.visible
        );
    }
}

#[test]
fn real_shell_operators_still_drive_status_warnings() {
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

    assert_eq!(rendered.command_status.shell_syntax_summary, "pipeline");
    assert!(!rendered.command_status.command_success);
    assert_eq!(
        rendered.command_status.failed_segment.as_deref(),
        Some("false")
    );
    assert!(rendered.command_status.pipeline_masking_warning.is_some());

    assert_eq!(
        split_shell_segments("printf 'a|b;c' || true"),
        vec!["printf 'a|b;c'", "true"]
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
fn shell_c_wrappers_do_not_analyze_positional_args_as_code() {
    let status = classify_command_status(
        "bash -c 'true' 'false | true' '$1; rm -rf /'",
        "",
        "",
        Some(0),
        false,
    );

    assert!(status.command_success, "{status:?}");
    assert!(status.failed_segment.is_none(), "{status:?}");
    assert_eq!(status.shell_syntax_summary, "argv/simple");
    assert!(status.pipeline_masking_warning.is_none(), "{status:?}");
    assert!(status.pipeline_rerun_command.is_none(), "{status:?}");
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

fn success_input<'a>(command: &'a str, stdout: &'a str, stderr: &'a str) -> ShellRenderInput<'a> {
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

#[test]
fn repeat_render_collapses_verified_unchanged_success() {
    let stdout = (0..40)
        .map(|idx| format!("processing item {idx} done"))
        .collect::<Vec<_>>()
        .join("\n");
    let input = success_input("./generate.sh --all", &stdout, "");
    let raw = count_tokens(&shell_combined_output(
        "./generate.sh --all",
        Some(0),
        &stdout,
        "",
    ));

    let rendered = render_shell_repeat(input, 3);
    assert_eq!(rendered.output_strategy, "repeat_unchanged_shell");
    assert!(
        rendered.visible.contains("unchanged; run 3"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("processing item"));
    assert!(rendered.command_status.command_success);
    let visible_tokens = count_tokens(&rendered.visible);
    assert!(
        visible_tokens * 5 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn repeat_render_never_compacts_failures_first_runs_or_explicit_modes() {
    let stdout = "step one\nstep two\n";

    let mut failed = success_input("cargo test", stdout, "error[E0308]: mismatched types");
    failed.exit_code = Some(101);
    let rendered = render_shell_repeat(failed.clone(), 5);
    assert_ne!(rendered.output_strategy, "repeat_unchanged_shell");
    assert_eq!(rendered, render_shell(failed));

    let first_run = success_input("cargo test", stdout, "");
    let rendered = render_shell_repeat(first_run.clone(), 1);
    assert_eq!(rendered, render_shell(first_run));

    let mut explicit = success_input("cargo test", stdout, "");
    explicit.mode = Mode::Passthrough;
    let rendered = render_shell_repeat(explicit.clone(), 4);
    assert_eq!(rendered, render_shell(explicit));

    let mut no_ref = success_input("cargo test", stdout, "");
    no_ref.combined_ref = None;
    let rendered = render_shell_repeat(no_ref.clone(), 4);
    assert_eq!(rendered, render_shell(no_ref));
}

#[test]
fn repeat_render_keeps_adaptive_floor_for_tiny_outputs() {
    let input = success_input("true", "", "");
    let raw = count_tokens(&shell_combined_output("true", Some(0), "", ""));
    let rendered = render_shell_repeat(input, 9);
    assert!(
        count_tokens(&rendered.visible) <= raw,
        "visible must never exceed raw: {} vs {raw}\n{}",
        count_tokens(&rendered.visible),
        rendered.visible
    );
}

#[test]
fn cargo_cold_build_success_collapses_compile_noise_into_counts() {
    let stderr = (0..50)
        .map(|idx| format!("   Compiling crate{idx} v0.{idx}.0"))
        .chain(std::iter::once(
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.42s".to_string(),
        ))
        .collect::<Vec<_>>()
        .join("\n");
    let rendered = render_shell(success_input("cargo check -p demo", "", &stderr));
    let raw = count_tokens(&shell_combined_output(
        "cargo check -p demo",
        Some(0),
        "",
        &stderr,
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("50 compiled"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("in 8.42s"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("combined_ref: tz://blob/combined"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("Compiling crate7"));
    assert!(
        visible_tokens * 3 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn cargo_success_keeps_warning_blocks_verbatim() {
    let mut stderr = (0..30)
        .map(|idx| format!("   Compiling dep{idx} v0.{idx}.0"))
        .collect::<Vec<_>>()
        .join("\n");
    stderr.push_str(
        "\nwarning: unused variable: `x`\n \
 --> src/lib.rs:5:9\n  |\n5 |     let x = 1;\n  |         ^ help: prefix with underscore\n",
    );
    stderr.push_str(
        &(30..60)
            .map(|idx| format!("   Compiling dep{idx} v0.{idx}.0"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    stderr.push_str("\n    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.00s\n");
    let rendered = render_shell(success_input("cargo build", "", &stderr));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        !rendered.visible.contains("Compiling dep7"),
        "{}",
        rendered.visible
    );
    for anchor in [
        "warning: unused variable: `x`",
        "--> src/lib.rs:5:9",
        "^ help: prefix with underscore",
    ] {
        assert!(
            rendered.visible.contains(anchor),
            "protected anchor lost: {anchor}\n{}",
            rendered.visible
        );
    }
}

#[test]
fn cargo_test_success_collapses_ok_lines_and_keeps_result_verbatim() {
    let stdout = std::iter::once("running 53 tests".to_string())
            .chain((0..53).map(|idx| format!("test module::case_{idx} ... ok")))
            .chain(std::iter::once(
                "test result: ok. 53 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.21s"
                    .to_string(),
            ))
            .collect::<Vec<_>>()
            .join("\n");
    let rendered = render_shell(success_input("cargo test -p demo --lib", &stdout, ""));
    let raw = count_tokens(&shell_combined_output(
        "cargo test -p demo --lib",
        Some(0),
        &stdout,
        "",
    ));
    let visible_tokens = count_tokens(&rendered.visible);

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("53 tests ok"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("test result: ok. 53 passed; 0 failed;"),
        "result line must stay verbatim: {}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("case_17"));
    assert!(
        visible_tokens * 3 < raw,
        "visible={visible_tokens} raw={raw}\n{}",
        rendered.visible
    );
}

#[test]
fn passing_tests_with_critical_keyword_names_still_collapse() {
    let stdout = "running 4 tests\n\
test tests::warning_handling_works ... ok\n\
test tests::failure_paths_are_covered ... ok\n\
test tests::error_messages_render ... ok\n\
test tests::panic_recovery_guard ... ok\n\
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    let rendered = render_shell(success_input("cargo test -p demo", stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("4 tests ok"),
        "{}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("warning_handling_works"),
        "pass markers must collapse regardless of name: {}",
        rendered.visible
    );
    assert!(
        rendered
            .visible
            .contains("test result: ok. 4 passed; 0 failed;"),
        "{}",
        rendered.visible
    );
}

#[test]
fn pytest_success_collapses_progress_and_keeps_summary() {
    let stdout = "============================= test session starts ==============================\n\
platform darwin -- Python 3.12.0, pytest-8.0.0\n\
rootdir: /tmp/demo\n\
collected 12 items\n\
tests/test_a.py::test_one PASSED\n\
tests/test_a.py::test_two PASSED\n\
........                                                                 [100%]\n\
============================== 12 passed in 0.34s ==============================\n";
    let rendered = render_shell(success_input("pytest tests", stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("12 passed in 0.34s"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("test_one PASSED"));
    assert!(!rendered.visible.contains("rootdir:"));
}

#[test]
fn npm_install_success_keeps_summary_and_drops_funding_noise() {
    let mut stdout = (0..20)
        .map(|idx| format!("npm http fetch GET 200 https://registry.npmjs.org/pkg-{idx} 41ms"))
        .collect::<Vec<_>>()
        .join("\n");
    stdout.push_str(
        "\nadded 57 packages, and audited 58 packages in 3s\n\
7 packages are looking for funding\n\
  run `npm fund` for details\n\
found 0 vulnerabilities\n",
    );
    let rendered = render_shell(success_input("npm install", &stdout, ""));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered.visible.contains("added 57 packages"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("found 0 vulnerabilities"),
        "{}",
        rendered.visible
    );
    assert!(!rendered.visible.contains("npm fund"));
}

#[test]
fn git_clone_success_collapses_progress_to_final_state() {
    let stderr = "Cloning into 'demo'...\n\
remote: Enumerating objects: 1200, done.\n\
remote: Counting objects:  10% (120/1200)\n\
remote: Counting objects: 100% (1200/1200), done.\n\
Receiving objects:  10% (120/1200)\n\
Receiving objects:  55% (660/1200)\n\
Receiving objects: 100% (1200/1200), 2.5 MiB | 5 MiB/s, done.\n\
Resolving deltas:  40% (200/500)\n\
Resolving deltas: 100% (500/500), done.\n";
    let rendered = render_shell(success_input(
        "git clone https://example.com/demo.git",
        "",
        stderr,
    ));

    assert_eq!(rendered.output_strategy, "compact_success_shell");
    assert!(
        rendered
            .visible
            .contains("Receiving objects: 100% (1200/1200), 2.5 MiB | 5 MiB/s, done."),
        "final progress state must survive: {}",
        rendered.visible
    );
    assert!(
        !rendered.visible.contains("Receiving objects:  55%"),
        "{}",
        rendered.visible
    );
    assert!(
        rendered.visible.contains("Cloning into 'demo'..."),
        "{}",
        rendered.visible
    );
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

#[test]
fn summarize_tokens_keeps_critical_lines_even_over_budget() {
    let mut lines: Vec<String> = (0..300).map(|idx| format!("noise line {idx}")).collect();
    lines[150] = "error[E0308]: mismatched types in src/lib.rs:42".to_string();
    lines[222] = "warning: unused import reported".to_string();
    let text = lines.join("\n");
    let summary = summarize_tokens(&text, 60, "");

    assert!(summary.contains("error[E0308]: mismatched types in src/lib.rs:42"));
    assert!(summary.contains("warning: unused import reported"));
    assert!(summary.contains("exact ref available"));
    assert!(count_tokens(&summary) < count_tokens(&text) / 4);
}

#[test]
fn structural_dedupe_collapses_digit_varying_runs_but_not_criticals() {
    let text = (0..40)
        .map(|idx| format!("Receiving chunk {idx} of 40 (eta {idx}s)"))
        .chain([
            "error: socket reset".to_string(),
            "error: socket reset".to_string(),
        ])
        .collect::<Vec<_>>()
        .join("\n");
    let deduped = dedupe_lines_structural(&text, 6);

    assert!(deduped.contains("similar lines collapsed"), "{deduped}");
    assert_eq!(
        deduped.matches("error: socket reset").count(),
        2,
        "critical lines must never collapse: {deduped}"
    );
    assert!(deduped.lines().count() < 10, "{deduped}");
}

proptest! {
    #[test]
    fn successful_shell_visible_never_exceeds_raw_plus_envelope(
        lines in prop::collection::vec("[a-z0-9 ]{0,40}", 0..120),
        inject_warning in prop::bool::ANY,
    ) {
        let mut stdout = lines.join("\n");
        if inject_warning {
            stdout.push_str("\nwarning: injected anchor line");
        }
        let rendered = render_shell(success_input("generic-tool --run", &stdout, ""));
        let raw = count_tokens(&shell_combined_output(
            "generic-tool --run",
            Some(0),
            &stdout,
            "",
        ));
        let visible_tokens = count_tokens(&rendered.visible);

        prop_assert!(
            visible_tokens <= raw + 16,
            "visible={} raw={} strategy={}\n{}",
            visible_tokens,
            raw,
            rendered.output_strategy,
            rendered.visible
        );
        if inject_warning {
            prop_assert!(
                rendered.visible.contains("warning: injected anchor line"),
                "anchor lost: {}",
                rendered.visible
            );
        }
    }

    #[test]
    fn repeat_render_floor_and_failure_fidelity(
        lines in prop::collection::vec("[a-z0-9 ]{0,40}", 0..120),
        seen in 0u32..6,
        exit_code in prop::sample::select(vec![Some(0), Some(1), None]),
    ) {
        let stdout = lines.join("\n");
        let mut input = success_input("generic-tool --run", &stdout, "");
        input.exit_code = exit_code;
        let rendered = render_shell_repeat(input.clone(), seen);
        let raw = count_tokens(&shell_combined_output(
            "generic-tool --run",
            exit_code,
            &stdout,
            "",
        ));
        let visible_tokens = count_tokens(&rendered.visible);

        if exit_code == Some(0) {
            prop_assert!(
                visible_tokens <= raw + 16,
                "visible={} raw={} strategy={}\n{}",
                visible_tokens,
                raw,
                rendered.output_strategy,
                rendered.visible
            );
        }
        if exit_code != Some(0) || seen < 2 {
            prop_assert_eq!(rendered, render_shell(input));
        }
    }

    #[test]
    fn summarize_tokens_never_loses_critical_lines(
        noise in prop::collection::vec("[a-z ]{0,30}", 10..200),
        anchor_pos in 0usize..200,
    ) {
        let mut lines = noise;
        let pos = anchor_pos % lines.len();
        lines[pos] = "assertion failed: protected anchor evidence".to_string();
        let text = lines.join("\n");
        let summary = summarize_tokens(&text, 48, "");

        prop_assert!(
            summary.contains("assertion failed: protected anchor evidence"),
            "{}",
            summary
        );
    }
}

#[test]
fn classifier_covers_required_diagnostic_families() {
    let cases = [
        ("cargo test", "test", "diagnostic"),
        ("cargo build", "build", "diagnostic"),
        ("cargo check", "build", "diagnostic"),
        ("cargo clippy", "build", "diagnostic"),
        ("pytest", "python-test", "diagnostic"),
        ("python -m unittest", "python-test", "diagnostic"),
        ("npm test", "test", "diagnostic"),
        ("vitest", "test", "diagnostic"),
        ("jest", "test", "diagnostic"),
        ("eslint src", "lint", "diagnostic"),
        ("tsc --noEmit", "lint", "diagnostic"),
        ("ruff check", "lint", "diagnostic"),
        ("mypy pkg", "lint", "diagnostic"),
        ("go test ./...", "go-test", "diagnostic"),
    ];
    for (command, family, policy) in cases {
        let decision = decide_shell_policy(command, "", "error: failed", Some(1), Mode::Auto);
        assert_eq!(decision.family, family, "{command}");
        assert_eq!(decision.policy, policy, "{command}");
    }
}

#[test]
fn diff_structured_and_dedupe_renderers_keep_critical_evidence() {
    let diff = "diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n";
    assert!(diff_summary(diff, 20).contains("@@ -1 +1 @@"));
    assert!(diff_summary(diff, 20).contains("+new"));

    let json = r#"[{"name":"api","status":"ok"},{"name":"db","status":"failed"}]"#;
    let structured = structured_shell_view("docker ps --format json", json, "");
    assert!(structured.contains("abnormal"));
    assert!(structured.contains("failed"));

    let repeated = dedupe_lines("tick\ntick\ntick\nerror: boom\n", 8);
    assert!(repeated.contains("repeated 2 more"));
    assert!(repeated.contains("error: boom"));
}

#[test]
fn repo_inventory_and_secret_masking_are_safe_visible_views() {
    let inventory = repo_inventory_view(
        "find . -type f | sort | wc -l && find . -type f | sort",
        "2\nsrc/lib.rs\nCargo.toml\n",
    );
    assert!(inventory.contains("repo_inventory"));
    assert!(inventory.contains("files_seen"));
    assert!(inventory.contains("src/lib.rs"));

    let masked = mask_visible_secrets("token=abc123\nAuthorization sk-proj-secret");
    assert!(masked.contains("token=[masked]"));
    assert!(!masked.contains("abc123"));
    assert!(!masked.contains("sk-proj-secret"));
}

proptest! {
    #[test]
    fn ids_are_stable(s in ".*") {
        prop_assert_eq!(id_for('b', &s), id_for('b', &s));
    }

    #[test]
    fn generated_shell_c_wrappers_preserve_inner_operator_semantics(
        shell in prop::sample::select(vec!["sh", "bash", "zsh"]),
        flags in prop::sample::select(vec!["-c", "-lc", "-euc"]),
        operator in prop::sample::select(vec!["|", "&&", "||", ";"]),
        quote in prop::sample::select(vec!["'", "\""]),
    ) {
        let inner = format!("false {operator} true");
        let command = format!("{shell} {flags} {quote}{inner}{quote}");
        let status = classify_command_status(&command, "", "", Some(0), false);

        prop_assert_eq!(
            status.failed_segment.as_deref(),
            Some("false"),
            "{}: {:?}",
            command,
            status
        );
        prop_assert!(
            !status.command_success,
            "{}: expected masked failure, got {:?}",
            command,
            status
        );
        prop_assert!(
            status.shell_syntax_summary != "argv/simple",
            "{}: {:?}",
            command,
            status
        );
    }
}
