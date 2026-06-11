use super::*;
use proptest::prelude::*;

#[test]
fn simple_command_plans_as_argv_without_alias() {
    let argv = vec!["tokenzero".to_string(), "--version".to_string()];
    let plan = plan_command(&argv, None, false).unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert!(!plan.alias_dependency);
    assert!(plan.explicit_binary);
    assert_eq!(plan.argv, argv);
}

#[test]
fn shell_metacharacters_inside_argv_args_do_not_force_shell() {
    let argv = vec![
        "rg".to_string(),
        "error|warning".to_string(),
        "src/lib.rs".to_string(),
    ];
    let plan = plan_command_for_platform(&argv, None, false, "posix").unwrap();

    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, argv);
}

#[test]
fn windows_findstr_regex_metacharacters_inside_argv_stay_argv() {
    let argv = vec![
        "findstr".to_string(),
        "error|warning".to_string(),
        "src\\sample.txt".to_string(),
    ];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();

    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, argv);
    assert_eq!(
        command_display_for_execution_mode(&plan.argv, plan.execution_mode, &plan.platform),
        "findstr \"error|warning\" src\\sample.txt"
    );
}

#[test]
fn shell_syntax_uses_real_shell_not_alias() {
    let argv = vec!["echo ok | cat".to_string()];
    let plan = plan_command(&argv, None, false).unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(
        plan.shell.as_deref(),
        Some(if cfg!(windows) { "cmd" } else { "/bin/sh" })
    );
    assert!(!plan.alias_dependency);
}

#[test]
fn multi_arg_shell_operators_use_real_shell() {
    let argv = vec![
        "echo".to_string(),
        "one".to_string(),
        "&&".to_string(),
        "echo".to_string(),
        "two".to_string(),
    ];
    let plan = plan_command_for_platform(&argv, None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.argv, vec!["/bin/sh", "-c", "echo one && echo two"]);
    assert!(!plan.alias_dependency);
    assert!(!plan.explicit_binary);
}

#[test]
fn multi_arg_shell_operators_quote_literal_arguments_in_plan() {
    let argv = vec![
        "printf".to_string(),
        "%s\n".to_string(),
        "literal; echo TOKENZERO_INJECTED".to_string(),
        "|".to_string(),
        "cat".to_string(),
    ];
    let plan = plan_command_for_platform(&argv, None, false, "linux").unwrap();

    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(
        plan.argv,
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '%s\n' 'literal; echo TOKENZERO_INJECTED' | cat".to_string(),
        ]
    );
}

#[cfg(not(windows))]
#[test]
fn generated_multi_arg_shell_literal_metacharacters_stay_data() {
    for literal in [
        "two words",
        "semi;colon",
        "$TOKENZERO_RUNTIME_TEST_SHOULD_NOT_EXPAND",
        "$(printf TOKENZERO_INJECTED)",
        "`printf TOKENZERO_INJECTED`",
        "amp&ersand",
        "quote'heavy",
    ] {
        let argv = vec![
            "printf".to_string(),
            "%s\n".to_string(),
            literal.to_string(),
            "|".to_string(),
            "cat".to_string(),
        ];

        let result = run_command(&argv, None, None, None, Duration::from_secs(5), false).unwrap();

        assert!(result.ok, "{literal}: {}", result.stderr);
        assert_eq!(result.execution_mode, ExecutionMode::Shell, "{literal}");
        assert_eq!(
            result.command,
            *result.argv.last().expect("planned shell payload"),
            "{literal}"
        );
        assert_eq!(result.stdout, format!("{literal}\n"), "{literal}");
    }
}

#[test]
fn quoted_operator_literals_stay_argv() {
    assert!(!contains_shell_syntax("rg 'a|b' ."));
    assert!(contains_shell_syntax("rg a | head"));
    let argv = split_command_string("rg 'a|b' .");
    let plan = plan_command_for_platform(&argv, None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, vec!["rg", "a|b", "."]);
}

#[test]
fn leading_posix_env_assignment_uses_shell() {
    let command = "TOKENZERO_SHELL_CAPTURE_BYTES=12 tokenzero --version";
    assert!(contains_shell_syntax(command));
    let plan = plan_command_for_platform(&[command.to_string()], None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(
        plan.argv,
        vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()]
    );
}

#[test]
fn windows_split_preserves_path_backslashes() {
    let argv = split_command_string_for_platform(
        "powershell -File scripts\\rust_windows_verify.ps1",
        "windows",
    );
    assert_eq!(
        argv,
        vec!["powershell", "-File", "scripts\\rust_windows_verify.ps1"]
    );
}

#[test]
fn cmd_split_treats_single_quotes_as_literal_characters() {
    let argv = split_command_string_for_platform("findstr 'error warning' sample.txt", "cmd");
    assert_eq!(argv, vec!["findstr", "'error", "warning'", "sample.txt"]);
    assert!(contains_platform_shell_syntax(
        "findstr 'error|warning' sample.txt",
        "cmd"
    ));
}

#[test]
fn powershell_split_uses_single_quotes_for_literal_arguments() {
    let argv = split_command_string_for_platform(
        "powershell -NoProfile -Command 'Write-Output ok'",
        "windows",
    );
    assert_eq!(
        argv,
        vec!["powershell", "-NoProfile", "-Command", "Write-Output ok"]
    );
    assert!(!contains_platform_shell_syntax(
        "findstr 'error|warning' sample.txt",
        "powershell"
    ));

    let path_argv = split_command_string_for_platform(
        "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command 'Write-Output ok'",
        "windows",
    );
    assert_eq!(
        path_argv,
        vec![
            "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
            "-Command",
            "Write-Output ok"
        ]
    );
}

#[test]
fn cmd_split_preserves_doubled_quotes_inside_quoted_arguments() {
    let argv = split_command_string_for_platform("\"a\"\"b\" plain", "cmd");
    assert_eq!(argv, vec!["a\"b", "plain"]);
}

#[test]
fn powershell_split_preserves_doubled_single_quotes_inside_quoted_arguments() {
    let argv = split_command_string_for_platform("powershell -Command 'a''b'", "windows");
    assert_eq!(argv, vec!["powershell", "-Command", "a'b"]);

    let direct = split_command_string_for_platform("'a''b' plain", "powershell");
    assert_eq!(direct, vec!["a'b", "plain"]);
}

#[test]
fn split_preserves_empty_quoted_arguments() {
    assert_eq!(split_command_string_for_platform("\"\"", "cmd"), vec![""]);
    assert_eq!(
        split_command_string_for_platform("tool \"\" tail", "cmd"),
        vec!["tool", "", "tail"]
    );
    assert_eq!(
        split_command_string_for_platform("tool '' tail", "powershell"),
        vec!["tool", "", "tail"]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_split_roundtrips_displayed_cmd_and_powershell_args(
        arg in prop::string::string_regex("[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}['\"]?[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}").unwrap(),
        platform in prop::sample::select(vec!["cmd", "powershell"]),
    ) {
        let argv = vec!["tool".to_string(), arg];
        let displayed = tokenzero_core::shell_display_command_from_argv_for_platform(
            &argv,
            platform,
        );
        let parsed = split_command_string_for_platform(&displayed, platform);

        prop_assert_eq!(parsed, argv);
    }
}

#[test]
fn run_command_preserves_multi_arg_shell_operators() {
    let argv = vec![
        "echo".to_string(),
        "one".to_string(),
        "&&".to_string(),
        "echo".to_string(),
        "two".to_string(),
    ];
    let result = run_command(&argv, None, None, None, Duration::from_secs(5), false).unwrap();
    assert!(result.ok);
    assert_eq!(result.execution_mode, ExecutionMode::Shell);
    assert_eq!(
        result.stdout.lines().map(str::trim_end).collect::<Vec<_>>(),
        vec!["one", "two"]
    );
}

#[test]
fn stream_capture_spills_and_truncates_large_output() {
    let dir = tempfile::tempdir().unwrap();
    let policy = RunOutputPolicy {
        per_stream_capture_bytes: 10,
        spill_threshold_bytes: 5,
        spill_dir: Some(dir.path().to_path_buf()),
    };
    let stream = capture_reader(std::io::Cursor::new(vec![b'a'; 20]), "stdout", policy).unwrap();

    assert_eq!(stream.text, "aaaaaaaaaa");
    assert_eq!(stream.capture.bytes_seen, 20);
    assert_eq!(stream.capture.captured_bytes, 10);
    assert!(stream.capture.truncated);
    let spill_path = stream.capture.spill_path.unwrap();
    assert_eq!(std::fs::read(spill_path).unwrap().len(), 20);
    assert_eq!(stream.capture.spill_bytes, 20);
}

#[test]
fn stream_capture_spill_is_not_double_counted_on_large_first_read() {
    let dir = tempfile::tempdir().unwrap();
    let policy = RunOutputPolicy {
        per_stream_capture_bytes: 10,
        spill_threshold_bytes: 5,
        spill_dir: Some(dir.path().to_path_buf()),
    };
    let payload = vec![b'z'; 20_000];
    let stream = capture_reader(std::io::Cursor::new(payload.clone()), "stdout", policy).unwrap();

    let spill_path = stream.capture.spill_path.unwrap();
    assert_eq!(std::fs::read(spill_path).unwrap(), payload);
    assert_eq!(stream.capture.spill_bytes, 20_000);
}

#[cfg(not(windows))]
#[test]
fn run_command_caps_large_stdout_with_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let argv = vec!["yes x | head -c 100".to_string()];
    let policy = RunOutputPolicy {
        per_stream_capture_bytes: 12,
        spill_threshold_bytes: 6,
        spill_dir: Some(dir.path().to_path_buf()),
    };

    let result = run_command_with_policy(
        &argv,
        None,
        None,
        None,
        Duration::from_secs(5),
        false,
        policy,
    )
    .unwrap();

    assert!(result.ok, "{}", result.stderr);
    assert_eq!(result.stdout.len(), 12);
    assert_eq!(result.stdout_capture.bytes_seen, 100);
    assert_eq!(result.stdout_capture.captured_bytes, 12);
    assert!(result.stdout_capture.truncated);
    assert!(result.stdout_capture.spill_path.is_some());
    assert!(!result.stderr_capture.truncated);
    assert_eq!(
        result.allocator_pressure_relief.attempted,
        cfg!(target_os = "macos")
    );
}

#[cfg(not(windows))]
#[test]
fn timeout_kills_child_while_large_stdin_write_is_blocked() {
    let input = "x".repeat(8 * 1024 * 1024);
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 5".to_string(),
    ];
    let start = Instant::now();

    let result = run_command(
        &argv,
        None,
        None,
        Some(&input),
        Duration::from_millis(150),
        false,
    )
    .unwrap();

    assert!(result.timed_out);
    assert!(!result.ok);
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "timeout was not enforced while stdin write was blocked"
    );
}

#[cfg(not(windows))]
#[test]
fn background_descendant_holding_stdio_is_cleaned_without_false_timeout() {
    let input = "x".repeat(8 * 1024 * 1024);
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 5 &".to_string(),
    ];
    let start = Instant::now();

    let result = run_command(
        &argv,
        None,
        None,
        Some(&input),
        Duration::from_millis(150),
        false,
    )
    .unwrap();

    // The shell exited successfully; the background descendant holding
    // stdio is terminated at the IO grace, which is an honest success —
    // not a timeout — and must still return promptly.
    assert!(!result.timed_out, "{result:?}");
    assert!(result.ok, "{result:?}");
    assert!(result.io_grace_expired, "{result:?}");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "IO grace was not enforced while a background descendant held stdio open"
    );
}

#[cfg(not(windows))]
#[test]
fn fast_command_with_background_child_returns_promptly_without_timeout() {
    let argv = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "sleep 5 & echo started".to_string(),
    ];
    let start = Instant::now();

    let result = run_command(&argv, None, None, None, Duration::from_secs(30), false).unwrap();

    assert!(result.ok, "{result:?}");
    assert!(!result.timed_out, "{result:?}");
    assert!(result.io_grace_expired, "{result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("started"), "{result:?}");
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "foreground exit must not wait for the background child"
    );
}

#[test]
fn unsafe_escape_hatch_stays_single_macos_allocator_shim() {
    let source = include_str!("lib.rs");
    let allow_attr = concat!("#[allow(", "\n    unsafe_code,");
    let allow_reason = concat!(
        "macOS allocator pressure relief",
        " requires a tiny FFI shim"
    );
    let unsafe_extern = concat!("unsafe", " extern \"C\"");
    let unsafe_call = concat!("unsafe", " {");

    assert_eq!(
        source.matches(allow_attr).count(),
        1,
        "unexpected unsafe_code allow count"
    );
    assert_eq!(
        source.matches(allow_reason).count(),
        1,
        "unsafe_code allow must keep a precise reason"
    );
    assert_eq!(
        source.matches(unsafe_extern).count(),
        1,
        "unexpected unsafe extern count"
    );
    assert_eq!(
        source.matches(unsafe_call).count(),
        1,
        "unexpected unsafe block count"
    );
    assert!(source.contains("No Rust allocation or borrowed pointer is passed to C"));
}

#[test]
fn windows_shell_plan_uses_cmd_without_alias_dependency() {
    let argv = vec!["echo ok | findstr ok".to_string()];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.shell.as_deref(), Some("cmd"));
    assert_eq!(plan.shell_arg.as_deref(), Some("/C"));
    assert!(!plan.alias_dependency);
    assert!(!plan.explicit_binary);
}

#[test]
fn windows_powershell_script_plan_uses_powershell() {
    let script = "$tzTmp = Join-Path $env:TEMP 'tz-quote'; [Console]::Out.Write($tzTmp)";
    let argv = vec![script.to_string()];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.shell.as_deref(), Some("powershell"));
    assert_eq!(plan.shell_arg.as_deref(), Some("-Command"));
    assert_eq!(
        plan.argv,
        vec!["powershell", "-NoProfile", "-Command", script]
    );
    assert!(!plan.alias_dependency);
    assert!(!plan.explicit_binary);
}

#[test]
fn explicit_powershell_invocation_stays_argv() {
    let argv = vec![
        "powershell".to_string(),
        "-NoProfile".to_string(),
        "-Command".to_string(),
        "$env:TEMP".to_string(),
    ];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, argv);
}

#[test]
fn posix_shell_plan_uses_non_login_sh() {
    let argv = vec!["echo ok | cat".to_string()];
    let plan = plan_command_for_platform(&argv, None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.shell.as_deref(), Some("/bin/sh"));
    assert_eq!(plan.shell_arg.as_deref(), Some("-c"));
    assert!(!plan.alias_dependency);
}

#[test]
fn simple_windows_command_stays_explicit_argv() {
    let argv = vec!["tokenzero.exe".to_string(), "--version".to_string()];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert!(plan.explicit_binary);
    assert!(!plan.alias_dependency);
    assert_eq!(plan.argv, argv);
}

#[cfg(windows)]
#[test]
fn windows_run_resolves_pathext_cmd_shims() {
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join("npm.cmd");
    std::fs::write(&shim, "@echo off\r\necho shim:%1\r\n").unwrap();
    std::fs::write(dir.path().join("npm"), "#!/bin/sh\nexit 99\n").unwrap();
    let mut env = BTreeMap::new();
    env.insert("PATH".to_string(), dir.path().display().to_string());
    env.insert("PATHEXT".to_string(), ".CMD".to_string());
    let argv = vec!["npm".to_string(), "ping".to_string()];

    let result = run_command(&argv, None, Some(&env), None, Duration::from_secs(5), false).unwrap();

    assert!(result.ok, "{}", result.stderr);
    assert_eq!(result.stdout.trim(), "shim:ping");
    assert_eq!(result.execution_mode, ExecutionMode::Argv);
    assert_eq!(result.argv, argv);
}

#[cfg(windows)]
#[test]
fn windows_run_powershell_variable_script() {
    let script = "$tzTmp = Join-Path $env:TEMP 'tz-quote'; [Console]::Out.Write($tzTmp)";
    let argv = vec![script.to_string()];

    let result = run_command(&argv, None, None, None, Duration::from_secs(5), false).unwrap();

    assert!(result.ok, "{}", result.stderr);
    assert_eq!(result.execution_mode, ExecutionMode::Shell);
    assert_eq!(result.argv[0], "powershell");
    assert!(result.stdout.trim_end().ends_with("tz-quote"));
}

#[test]
fn windows_builtin_echo_uses_cmd() {
    let argv = vec!["echo".to_string(), "ok".to_string()];
    let plan = plan_command_for_platform(&argv, None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.argv, vec!["cmd", "/C", "echo ok"]);
    assert_eq!(plan.shell.as_deref(), Some("cmd"));
    assert_eq!(plan.shell_arg.as_deref(), Some("/C"));
    assert!(!plan.alias_dependency);
    assert!(!plan.explicit_binary);
}

#[test]
fn env_i_style_invocation_works() {
    let argv = vec!["echo".to_string(), "ok".to_string()];
    let result = run_command(&argv, None, None, None, Duration::from_secs(5), false).unwrap();
    assert!(result.ok);
    assert_eq!(result.stdout.trim(), "ok");
}

#[test]
fn quoting_preserves_spaces() {
    assert_eq!(quote_posix("a b"), "'a b'");
    assert_eq!(quote_powershell("a'b"), "'a''b'");
    assert_eq!(
        quote_windows_cmd("C:\\Program Files\\tz"),
        "\"C:\\Program Files\\tz\""
    );
}
