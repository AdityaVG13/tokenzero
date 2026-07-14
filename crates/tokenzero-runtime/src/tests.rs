use super::*;
use proptest::prelude::*;
use std::path::Path;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

#[test]
fn plan_and_shell_syntax_matrix() {
    // simple argv plan
    let simple = argv(&["tokenzero", "--version"]);
    let plan = plan_command(&simple, None, false).unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert!(!plan.alias_dependency && plan.explicit_binary);
    assert_eq!(plan.argv, simple);

    // shell metacharacters inside argv args stay Argv
    let rg = argv(&["rg", "error|warning", "src/lib.rs"]);
    let plan = plan_command_for_platform(&rg, None, false, "posix").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, rg);
    assert!(!contains_shell_syntax("rg 'error|warning' ."));

    // multi-arg operators force shell
    for &(name, words, expected_c) in &[
        ("and_list", &["echo", "one", "&&", "echo", "two"][..], "echo one && echo two"),
        (
            "quoted_literal_args",
            &["printf", "%s\n", "literal; echo TOKENZERO_INJECTED", "|", "cat"][..],
            "printf '%s\n' 'literal; echo TOKENZERO_INJECTED' | cat",
        ),
    ] {
        let plan = plan_command_for_platform(&argv(words), None, false, "linux").unwrap();
        assert_eq!(plan.execution_mode, ExecutionMode::Shell, "{name}");
        assert_eq!(
            plan.argv,
            vec!["/bin/sh".to_string(), "-c".to_string(), expected_c.to_string()],
            "{name}"
        );
        assert!(!plan.alias_dependency && !plan.explicit_binary, "{name}");
    }

    // quoted operator literals stay argv
    assert!(!contains_shell_syntax("rg 'a|b' ."));
    assert!(contains_shell_syntax("rg a | head"));
    let words = split_command_string_for_platform("rg 'a|b' .", "linux");
    let plan = plan_command_for_platform(&words, None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Argv);
    assert_eq!(plan.argv, vec!["rg", "a|b", "."]);

    // double-quoted backslash literals
    let split = split_command_string_for_platform("grep -n \"a\\|b\" file.txt", "linux");
    assert_eq!(split, vec!["grep", "-n", "a\\|b", "file.txt"]);
    assert_ne!(split[2], "a|b", "backslash before ordinary char '|' must be literal");
    assert_eq!(
        split_command_string_for_platform("echo \"a\\\"b\\\\c\\$d\"", "linux"),
        vec!["echo", "a\"b\\c$d"]
    );
    let unquoted = split_command_string_for_platform("echo a\\ b", "linux");
    assert_eq!(unquoted, vec!["echo", "a b"]);
    assert_eq!(unquoted.len(), 2, "escaped space must not split the token");

    // variable/tilde expansion routing
    for command in [
        "cat \"$HOME/.zshrc\"", "cat $HOME/.zshrc", "echo ${PATH}", "cat ~/notes.txt", "ls ~", "ls ~bob/dir",
    ] {
        assert!(contains_shell_syntax(command), "{command}");
    }
    for command in ["echo '$HOME'", "rg '~/x' .", "git show HEAD~1", "echo \\$HOME", "echo a$"] {
        assert!(!contains_shell_syntax(command), "{command}");
    }

    // leading env assignment uses shell
    let command = "TOKENZERO_SHELL_CAPTURE_BYTES=12 tokenzero --version";
    assert!(contains_shell_syntax(command));
    let plan = plan_command_for_platform(&[command.to_string()], None, false, "linux").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.argv, vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()]);
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
fn stream_capture_spill_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let policy = test_policy(dir.path());
    let stream = capture_reader(std::io::Cursor::new(vec![b'a'; 20]), "stdout", policy.clone()).unwrap();
    assert_eq!(stream.text, "aaaaaaaaaa");
    assert_eq!((stream.capture.bytes_seen, stream.capture.captured_bytes, stream.capture.spill_bytes), (20, 10, 20));
    assert!(stream.capture.truncated);
    let spill_content = std::fs::read(stream.capture.spill_path.as_ref().unwrap()).unwrap();
    assert_eq!(spill_content.len(), 20);
    assert!(spill_content.iter().all(|&b| b == b'a'));

    let payload = vec![b'z'; 20_000];
    let stream = capture_reader(std::io::Cursor::new(payload.clone()), "stdout", policy).unwrap();
    assert_eq!(std::fs::read(stream.capture.spill_path.unwrap()).unwrap(), payload);
    assert_eq!(stream.capture.spill_bytes, 20_000);
}

#[cfg(not(windows))]
#[test]
fn timeout_kills_child_while_large_stdin_write_is_blocked() {
    let input = "x".repeat(8 * 1024 * 1024);
    let argv = vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()];
    let start = Instant::now();
    let result = run_command(&argv, None, None, Some(&input), Duration::from_millis(150), false).unwrap();
    assert!(result.timed_out && !result.ok && result.exit_code != Some(0));
    assert!(start.elapsed() < Duration::from_secs(4), "timeout was not enforced while stdin write was blocked");
}

#[cfg(not(windows))]
#[test]
fn fast_command_with_background_child_returns_promptly_without_timeout() {
    let argv = vec!["/bin/sh".into(), "-c".into(), "sleep 5 & echo started".into()];
    let start = Instant::now();
    let result = run_command(&argv, None, None, None, Duration::from_secs(30), false).unwrap();
    assert!(result.ok && !result.timed_out && result.io_grace_expired, "{result:?}");
    assert_eq!(result.exit_code, Some(0));
    assert!(result.stdout.contains("started"), "{result:?}");
    assert!(start.elapsed() < Duration::from_secs(4), "foreground exit must not wait for the background child");
}

/// wqw.4: timeout SIGTERM/SIGKILL whole group; partial stdout preserved.
#[cfg(unix)]
#[test]
fn timeout_process_group_kill_leaves_no_orphans_and_keeps_partial_stdout() {
    use std::fs;
    use std::process::Command as SysCommand;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let pidfile = dir.path().join("grandchild.pid");
    let pidfile_str = pidfile.display().to_string();
    let script = format!(
        "set -e\necho 'partial-before-timeout-wqw4'\n(sleep 120) &\necho $! > '{pidfile_str}'\nsleep 120\n"
    );
    let argv = vec!["/bin/sh".into(), "-c".into(), script];
    let start = Instant::now();
    let result = run_command(&argv, None, None, None, Duration::from_millis(250), false).unwrap();
    assert!(result.timed_out && !result.ok, "{result:?}");
    assert!(result.stdout.contains("partial-before-timeout-wqw4"), "{:?}", result.stdout);
    assert!(start.elapsed() < Duration::from_secs(5), "timeout must not wait for sleep 120");
    std::thread::sleep(Duration::from_millis(200));
    if let Some(pid) = fs::read_to_string(&pidfile).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
        let still_alive = SysCommand::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!still_alive, "grandchild pid {pid} must not be an orphan after process-group timeout kill");
    } else {
        let ps = SysCommand::new("ps").args(["-axo", "pid=,command="]).output().expect("ps");
        let listing = String::from_utf8_lossy(&ps.stdout);
        assert!(!listing.contains(&pidfile_str), "no process should still reference the pidfile path: {listing}");
    }
}

#[cfg(not(windows))]
#[test]
fn shell_timeout_is_configurable_short_vs_long() {
    let argv = vec!["/bin/sh".into(), "-c".into(), "sleep 2".into()];
    let short = run_command(&argv, None, None, None, Duration::from_millis(100), false).unwrap();
    assert!(short.timed_out, "100ms timeout must fire on sleep 2");
    let long = run_command(&argv, None, None, None, Duration::from_secs(10), false).unwrap();
    assert!(!long.timed_out && long.ok, "10s budget must allow sleep 2: {long:?}");
}

fn windows_shell_plan_matrix() {
    let script = "$tzTmp = Join-Path $env:TEMP 'tz-quote'; [Console]::Out.Write($tzTmp)".to_string();
    let plan = plan_command_for_platform(&[script.clone()], None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!((plan.shell.as_deref(), plan.shell_arg.as_deref()), (Some("powershell"), Some("-Command")));
    assert_eq!(plan.argv, vec!["powershell".to_string(), "-NoProfile".to_string(), "-Command".to_string(), script]);
    assert!(!plan.alias_dependency && !plan.explicit_binary);

    let plan = plan_command_for_platform(&argv(&["echo", "ok"]), None, false, "windows").unwrap();
    assert_eq!(plan.execution_mode, ExecutionMode::Shell);
    assert_eq!(plan.argv, vec!["cmd", "/C", "echo ok"]);
    assert_eq!((plan.shell.as_deref(), plan.shell_arg.as_deref()), (Some("cmd"), Some("/C")));
    assert!(!plan.alias_dependency && !plan.explicit_binary);
}
#[test]
fn quoting_preserves_spaces() {
    let cases: &[(&str, &str, &str)] = &[
        ("posix", "a b", "'a b'"),
        ("powershell", "a'b", "'a''b'"),
        ("cmd", "C:\\Program Files\\tz", "\"C:\\Program Files\\tz\""),
        ("cmd", "%PATH%", "\"%%PATH%%\""),
        ("cmd", "a^b", "\"a^^b\""),
        ("cmd", "a\"b", "\"a\\\"b\""),
    ];
    for &(platform, input, expected) in cases {
        let actual = match platform {
            "posix" => quote_posix(input),
            "powershell" => quote_powershell(input),
            _ => quote_windows_cmd(input),
        };
        assert_eq!(actual, expected, "platform={platform} input={input:?}");
    }
    assert_eq!(
        split_command_string_for_platform(&format!("tool {}", quote_posix("a b")), "linux"),
        vec!["tool", "a b"]
    );
    assert_eq!(
        split_command_string_for_platform(
            &format!("tool {}", quote_windows_cmd("C:\\Program Files\\tz")),
            "cmd",
        ),
        vec!["tool", "C:\\Program Files\\tz"]
    );
    assert_ne!(quote_windows_cmd("%PATH%"), "%PATH%", "percent signs must be doubled to suppress expansion");
}

fn write_spill_aged(dir: &Path, name: &str, bytes: usize, age: Duration) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, vec![b'x'; bytes]).unwrap();
    File::options().write(true).open(&path).unwrap()
        .set_modified(SystemTime::now() - age).unwrap();
    path
}

fn assert_spill_prune_kept(old: &Path, fresh: &Path, foreign: &Path, report: &SpillPruneReport) {
    assert!(!old.exists(), "expired spill must be reclaimed");
    assert!(fresh.exists(), "fresh spill must survive");
    assert!(foreign.exists(), "non-spill files must never be touched");
    assert_eq!((report.scanned_files, report.removed_files, report.removed_bytes, report.kept_files, report.kept_bytes, report.failed_removals), (2, 1, 10, 1, 20, 0));
}

#[test]
fn spill_prune_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let old = write_spill_aged(dir.path(), "tokenzero-1-1-stdout.log", 10, Duration::from_secs(48 * 60 * 60));
    let fresh = write_spill_aged(dir.path(), "tokenzero-2-2-stderr.log", 20, Duration::from_secs(60));
    let foreign = write_spill_aged(dir.path(), "user-notes.log", 30, Duration::from_secs(48 * 60 * 60));
    let report = prune_spill_dir(dir.path(), DEFAULT_SPILL_TTL, DEFAULT_SPILL_MAX_TOTAL_BYTES, false);
    assert_spill_prune_kept(&old, &fresh, &foreign, &report);

    let dir = tempfile::tempdir().unwrap();
    let oldest = write_spill_aged(dir.path(), "tokenzero-1-1-stdout.log", 60, Duration::from_secs(300));
    let middle = write_spill_aged(dir.path(), "tokenzero-2-2-stdout.log", 60, Duration::from_secs(200));
    let newest = write_spill_aged(dir.path(), "tokenzero-3-3-stdout.log", 60, Duration::from_secs(100));
    let report = prune_spill_dir(dir.path(), DEFAULT_SPILL_TTL, 130, false);
    assert!(!oldest.exists() && middle.exists() && newest.exists());
    assert_eq!((report.removed_files, report.kept_bytes), (1, 120));

    let dir = tempfile::tempdir().unwrap();
    let old = write_spill_aged(dir.path(), "tokenzero-1-1-stdout.log", 10, Duration::from_secs(48 * 60 * 60));
    let report = prune_spill_dir(dir.path(), DEFAULT_SPILL_TTL, DEFAULT_SPILL_MAX_TOTAL_BYTES, true);
    assert!(old.exists());
    assert_eq!((report.dry_run, report.removed_files, report.removed_bytes), (true, 1, 10));

    let missing = std::env::temp_dir().join(format!("tokenzero-missing-spill-{}", std::process::id()));
    let report = prune_spill_dir(&missing, DEFAULT_SPILL_TTL, DEFAULT_SPILL_MAX_TOTAL_BYTES, false);
    assert_eq!((report.scanned_files, report.removed_files, report.kept_files, report.failed_removals), (0, 0, 0, 0));
}
