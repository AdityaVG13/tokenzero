use super::*;
use proptest::prelude::*;
use std::path::Path;

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
    // Mutation-killed: if `|` inside a single argv arg were mistaken for a
    // shell pipe, mode would be Shell. Verify the pipe char is literal.
    assert!(!contains_shell_syntax("rg 'error|warning' ."));
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

#[test]
fn quoted_operator_literals_stay_argv() {
    assert!(!contains_shell_syntax("rg 'a|b' ."));
    assert!(contains_shell_syntax("rg a | head"));
    // Pin POSIX tokenization to match the pinned linux plan: the
    // platform-current splitter keeps single quotes literal on Windows.
    let argv = split_command_string_for_platform("rg 'a|b' .", "linux");
    let plan = plan_command_for_platform(&argv, None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, vec!["rg", "a|b", "."]);
}

#[test]
fn double_quoted_backslash_stays_literal_before_ordinary_chars() {
    // POSIX: inside double quotes, backslash is literal unless before $ ` " \.
    let argv = split_command_string_for_platform("grep -n \"a\\|b\" file.txt", "linux");
    assert_eq!(argv, vec!["grep", "-n", "a\\|b", "file.txt"]);
    // Mutation-killed: a naive double-quote handler that treats \ as a
    // universal escape (collapsing `\|` -> `|`) would produce "a|b".
    // The correct POSIX behavior preserves the literal backslash.
    assert_ne!(
        argv[2], "a|b",
        "backslash before ordinary char '|' must be literal, not consumed as escape"
    );

    let escapes = split_command_string_for_platform("echo \"a\\\"b\\\\c\\$d\"", "linux");
    assert_eq!(escapes, vec!["echo", "a\"b\\c$d"]);

    // Unquoted backslash still escapes the next char.
    let unquoted = split_command_string_for_platform("echo a\\ b", "linux");
    assert_eq!(unquoted, vec!["echo", "a b"]);
    // Mutation-killed: without the escape, the space would split into
    // two args ["echo", "a", "b"].
    assert_eq!(unquoted.len(), 2, "escaped space must not split the token");
}

#[test]
fn variable_and_tilde_expansion_route_through_shell() {
    assert!(contains_shell_syntax("cat \"$HOME/.zshrc\""));
    assert!(contains_shell_syntax("cat $HOME/.zshrc"));
    assert!(contains_shell_syntax("echo ${PATH}"));
    assert!(contains_shell_syntax("cat ~/notes.txt"));
    assert!(contains_shell_syntax("ls ~"));
    assert!(contains_shell_syntax("ls ~bob/dir"));

    // Single quotes suppress expansion; literal cost must not route to shell.
    assert!(!contains_shell_syntax("echo '$HOME'"));
    assert!(!contains_shell_syntax("rg '~/x' ."));
    // Non-word-start tilde (git revision syntax) is not expansion.
    assert!(!contains_shell_syntax("git show HEAD~1"));
    // Escaped dollar is literal.
    assert!(!contains_shell_syntax("echo \\$HOME"));
    // Trailing bare dollar is literal.
    assert!(!contains_shell_syntax("echo a$"));
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

struct SplitCase {
    input: &'static str,
    platform: &'static str,
    expected: Vec<&'static str>,
}

fn assert_split_cases(cases: &[SplitCase]) {
    for (i, case) in cases.iter().enumerate() {
        let actual = split_command_string_for_platform(case.input, case.platform);
        let expected: Vec<String> = case.expected.iter().map(|s| s.to_string()).collect();
        assert_eq!(
            actual, expected,
            "case {i}: input={:?} platform={}",
            case.input, case.platform
        );
    }
}

/// Parametrized table for Windows/cmd/powershell split_command_string_for_platform.
/// Consolidates: windows_split_preserves_path_backslashes,
/// cmd_split_treats_single_quotes_as_literal_characters,
/// powershell_split_uses_single_quotes_for_literal_arguments,
/// split_preserves_empty_quoted_arguments.
#[test]
fn windows_quote_split_table() {
    let cases = [
        // Path backslashes preserved on Windows.
        SplitCase {
            input: "powershell -File scripts\\rust_windows_verify.ps1",
            platform: "windows",
            expected: vec!["powershell", "-File", "scripts\\rust_windows_verify.ps1"],
        },
        // cmd: single quotes are literal characters, not grouping.
        SplitCase {
            input: "findstr 'error warning' sample.txt",
            platform: "cmd",
            expected: vec!["findstr", "'error", "warning'", "sample.txt"],
        },
        // powershell: single quotes group literals.
        SplitCase {
            input: "powershell -NoProfile -Command 'Write-Output ok'",
            platform: "windows",
            expected: vec!["powershell", "-NoProfile", "-Command", "Write-Output ok"],
        },
        // powershell: double-quoted path with spaces.
        SplitCase {
            input: "\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -Command 'Write-Output ok'",
            platform: "windows",
            expected: vec![
                "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
                "-Command",
                "Write-Output ok",
            ],
        },
        // Empty double-quoted arg on cmd.
        SplitCase {
            input: "\"\"",
            platform: "cmd",
            expected: vec![""],
        },
        // Empty double-quoted arg embedded on cmd.
        SplitCase {
            input: "tool \"\" tail",
            platform: "cmd",
            expected: vec!["tool", "", "tail"],
        },
        // Empty single-quoted arg on powershell.
        SplitCase {
            input: "tool '' tail",
            platform: "powershell",
            expected: vec!["tool", "", "tail"],
        },
    ];
    assert_split_cases(&cases);
    // cmd: single quotes inside contains_platform_shell_syntax.
    assert!(contains_platform_shell_syntax(
        "findstr 'error|warning' sample.txt",
        "cmd"
    ));
    // powershell: single-quote-enclosed pipe is not shell syntax.
    assert!(!contains_platform_shell_syntax(
        "findstr 'error|warning' sample.txt",
        "powershell"
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_split_roundtrips_displayed_cmd_and_powershell_args(
        arg in prop::string::string_regex("[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}['\"]?[A-Za-z0-9 _./:\\\\@%+=,;|&$()`-]{0,32}").unwrap(),
        platform in prop::sample::select(vec!["cmd", "powershell", "posix"]),
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

/// Build a RunOutputPolicy for spill tests: 10-byte capture, 5-byte spill
/// threshold, spill dir set to the given temp dir.
fn test_policy(dir: &Path) -> RunOutputPolicy {
    RunOutputPolicy {
        per_stream_capture_bytes: 10,
        spill_threshold_bytes: 5,
        spill_dir: Some(dir.to_path_buf()),
    }
    .normalized()
}

#[test]
fn stream_capture_spills_and_truncates_large_output() {
    let dir = tempfile::tempdir().unwrap();
    let policy = test_policy(dir.path());
    let stream = capture_reader(std::io::Cursor::new(vec![b'a'; 20]), "stdout", policy).unwrap();

    assert_eq!(stream.text, "aaaaaaaaaa");
    assert_eq!(stream.capture.bytes_seen, 20);
    assert_eq!(stream.capture.captured_bytes, 10);
    assert!(stream.capture.truncated);
    let spill_path = stream.capture.spill_path.unwrap();
    // Mutation-killed: spill file must contain ALL 20 bytes, not just the
    // 10-byte captured prefix.
    let spill_content = std::fs::read(&spill_path).unwrap();
    assert_eq!(spill_content.len(), 20);
    assert!(
        spill_content.iter().all(|&b| b == b'a'),
        "spill file must contain the original payload, not partial/garbage"
    );
    assert_eq!(stream.capture.spill_bytes, 20);
}

#[test]
fn stream_capture_spill_is_not_double_counted_on_large_first_read() {
    let dir = tempfile::tempdir().unwrap();
    let policy = test_policy(dir.path());
    let payload = vec![b'z'; 20_000];
    let stream = capture_reader(std::io::Cursor::new(payload.clone()), "stdout", policy).unwrap();

    let spill_path = stream.capture.spill_path.unwrap();
    assert_eq!(std::fs::read(spill_path).unwrap(), payload);
    assert_eq!(stream.capture.spill_bytes, 20_000);
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
    assert!(result.exit_code != Some(0));
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "timeout was not enforced while stdin write was blocked"
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
fn quoting_preserves_spaces() {
    assert_eq!(quote_posix("a b"), "'a b'");
    assert_eq!(quote_powershell("a'b"), "'a''b'");
    assert_eq!(
        quote_windows_cmd("C:\\Program Files\\tz"),
        "\"C:\\Program Files\\tz\""
    );
    assert_eq!(quote_windows_cmd("%PATH%"), "\"%%PATH%%\"");
    assert_eq!(quote_windows_cmd("a^b"), "\"a^^b\"");
    assert_eq!(quote_windows_cmd("a\"b"), "\"a\\\"b\"");

    // Mutation-killed round-trip: an implementation that dropped the
    // space-preserving quotes would split "a b" into two tokens.
    let roundtripped =
        split_command_string_for_platform(&format!("tool {}", quote_posix("a b")), "linux");
    assert_eq!(roundtripped, vec!["tool", "a b"]);

    let roundtripped_cmd = split_command_string_for_platform(
        &format!("tool {}", quote_windows_cmd("C:\\Program Files\\tz")),
        "cmd",
    );
    assert_eq!(roundtripped_cmd, vec!["tool", "C:\\Program Files\\tz"]);

    // Mutation-killed: a cmd implementation that ignored %% doubling
    // would pass %PATH% through literally, expanding the env var.
    assert_ne!(
        quote_windows_cmd("%PATH%"),
        "%PATH%",
        "percent signs must be doubled to suppress expansion"
    );
}

fn write_spill_aged(dir: &Path, name: &str, bytes: usize, age: Duration) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, vec![b'x'; bytes]).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
    path
}

fn assert_spill_prune_kept(old: &Path, fresh: &Path, foreign: &Path, report: &SpillPruneReport) {
    assert!(!old.exists(), "expired spill must be reclaimed");
    assert!(fresh.exists(), "fresh spill must survive");
    assert!(foreign.exists(), "non-spill files must never be touched");
    assert_eq!(report.scanned_files, 2);
    assert_eq!(report.removed_files, 1);
    assert_eq!(report.removed_bytes, 10);
    assert_eq!(report.kept_files, 1);
    assert_eq!(report.kept_bytes, 20);
    assert_eq!(report.failed_removals, 0);
}

#[test]
fn spill_prune_reclaims_expired_and_keeps_fresh_and_foreign_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = write_spill_aged(
        dir.path(),
        "tokenzero-1-1-stdout.log",
        10,
        Duration::from_secs(48 * 60 * 60),
    );
    let fresh = write_spill_aged(
        dir.path(),
        "tokenzero-2-2-stderr.log",
        20,
        Duration::from_secs(60),
    );
    let foreign = write_spill_aged(
        dir.path(),
        "user-notes.log",
        30,
        Duration::from_secs(48 * 60 * 60),
    );
    let report = prune_spill_dir(
        dir.path(),
        DEFAULT_SPILL_TTL,
        DEFAULT_SPILL_MAX_TOTAL_BYTES,
        false,
    );
    assert_spill_prune_kept(&old, &fresh, &foreign, &report);
}

#[test]
fn spill_prune_byte_ceiling_evicts_oldest_first() {
    let dir = tempfile::tempdir().unwrap();
    let oldest = write_spill_aged(
        dir.path(),
        "tokenzero-1-1-stdout.log",
        60,
        Duration::from_secs(300),
    );
    let middle = write_spill_aged(
        dir.path(),
        "tokenzero-2-2-stdout.log",
        60,
        Duration::from_secs(200),
    );
    let newest = write_spill_aged(
        dir.path(),
        "tokenzero-3-3-stdout.log",
        60,
        Duration::from_secs(100),
    );
    let report = prune_spill_dir(dir.path(), DEFAULT_SPILL_TTL, 130, false);
    assert!(!oldest.exists(), "oldest spill must be evicted first");
    assert!(middle.exists());
    assert!(newest.exists());
    assert_eq!(report.removed_files, 1);
    assert_eq!(report.kept_bytes, 120);
}

#[test]
fn spill_prune_dry_run_counts_without_unlinking() {
    let dir = tempfile::tempdir().unwrap();
    let old = write_spill_aged(
        dir.path(),
        "tokenzero-1-1-stdout.log",
        10,
        Duration::from_secs(48 * 60 * 60),
    );
    let report = prune_spill_dir(
        dir.path(),
        DEFAULT_SPILL_TTL,
        DEFAULT_SPILL_MAX_TOTAL_BYTES,
        true,
    );
    assert!(old.exists(), "dry run must not unlink");
    assert!(report.dry_run);
    assert_eq!(report.removed_files, 1);
    assert_eq!(report.removed_bytes, 10);
}

#[test]
fn spill_prune_missing_dir_is_empty_report() {
    let dir = tempfile::tempdir().unwrap();
    let report = prune_spill_dir(
        &dir.path().join("does-not-exist"),
        DEFAULT_SPILL_TTL,
        DEFAULT_SPILL_MAX_TOTAL_BYTES,
        false,
    );
    assert_eq!(report.scanned_files, 0);
    assert_eq!(report.removed_files, 0);
}
