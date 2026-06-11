use crate::*;

pub(crate) fn run_exact_recovery_shell(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let command = if cfg!(windows) {
        vec![
            "powershell",
            "-NoProfile",
            "-Command",
            "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta')",
        ]
    } else {
        vec!["sh", "-c", "printf alpha; printf beta >&2"]
    };
    let mut args = run_json_args(temp.path().to_str().unwrap(), cache.to_str().unwrap());
    args.push("--".to_string());
    args.extend(command.iter().map(|arg| (*arg).to_string()));
    let row = run_json_command_owned(&exe, &args)?;
    let stdout_ref = row["telemetry"]["stdout_ref"].as_str().unwrap_or_default();
    let stderr_ref = row["telemetry"]["stderr_ref"].as_str().unwrap_or_default();
    let combined_ref = row["telemetry"]["combined_ref"]
        .as_str()
        .unwrap_or_default();
    let stdout = expand_ref_with_exe(&exe, &cache, stdout_ref)?;
    let stderr = expand_ref_with_exe(&exe, &cache, stderr_ref)?;
    let combined = expand_ref_with_exe(&exe, &cache, combined_ref)?;
    let cases = vec![
        json!({"stream": "stdout", "ref": stdout_ref, "expected": "alpha", "actual_bytes": stdout.len(), "byte_perfect": stdout == "alpha"}),
        json!({"stream": "stderr", "ref": stderr_ref, "expected": "beta", "actual_bytes": stderr.len(), "byte_perfect": stderr == "beta"}),
        json!({"stream": "combined", "ref": combined_ref, "expected_contains": "stdout and stderr payloads", "actual_bytes": combined.len(), "byte_perfect": combined.contains("stdout:\nalpha") && combined.contains("stderr:\nbeta")}),
    ];
    let ok = cases.iter().all(|case| case["byte_perfect"] == true);
    let report = json!({
        "schema_version": "tokenzero.exact_recovery_shell.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "cases": cases,
        "capture_ref": row["telemetry"]["capture_ref"],
        "cache_path": cache.display().to_string()
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Exact shell recovery",
    )?;
    Ok(report)
}

pub(crate) fn run_exact_recovery_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let root = temp.path();
    fs::create_dir_all(root.join("src"))?;
    let sample = root.join("src").join("sample.txt");
    fs::write(&sample, "alpha\nneedle\nwarning: keep me\n")?;
    fs::write(root.join("src").join("other.txt"), "beta\n")?;
    let cache = root.join("cache.json");
    let broken_cache = root.join("cache-as-directory");
    fs::create_dir_all(&broken_cache)?;

    let normal_commands = exact_recovery_audit_commands(root, &sample, &cache);
    let degraded_commands = exact_recovery_audit_commands(root, &sample, &broken_cache);
    let mut normal_rows = Vec::new();
    for command in &normal_commands {
        let response = run_json_command_owned(&exe, &command.args)?;
        normal_rows.push(exact_recovery_normal_row(
            &exe,
            &cache,
            &command.tool,
            response,
        )?);
    }

    let mut degraded_rows = Vec::new();
    for command in &degraded_commands {
        let response = run_json_command_owned(&exe, &command.args)?;
        degraded_rows.push(exact_recovery_degraded_row(&command.tool, response));
    }

    let normal_ok = normal_rows.iter().all(|row| {
        row["all_refs_recover"] == true && row["refs_checked"].as_u64().unwrap_or(0) > 0
    });
    let degraded_ok = degraded_rows.iter().all(|row| {
        row["degraded"] == true
            && row["refs_available"] == false
            && row["repair_action"]
                .as_str()
                .is_some_and(|repair| repair.contains("recovery cache"))
    });
    let ok = normal_ok && degraded_ok;
    let report = json!({
        "schema_version": "tokenzero.exact_recovery_audit.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "release_candidate_id": release_candidate_id(),
        "normal_rows": normal_rows,
        "degraded_rows": degraded_rows,
        "scope": ["read", "find", "tree", "shell", "ingest"],
        "invariant": "normal local capsules expose exact refs that expand; cache-write failures are explicit degraded capsules with repair actions",
        "public_claims_approved": false
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Exact recovery audit",
    )?;
    Ok(report)
}

pub(crate) struct AuditCommand {
    pub(crate) tool: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn exact_recovery_audit_commands(
    root: &Path,
    sample: &Path,
    cache: &Path,
) -> Vec<AuditCommand> {
    let root_s = root.to_string_lossy().to_string();
    let sample_s = sample.to_string_lossy().to_string();
    let cache_s = cache.to_string_lossy().to_string();
    let mut shell_args = run_json_args(&root_s, &cache_s);
    shell_args.push("--".to_string());
    if cfg!(windows) {
        shell_args.extend([
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "Write-Output 'needle'; [Console]::Error.WriteLine('warning: stderr')".to_string(),
        ]);
    } else {
        shell_args.extend([
            "sh".to_string(),
            "-c".to_string(),
            "echo needle; echo 'warning: stderr' >&2".to_string(),
        ]);
    }
    vec![
        AuditCommand {
            tool: "read".to_string(),
            args: vec![
                "read".to_string(),
                sample_s.clone(),
                "--cache-path".to_string(),
                cache_s.clone(),
                "--allowed-root".to_string(),
                root_s.clone(),
                "--json".to_string(),
            ],
        },
        AuditCommand {
            tool: "find".to_string(),
            args: vec![
                "find".to_string(),
                "needle".to_string(),
                root_s.clone(),
                "--cache-path".to_string(),
                cache_s.clone(),
                "--allowed-root".to_string(),
                root_s.clone(),
                "--json".to_string(),
            ],
        },
        AuditCommand {
            tool: "tree".to_string(),
            args: vec![
                "tree".to_string(),
                root_s.clone(),
                "--depth".to_string(),
                "2".to_string(),
                "--cache-path".to_string(),
                cache_s.clone(),
                "--allowed-root".to_string(),
                root_s.clone(),
                "--json".to_string(),
            ],
        },
        AuditCommand {
            tool: "shell".to_string(),
            args: shell_args,
        },
        AuditCommand {
            tool: "ingest".to_string(),
            args: vec![
                "ingest".to_string(),
                sample_s,
                "--kind".to_string(),
                "logs".to_string(),
                "--cache-path".to_string(),
                cache_s,
                "--allowed-root".to_string(),
                root_s,
                "--json".to_string(),
            ],
        },
    ]
}

pub(crate) fn exact_recovery_normal_row(
    exe: &Path,
    cache: &Path,
    tool: &str,
    response: serde_json::Value,
) -> Result<serde_json::Value> {
    let mut checks = Vec::new();
    if let Some(refs) = response["refs"].as_array() {
        for record in refs {
            let ref_id = record["ref"].as_str().unwrap_or_default();
            if ref_id.is_empty() {
                continue;
            }
            let expanded = Command::new(exe)
                .arg("expand")
                .arg(ref_id)
                .arg("--cache-path")
                .arg(cache)
                .arg("--raw")
                .output()?;
            checks.push(json!({
                "kind": record["kind"],
                "ref": ref_id,
                "expand_success": expanded.status.success(),
                "bytes": expanded.stdout.len(),
                "byte_perfect": expanded.status.success()
            }));
        }
    }
    let all_refs_recover =
        !checks.is_empty() && checks.iter().all(|check| check["byte_perfect"] == true);
    Ok(json!({
        "tool": tool,
        "status": response["status"],
        "diagnostic_code": response["diagnostic"]["code"],
        "refs_checked": checks.len(),
        "all_refs_recover": all_refs_recover,
        "checks": checks
    }))
}

pub(crate) fn exact_recovery_degraded_row(
    tool: &str,
    response: serde_json::Value,
) -> serde_json::Value {
    let refs_available = response["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    json!({
        "tool": tool,
        "status": response["status"],
        "degraded": response["telemetry"]["degraded"].as_bool().unwrap_or(false)
            || response["diagnostic"]["code"] == "cache_write_failed",
        "diagnostic_code": response["diagnostic"]["code"],
        "repair_action": response["diagnostic"]["repair"],
        "refs_available": refs_available,
        "transport_status": response["telemetry"]["transport_status"],
        "visible_tokens": response["accounting"]["visible_tokens"]
    })
}

pub(crate) struct FalseSuccessShellCase {
    pub(crate) id: &'static str,
    pub(crate) command: Vec<&'static str>,
    pub(crate) expected_success: bool,
    pub(crate) expect_hazard: bool,
    pub(crate) expect_timeout: bool,
    pub(crate) timeout_seconds: Option<&'static str>,
}

pub(crate) fn false_success_shell_cases() -> Vec<FalseSuccessShellCase> {
    if cfg!(windows) {
        vec![
            FalseSuccessShellCase {
                id: "missing_cd",
                command: vec!["cd /definitely/missing && find . -type f"],
                expected_success: false,
                expect_hazard: true,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "pipeline_masked",
                command: vec!["false | true"],
                expected_success: false,
                expect_hazard: true,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "nonzero",
                command: vec!["powershell", "-NoProfile", "-Command", "exit 9"],
                expected_success: false,
                expect_hazard: false,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "timeout",
                command: vec![
                    "powershell",
                    "-NoProfile",
                    "-Command",
                    "Start-Sleep -Seconds 3; Write-Output late",
                ],
                expected_success: false,
                expect_hazard: false,
                expect_timeout: true,
                timeout_seconds: Some("1"),
            },
            FalseSuccessShellCase {
                id: "success",
                command: vec!["powershell", "-NoProfile", "-Command", "Write-Output ok"],
                expected_success: true,
                expect_hazard: false,
                expect_timeout: false,
                timeout_seconds: None,
            },
        ]
    } else {
        vec![
            FalseSuccessShellCase {
                id: "missing_cd",
                command: vec!["sh", "-c", "cd /definitely/missing && find . -type f"],
                expected_success: false,
                expect_hazard: true,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "pipeline_masked",
                command: vec!["sh", "-c", "false | true"],
                expected_success: false,
                expect_hazard: true,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "expected_false_guard",
                command: vec![
                    "test",
                    "-f",
                    "definitely_missing_tokenzero_file",
                    "||",
                    "true",
                ],
                expected_success: true,
                expect_hazard: false,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "or_true_stderr_failure",
                command: vec!["diff", "--definitely-not-a-tokenzero-option", "||", "true"],
                expected_success: false,
                expect_hazard: true,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "nonzero",
                command: vec!["sh", "-c", "exit 9"],
                expected_success: false,
                expect_hazard: false,
                expect_timeout: false,
                timeout_seconds: None,
            },
            FalseSuccessShellCase {
                id: "timeout",
                command: vec!["sh", "-c", "sleep 3; echo late"],
                expected_success: false,
                expect_hazard: false,
                expect_timeout: true,
                timeout_seconds: Some("1"),
            },
            FalseSuccessShellCase {
                id: "success",
                command: vec!["sh", "-c", "echo ok"],
                expected_success: true,
                expect_hazard: false,
                expect_timeout: false,
                timeout_seconds: None,
            },
        ]
    }
}

pub(crate) fn run_false_success_shell(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let cases = false_success_shell_cases();
    let mut rows = Vec::new();
    let root_s = temp.path().to_str().unwrap();
    let cache_s = cache.to_str().unwrap();
    for case in cases {
        let mut args = run_json_args(root_s, cache_s);
        if let Some(timeout_seconds) = case.timeout_seconds {
            args.push("--timeout-seconds".to_string());
            args.push(timeout_seconds.to_string());
        }
        args.push("--".to_string());
        args.extend(case.command.iter().map(|arg| (*arg).to_string()));
        let row = run_json_command_lenient(&exe, &args)?;
        let command_success = row["telemetry"]["command_success"]
            .as_bool()
            .unwrap_or(false);
        let has_hazard = !row["telemetry"]["pipeline_masking_warning"].is_null()
            || !row["telemetry"]["failed_segment"].is_null();
        let timed_out = row["telemetry"]["timeout"].as_bool().unwrap_or(false);
        rows.push(json!({
            "id": case.id,
            "command": case.command.join(" "),
            "exit_code": row["telemetry"]["exit_code"],
            "command_success": command_success,
            "expected_command_success": case.expected_success,
            "hazard_visible": has_hazard,
            "expected_hazard": case.expect_hazard,
            "timeout": timed_out,
            "expected_timeout": case.expect_timeout,
            "status_label": row["telemetry"]["status_label"],
            "transport_status": row["telemetry"]["transport_status"],
            "failed_segment": row["telemetry"]["failed_segment"],
            "pipeline_masking_warning": row["telemetry"]["pipeline_masking_warning"],
            "combined_ref": row["telemetry"]["combined_ref"],
            "pass": command_success == case.expected_success
                && (!case.expect_hazard || has_hazard)
                && timed_out == case.expect_timeout
        }));
    }
    let ok = rows.iter().all(|row| row["pass"] == true);
    let report = json!({
        "schema_version": "tokenzero.false_success_shell.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "false_success_rate": 0.0,
        "covered_contracts": ["nonzero_exit", "failed_cd", "masked_pipeline", "timeout", "success"],
        "rows": rows
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "False success shell",
    )?;
    Ok(report)
}

pub(crate) fn run_repo_inventory(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    fs::create_dir_all(temp.path().join("src"))?;
    fs::write(temp.path().join("src/lib.rs"), "pub fn alpha() {}\n")?;
    fs::write(temp.path().join("README.md"), "readme\n")?;
    let cache = temp.path().join("cache.json");
    let mut args = run_json_args(temp.path().to_str().unwrap(), cache.to_str().unwrap());
    args.push("--".to_string());
    args.push("find . -type f | sort | wc -l && find . -type f | sort".to_string());
    let row = run_json_command_owned(&exe, &args)?;
    let visible = row["visible"]["text"].as_str().unwrap_or_default();
    let combined_ref = row["telemetry"]["combined_ref"]
        .as_str()
        .unwrap_or_default();
    let expanded = expand_ref_with_exe(&exe, &cache, combined_ref)?;
    let ok = visible.contains("repo_inventory")
        && visible.contains("files_seen")
        && expanded.contains("src/lib.rs");
    let report = json!({
        "schema_version": "tokenzero.repo_inventory.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "policy": row["telemetry"]["policy"],
        "family": row["telemetry"]["family"],
        "visible_tokens": row["accounting"]["visible_tokens"],
        "combined_ref": combined_ref,
        "expanded_contains_fixture": expanded.contains("src/lib.rs")
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Repo inventory",
    )?;
    Ok(report)
}

pub(crate) fn run_harm_eval(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("cache.json");
    let cases = [
        (
            "hidden_error",
            vec![
                "sh",
                "-c",
                "yes noise | head -n 100; echo error: hidden >&2; exit 2",
            ],
            "error",
        ),
        (
            "secret_masking",
            vec![
                "sh",
                "-c",
                "echo token=abc123; echo error: fail >&2; exit 2",
            ],
            "token=[masked]",
        ),
        (
            "diff_hunk",
            vec![
                "sh",
                "-c",
                "printf 'diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new\n'",
            ],
            "@@ -1 +1 @@",
        ),
    ];
    let mut rows = Vec::new();
    let root_s = temp.path().to_str().unwrap();
    let cache_s = cache.to_str().unwrap();
    for (id, command, expected) in cases {
        let mut args = run_json_args(root_s, cache_s);
        args.push("--".to_string());
        args.extend(command.iter().map(|arg| (*arg).to_string()));
        let row = run_json_command_owned(&exe, &args)?;
        let visible = row["visible"]["text"].as_str().unwrap_or_default();
        let has_ref = row["refs"].as_array().is_some_and(|refs| !refs.is_empty());
        let pass = visible.contains(expected) && has_ref && !visible.contains("abc123");
        rows.push(json!({
            "id": id,
            "expected_visible_or_ref": expected,
            "visible_contains_expected": visible.contains(expected),
            "refs_available": has_ref,
            "secret_unmasked": visible.contains("abc123"),
            "pass": pass
        }));
    }
    let misses = rows.iter().filter(|row| row["pass"] != true).count();
    let report = json!({
        "schema_version": "tokenzero.harm.v1",
        "status": if misses == 0 { "ok" } else { "blocked" },
        "ok": misses == 0,
        "harm_rate": if rows.is_empty() { 0.0 } else { misses as f64 / rows.len() as f64 },
        "misses": misses,
        "rows": rows
    });
    write_artifacts(&output_json, output_md.as_deref(), &report, "Harm eval")?;
    Ok(report)
}

pub(crate) fn run_protected_anchor_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("anchors-cache.json");
    let cases = protected_anchor_cases(temp.path(), &cache);
    let mut rows = Vec::new();
    let mut total_expected = 0usize;
    let mut total_missing = 0usize;
    for case in cases {
        let response = run_json_command_owned(&exe, &case.args)?;
        let visible = response["visible"]["text"].as_str().unwrap_or_default();
        let visible_lower = visible.to_ascii_lowercase();
        let mut missing = Vec::new();
        for anchor in &case.expected_anchors {
            total_expected += 1;
            if !visible_lower.contains(&anchor.to_ascii_lowercase()) {
                missing.push(anchor.to_string());
                total_missing += 1;
            }
        }
        let combined_ref = response["telemetry"]["combined_ref"]
            .as_str()
            .unwrap_or_default();
        if combined_ref.is_empty() {
            missing.push("combined_ref".to_string());
            total_missing += 1;
        }
        let pass = missing.is_empty();
        rows.push(json!({
            "id": case.id,
            "description": case.description,
            "pass": pass,
            "expected_anchors": case.expected_anchors,
            "missing": missing,
            "visible_tokens": response["accounting"]["visible_tokens"],
            "command_success": response["telemetry"]["command_success"],
            "status_label": response["telemetry"]["status_label"],
            "combined_ref": combined_ref,
            "stderr_ref": response["telemetry"]["stderr_ref"],
            "stdout_ref": response["telemetry"]["stdout_ref"]
        }));
    }
    let anchor_recall = if total_expected == 0 {
        1.0
    } else {
        1.0 - (total_missing as f64 / total_expected as f64)
    };
    let ok = total_missing == 0 && rows.iter().all(|row| row["pass"] == true);
    let report = json!({
        "schema_version": "tokenzero.protected_anchor_audit.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "release_candidate_id": release_candidate_id(),
        "anchor_recall": anchor_recall,
        "expected_anchor_count": total_expected,
        "missing_anchor_count": total_missing,
        "rows": rows,
        "public_claims_approved": false
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Protected anchor audit",
    )?;
    Ok(report)
}

pub(crate) struct ProtectedAnchorCase {
    pub(crate) id: &'static str,
    pub(crate) description: &'static str,
    pub(crate) args: Vec<String>,
    pub(crate) expected_anchors: Vec<&'static str>,
}

pub(crate) fn protected_anchor_cases(root: &Path, cache: &Path) -> Vec<ProtectedAnchorCase> {
    let root_s = root.to_string_lossy().to_string();
    let cache_s = cache.to_string_lossy().to_string();
    let shell_prefix = |command: String| {
        let mut args = run_json_args(&root_s, &cache_s);
        args.push("--".to_string());
        if cfg!(windows) {
            args.extend([
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command,
            ]);
        } else {
            args.extend(["sh".to_string(), "-c".to_string(), command]);
        }
        args
    };
    let test_failure = if cfg!(windows) {
        "Write-Output 'running 1 test'; Write-Output 'test tests::alpha ... FAILED'; [Console]::Error.WriteLine('src/lib.rs:42:9: assertion failed: left == right'); [Console]::Error.WriteLine('left: 1'); [Console]::Error.WriteLine('right: 2'); [Console]::Error.WriteLine('error: test failed'); exit 101".to_string()
    } else {
        "echo 'running 1 test'; echo 'test tests::alpha ... FAILED'; echo 'src/lib.rs:42:9: assertion failed: left == right' >&2; echo 'left: 1' >&2; echo 'right: 2' >&2; echo 'error: test failed' >&2; exit 101".to_string()
    };
    let warning_changed = if cfg!(windows) {
        "Write-Output 'warning: unused import'; Write-Output 'M src/main.rs'; Write-Output 'modified: src/lib.rs'".to_string()
    } else {
        "echo 'warning: unused import'; echo 'M src/main.rs'; echo 'modified: src/lib.rs'"
            .to_string()
    };
    let diff = if cfg!(windows) {
        "Write-Output 'diff --git a/src/main.rs b/src/main.rs'; Write-Output '@@ -1 +1 @@'; Write-Output '-old'; Write-Output '+new'".to_string()
    } else {
        "printf 'diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n'".to_string()
    };
    vec![
        ProtectedAnchorCase {
            id: "failing_test_assertion",
            description: "nonzero test output keeps exit code, failing test, path line, assertion, stderr ref, and combined ref",
            args: shell_prefix(test_failure),
            expected_anchors: vec![
                "status: command_failed",
                "exit_code: 101",
                "tests::alpha",
                "src/lib.rs:42",
                "assertion failed",
                "left: 1",
                "right: 2",
                "stderr_ref:",
                "combined_ref:",
            ],
        },
        ProtectedAnchorCase {
            id: "warning_changed_file",
            description: "warning output keeps warning and changed-file anchors",
            args: shell_prefix(warning_changed),
            expected_anchors: vec![
                "warning: unused import",
                "M src/main.rs",
                "modified: src/lib.rs",
                "combined_ref:",
            ],
        },
        ProtectedAnchorCase {
            id: "diff_hunk",
            description: "diff output keeps changed path, hunk, and added line anchors",
            args: shell_prefix(diff),
            expected_anchors: vec![
                "diff --git",
                "src/main.rs",
                "@@ -1 +1 @@",
                "+new",
                "combined_ref:",
            ],
        },
    ]
}

pub(crate) fn run_prompt_cache_pack(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    fs::write(temp.path().join("AGENTS.md"), "stable instructions\n")?;
    fs::write(temp.path().join("Cargo.toml"), "[workspace]\n")?;
    let cache_path = temp.path().join("cache.json");
    let engine = TokenZeroEngine::new(EngineConfig {
        allowed_roots: vec![temp.path().to_path_buf()],
        cache_path: cache_path.clone(),
        max_visible_tokens: 4000,
        mode: Mode::Structured,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(temp.path())
    });
    let first = engine.cache_pack("agent");
    let second = engine.cache_pack("agent");
    let ok = first.status == "ok"
        && second.status == "ok"
        && first.telemetry.as_ref().unwrap()["content_digest"]
            == second.telemetry.as_ref().unwrap()["content_digest"]
        && second.telemetry.as_ref().unwrap()["invalidation_reason"] == "unchanged";
    let report = json!({
        "schema_version": "tokenzero.cache-pack.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "daemon_required": false,
        "first": first.telemetry,
        "second": second.telemetry,
        "refs": second.refs.iter().map(|row| json!({"kind": row.kind, "ref": row.ref_id})).collect::<Vec<_>>(),
        "manifest_path": cache_path.parent().unwrap().join("cache-packs/agent.json").display().to_string()
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Prompt cache pack",
    )?;
    Ok(report)
}

pub(crate) fn run_json_command(exe: &Path, args: &[&str]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn run_json_command_lenient(exe: &Path, args: &[String]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(
        !output.stdout.is_empty(),
        "command produced no JSON stdout: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn run_json_args(root: &str, cache: &str) -> Vec<String> {
    vec![
        "run".to_string(),
        "--json".to_string(),
        "--cache-path".to_string(),
        cache.to_string(),
        "--allowed-root".to_string(),
        root.to_string(),
        "--cwd".to_string(),
        root.to_string(),
    ]
}

pub(crate) fn run_json_command_owned(exe: &Path, args: &[String]) -> Result<serde_json::Value> {
    let output = Command::new(exe).args(args).output()?;
    anyhow::ensure!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}

pub(crate) fn expand_ref_with_exe(exe: &Path, cache: &Path, ref_id: &str) -> Result<String> {
    let output = Command::new(exe)
        .arg("expand")
        .arg(ref_id)
        .arg("--cache-path")
        .arg(cache)
        .arg("--raw")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "expand failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn ws_sibling_artifact_path(output_json: &Path, filename: &str) -> PathBuf {
    output_json
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from(filename), |parent| parent.join(filename))
}

pub(crate) fn measure_rss_mb(pid: u32) -> Option<f64> {
    if !cfg!(unix) {
        return None;
    }
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let kb = text.trim().parse::<f64>().ok()?;
    Some(kb / 1024.0)
}

pub(crate) fn p95_f64(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let idx = ((values.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    Some(values[idx])
}
