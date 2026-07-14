mod common;
use common::*;

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn cli_run_has_no_alias_dependency() {
    let (_dir, cache) = setup_temp_with_cache();
    let output = tokenzero_cmd()
        .args([
            "run",
            "--json",
            "--cache-path",
            cache.to_str().unwrap(),
            "--",
            "echo",
            "ok",
        ])
        .output()
        .unwrap();
    let output = assert_success(output, "run echo");
    let json = parse_json_stdout(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["telemetry"]["alias_dependency"], false);
    let argv = json["telemetry"]["argv"].as_array().unwrap();
    if cfg!(windows) {
        assert_eq!(json["telemetry"]["execution_mode"], "shell");
        assert_eq!(argv[0], Value::String("cmd".to_string()));
        assert!(argv.contains(&Value::String("echo ok".to_string())));
    } else {
        assert_eq!(json["telemetry"]["execution_mode"], "argv");
        assert!(argv.contains(&Value::String("echo".to_string())));
    }
}

#[cfg(not(windows))]
#[test]
fn shell_inline_budget_zero_disables_small_output_inlining() {
    let (dir, cache) = setup_temp_with_cache();
    std::fs::write(dir.path().join("small.txt"), "tok ".repeat(200)).unwrap();
    let output = tokenzero_cmd()
        .env("TOKENZERO_SHELL_INLINE_BUDGET", "0")
        .args([
            "run",
            "--json",
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            dir.path().to_str().unwrap(),
            "--cwd",
            dir.path().to_str().unwrap(),
            "--",
            "cat",
            "small.txt",
        ])
        .output()
        .unwrap();
    let json = parse_json_stdout(&assert_success(output, "inline budget"));
    let visible = json["visible"]["text"].as_str().unwrap();
    assert!(visible.contains("combined_ref:"), "visible should point at refs: {visible}");
    assert!(
        !visible.contains("tok tok tok tok tok tok tok tok tok tok"),
        "env override disabled inline shell payloads: {visible}"
    );
}

#[test]
fn cli_run_has_status_truth_stream_refs_and_expand() {
    let (dir, cache) = setup_temp_with_cache();
    let command: Vec<&str> = if cfg!(windows) {
        vec![
            "powershell",
            "-NoProfile",
            "-Command",
            "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta'); exit 7",
        ]
    } else {
        vec!["sh", "-c", "printf alpha; printf beta >&2; exit 7"]
    };
    let mut args = vec![
        "run",
        "--json",
        "--mode",
        "auto",
        "--budget",
        "120",
        "--cache-path",
        cache.to_str().unwrap(),
        "--allowed-root",
        dir.path().to_str().unwrap(),
        "--cwd",
        dir.path().to_str().unwrap(),
        "--",
    ];
    args.extend(command);
    let output = assert_success(tokenzero_cmd().args(&args).output().unwrap(), "status truth");
    let json = parse_json_stdout(&output);
    assert_eq!(json["status"], "ok");
    assert_eq!(json["telemetry"]["transport_status"], "ok");
    assert_eq!(json["telemetry"]["command_success"], false);
    assert_eq!(json["telemetry"]["exit_code"], 7);
    assert!(json["refs"].as_array().unwrap().iter().any(|r| r["kind"] == "stdout"));
    assert!(json["refs"].as_array().unwrap().iter().any(|r| r["kind"] == "stderr"));
    let stdout_ref = json["telemetry"]["stdout_ref"].as_str().unwrap();
    let text = expand_raw_text(stdout_ref, Some(&cache), None, &[]);
    assert_eq!(text, "alpha");
}

#[cfg(not(windows))]
#[test]
fn cli_run_text_mode_propagates_child_exit_code() {
    let (dir, cache) = setup_temp_with_cache();
    let run = |command: &[&str]| {
        tokenzero_cmd()
            .args([
                "run",
                "--cache-path",
                cache.to_str().unwrap(),
                "--allowed-root",
                dir.path().to_str().unwrap(),
                "--cwd",
                dir.path().to_str().unwrap(),
                "--",
            ])
            .args(command)
            .output()
            .unwrap()
    };
    assert_eq!(run(&["sh", "-c", "exit 3"]).status.code(), Some(3));
    assert_eq!(run(&["false"]).status.code(), Some(1));
    assert_eq!(run(&["true"]).status.code(), Some(0));
    // Masked pipeline: shell exits 0; warning carries failure evidence.
    assert_eq!(run(&["false | true"]).status.code(), Some(0));
}

#[test]
fn cli_rewrite_accepts_trailing_command_args() {
    for (args, family, rewritten) in [
        (
            &["rewrite", "--json", "--", "git", "status"][..],
            "git",
            "git status",
        ),
        (
            &["rewrite", "--json", "--", "ls", "-la"][..],
            "tree",
            "ls -la",
        ),
        (
            &["rewrite", "--json", "cat README.md"][..],
            "read",
            "tokenzero read README.md",
        ),
    ] {
        let output = assert_success(tokenzero_cmd().args(args).output().unwrap(), "rewrite");
        let json = parse_json_stdout(&output);
        if family != "read" {
            assert_eq!(json["family"], family);
        }
        assert_eq!(json["rewritten_command"], rewritten);
    }
    let missing = tokenzero_cmd().args(["rewrite", "--json"]).output().unwrap();
    assert!(!missing.status.success());
}

#[test]
fn cli_run_pipeline_masking_is_not_reported_as_success() {
    let (dir, cache) = setup_temp_with_cache();
    let json = run_cli_run_json(dir.path(), &cache, &["false | true"]);
    assert_eq!(json["telemetry"]["command_success"], false);
    if !cfg!(windows) {
        assert_eq!(json["telemetry"]["exit_code"], 0);
        assert_eq!(json["telemetry"]["failed_segment"], "false");
        assert!(json["visible"]["text"]
            .as_str()
            .unwrap()
            .contains("pipeline_masking_warning"));
        assert!(json["telemetry"]["pipeline_masking_warning"]
            .as_str()
            .is_some_and(|w| !w.is_empty()));
        assert!(json["telemetry"]["pipeline_rerun_command"]
            .as_str()
            .is_some_and(|c| !c.is_empty()));
    } else {
        assert_eq!(json["telemetry"]["status_label"], "command_failed");
    }
}

#[cfg(not(windows))]
#[test]
fn cli_run_pipeline_masking_includes_pipefail_rerun_command() {
    for (script, rerun) in [
        ("false | true", "bash -o pipefail -c 'false | true'"),
        (
            "printf ok | false | true",
            "bash -o pipefail -c 'printf ok | false | true'",
        ),
    ] {
        let (dir, cache) = setup_temp_with_cache();
        let json = run_cli_run_json(dir.path(), &cache, &[script]);
        assert_eq!(json["telemetry"]["command_success"], false, "{script}");
        assert_eq!(json["telemetry"]["pipeline_rerun_command"], rerun, "{script}");
        assert!(
            json["visible"]["text"]
                .as_str()
                .unwrap()
                .contains(&format!("pipeline_rerun_command: {rerun}")),
            "{script}: {}",
            json["visible"]["text"].as_str().unwrap()
        );
    }
}

#[cfg(not(windows))]
#[test]
fn cli_run_shell_wrapped_rg_keeps_search_summary() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, cache) = setup_temp_with_cache();
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir(&bin_dir).unwrap();
    let rg_path = bin_dir.join("rg");
    std::fs::write(&rg_path, "#!/bin/sh\nprintf 'sample.txt:1:error: tokenzero\\n'\n").unwrap();
    let mut perms = std::fs::metadata(&rg_path).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&rg_path, perms).unwrap();
    let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());
    let path_assignment = format!("PATH={path}");
    let json = run_cli_run_json(
        dir.path(),
        &cache,
        &[
            "env",
            path_assignment.as_str(),
            "bash",
            "-c",
            "rg -P '(?=tokenzero)' sample.txt",
        ],
    );
    assert_eq!(json["telemetry"]["family"], "search");
    assert_eq!(json["telemetry"]["policy"], "structured");
    assert_eq!(json["telemetry"]["command_success"], true);
    let visible = json["visible"]["text"].as_str().unwrap();
    assert!(visible.contains("search_summary"), "{visible}");
    assert!(visible.contains("matches_seen: 1"), "{visible}");
    assert!(visible.contains("sample.txt:1:error: tokenzero"), "{visible}");
}

#[cfg(not(windows))]
#[test]
fn cli_run_inner_script_pipeline_masking_uses_pipefail_rerun() {
    let cases: [(&str, Vec<&str>, &str); 4] = [
        (
            "env-wrap | false | true",
            vec!["env", "TZ=UTC", "bash", "-lc", "false | true"],
            "bash -o pipefail -c 'false | true'",
        ),
        (
            "env-wrap | printf ok | false | true",
            vec!["env", "TZ=UTC", "bash", "-lc", "printf ok | false | true"],
            "bash -o pipefail -c 'printf ok | false | true'",
        ),
        (
            "split-string -S",
            vec!["env", "-S", r#"bash -lc "false | true""#],
            "bash -o pipefail -c 'false | true'",
        ),
        (
            "split-string --split-string",
            vec![
                "env",
                r#"--split-string=bash -lc "printf ok | false | true""#,
            ],
            "bash -o pipefail -c 'printf ok | false | true'",
        ),
    ];
    for (label, trailing_args, rerun) in cases {
        let (dir, cache) = setup_temp_with_cache();
        let json = run_cli_run_json(dir.path(), &cache, &trailing_args);
        assert_eq!(json["telemetry"]["command_success"], false, "{label}");
        assert_eq!(json["telemetry"]["failed_segment"], "false", "{label}");
        assert_eq!(json["telemetry"]["shell_syntax_summary"], "pipeline", "{label}");
        assert_eq!(json["telemetry"]["pipeline_rerun_command"], rerun, "{label}");
        assert!(
            json["visible"]["text"]
                .as_str()
                .unwrap()
                .contains(&format!("pipeline_rerun_command: {rerun}")),
            "{label}: {}",
            json["visible"]["text"].as_str().unwrap()
        );
    }
}

#[cfg(not(windows))]
#[test]
fn cli_run_expected_false_or_true_has_no_masking_warning() {
    let (dir, cache) = setup_temp_with_cache();
    let json = run_cli_run_json(
        dir.path(),
        &cache,
        &[
            "test",
            "-f",
            "definitely-missing-tokenzero-file",
            "||",
            "true",
        ],
    );
    assert_eq!(json["telemetry"]["command_success"], true);
    assert_eq!(json["telemetry"]["status_label"], "command_success");
    assert!(json["telemetry"]["failed_segment"].is_null());
    assert!(json["telemetry"]["pipeline_masking_warning"].is_null());
}

#[cfg(not(windows))]
#[test]
fn cli_run_expected_false_pipelines_preserve_status_truth() {
    let (dir, cache) = setup_temp_with_cache();
    std::fs::write(dir.path().join("left.txt"), b"left\n").unwrap();
    std::fs::write(dir.path().join("right.txt"), b"right\n").unwrap();

    let success_cmds: [&[&str]; 2] = [
        &[
            "test",
            "-f",
            "definitely-missing-tokenzero-file",
            "|",
            "cat",
        ],
        &["cmp", "left.txt", "right.txt", "|", "cat"],
    ];
    for command in success_cmds {
        let json = run_cli_run_json(dir.path(), &cache, command);
        assert_eq!(json["telemetry"]["command_success"], true, "{command:?}");
        assert_eq!(json["telemetry"]["status_label"], "command_success", "{command:?}");
        assert_eq!(json["telemetry"]["shell_syntax_summary"], "pipeline", "{command:?}");
        assert!(json["telemetry"]["failed_segment"].is_null(), "{command:?}");
        assert!(json["telemetry"]["pipeline_masking_warning"].is_null(), "{command:?}");
        assert!(json["telemetry"]["pipeline_rerun_command"].is_null(), "{command:?}");
    }

    let fail_cmds: [(&[&str], &str, &str); 2] = [
        (
            &["cmp", "missing-a", "missing-b", "|", "cat"],
            "cmp missing-a missing-b",
            "bash -o pipefail -c 'cmp missing-a missing-b | cat'",
        ),
        (
            &[
                "test",
                "-f",
                "definitely-missing-tokenzero-file",
                "|",
                "false",
            ],
            "false",
            "bash -o pipefail -c 'test -f definitely-missing-tokenzero-file | false'",
        ),
    ];
    for (command, failed_segment, rerun) in fail_cmds {
        let json = run_cli_run_json(dir.path(), &cache, command);
        assert_eq!(json["telemetry"]["command_success"], false, "{command:?}");
        assert_eq!(json["telemetry"]["status_label"], "command_failed", "{command:?}");
        assert_eq!(json["telemetry"]["shell_syntax_summary"], "pipeline", "{command:?}");
        assert_eq!(json["telemetry"]["failed_segment"], failed_segment, "{command:?}");
        assert!(
            json["telemetry"]["pipeline_masking_warning"]
                .as_str()
                .is_some_and(|warning| warning.contains("mask")),
            "{command:?}: {}",
            json["visible"]["text"].as_str().unwrap()
        );
        assert_eq!(json["telemetry"]["pipeline_rerun_command"], rerun, "{command:?}");
        assert!(
            json["visible"]["text"]
                .as_str()
                .unwrap()
                .contains(&format!("pipeline_rerun_command: {rerun}")),
            "{command:?}: {}",
            json["visible"]["text"].as_str().unwrap()
        );
    }
}

#[cfg(not(windows))]
#[test]
fn cli_run_argv_metacharacters_are_display_quoted() {
    let (dir, cache) = setup_temp_with_cache();
    let json = run_cli_run_json(dir.path(), &cache, &["true", "error|warning"]);
    assert_eq!(json["telemetry"]["command"], "true 'error|warning'");
    assert_eq!(json["telemetry"]["shell_syntax_summary"], "argv/simple");
    assert_eq!(json["telemetry"]["command_success"], true);
    assert!(json["telemetry"]["pipeline_masking_warning"].is_null());
}

#[cfg(not(windows))]
#[test]
fn cli_run_or_true_does_not_hide_stderr_failure() {
    let (dir, cache) = setup_temp_with_cache();
    let json = run_cli_run_json(
        dir.path(),
        &cache,
        &[
            "diff",
            "--definitely-not-a-tokenzero-option",
            "||",
            "true",
        ],
    );
    assert_eq!(json["telemetry"]["command_success"], false);
    assert_eq!(json["telemetry"]["status_label"], "command_failed");
    assert_eq!(
        json["telemetry"]["failed_segment"],
        "diff --definitely-not-a-tokenzero-option"
    );
    assert!(json["telemetry"]["pipeline_masking_warning"]
        .as_str()
        .unwrap()
        .contains("mask"));
}

#[test]
fn cli_run_preserves_multi_arg_shell_operators() {
    let (_dir, cache) = setup_temp_with_cache();
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args([
                "run",
                "--json",
                "--cache-path",
                cache.to_str().unwrap(),
                "--",
                "echo",
                "one",
                "&&",
                "echo",
                "two",
            ])
            .output()
            .unwrap(),
        "multi-arg operators",
    ));
    assert_eq!(json["telemetry"]["execution_mode"], "shell");
    assert_eq!(json["telemetry"]["command_success"], true);
    assert!(
        json["telemetry"]["command"]
            .as_str()
            .unwrap()
            .contains("&&"),
        "command should contain && operator: {:?}",
        json["telemetry"]["command"]
    );
    let stdout_preview = json["telemetry"]["stdout_preview"].as_str().unwrap();
    assert_eq!(
        stdout_preview
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert!(json["telemetry"]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg == "echo one && echo two"));
}

#[cfg(not(windows))]
#[test]
fn cli_run_quotes_multi_arg_shell_operator_literals() {
    let (_dir, cache) = setup_temp_with_cache();
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args([
                "run",
                "--json",
                "--cache-path",
                cache.to_str().unwrap(),
                "--",
                "printf",
                "%s\n",
                "literal; echo TOKENZERO_INJECTED",
                "|",
                "cat",
            ])
            .output()
            .unwrap(),
        "quoted literals",
    ));
    assert_eq!(json["telemetry"]["execution_mode"], "shell");
    assert_eq!(json["telemetry"]["command_success"], true);
    assert_eq!(json["telemetry"]["shell_syntax_summary"], "pipeline");
    assert_eq!(
        json["telemetry"]["command"],
        "printf '%s\n' 'literal; echo TOKENZERO_INJECTED' | cat"
    );
    assert_eq!(
        json["telemetry"]["stdout_preview"],
        "literal; echo TOKENZERO_INJECTED"
    );
}

#[test]
fn cli_default_artifact_output_does_not_overwrite_mcp_smoke() {
    for (subcommand, artifact_name, schema_version) in [
        (
            "shell-matrix",
            "tokenzero_shell_matrix.json",
            "tokenzero.shell_matrix.v1",
        ),
        (
            "exact-recovery-shell",
            "tokenzero_exact_recovery_shell.json",
            "tokenzero.exact_recovery_shell.v1",
        ),
    ] {
        let dir = tempdir().unwrap();
        let results_dir = results_current_dir(dir.path());
        let mcp_smoke = results_dir.join("rust_mcp_smoke.json");
        std::fs::write(&mcp_smoke, br#"{"schema_version":"sentinel.mcp"}"#).unwrap();
        let output = assert_success(
            tokenzero_cmd()
                .current_dir(dir.path())
                .args([subcommand, "--json"])
                .output()
                .unwrap(),
            subcommand,
        );
        let _ = output;
        let artifact_path = results_dir.join(artifact_name);
        assert!(artifact_path.exists(), "{subcommand} default artifact");
        let artifact: Value =
            serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).expect("JSON");
        assert_eq!(artifact["schema_version"], schema_version);
        assert_eq!(
            std::fs::read_to_string(&mcp_smoke).unwrap(),
            r#"{"schema_version":"sentinel.mcp"}"#
        );
    }
}

#[cfg(not(windows))]
#[test]
fn cli_false_success_shell_audit_covers_expected_false_and_masked_failure() {
    let dir = tempdir().unwrap();
    let artifact = dir.path().join("false-success-shell.json");
    let json = parse_json_stdout(&assert_success(
        tokenzero_cmd()
            .args([
                "false-success-shell",
                "--output-json",
                artifact.to_str().unwrap(),
                "--json",
            ])
            .output()
            .unwrap(),
        "false-success-shell",
    ));
    assert_eq!(json["schema_version"], "tokenzero.false_success_shell.v1");
    assert_eq!(json["ok"], true);
    let rows = json["rows"].as_array().unwrap();
    for id in [
        "missing_cd",
        "pipeline_masked",
        "expected_false_guard",
        "or_true_stderr_failure",
        "nonzero",
        "timeout",
        "success",
    ] {
        assert!(
            rows.iter()
                .any(|row| row["id"] == id && row["pass"] == true),
            "missing passing false-success row for {id}: {rows:#?}"
        );
    }
    let expected_false = find_row_by(rows, "id", "expected_false_guard");
    assert_eq!(expected_false["command_success"], true);
    assert_eq!(expected_false["hazard_visible"], false);
    let masked_failure = find_row_by(rows, "id", "or_true_stderr_failure");
    assert_eq!(masked_failure["command_success"], false);
    assert_eq!(masked_failure["hazard_visible"], true);
}
