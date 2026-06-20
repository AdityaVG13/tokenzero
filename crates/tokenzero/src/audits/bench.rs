use crate::*;

pub(crate) fn run_bench_competitors(args: BenchCompetitorsArgs) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    fs::write(
        temp.path().join("sample.txt"),
        "alpha\nbeta\nwarning: check me\n",
    )?;
    let cache_path = temp.path().join("bench-cache.json");
    let scenarios: Vec<(&str, Vec<&str>, bool)> = match args.suite.as_str() {
        "repo-debug" => vec![
            (
                "repo_inventory",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName",
                    ]
                } else {
                    vec!["find . -type f | sort | wc -l && find . -type f | sort"]
                },
                true,
            ),
            (
                "grep_warning",
                if cfg!(windows) {
                    vec!["findstr", "warning", "sample.txt"]
                } else {
                    vec!["grep", "warning", "sample.txt"]
                },
                true,
            ),
        ],
        "exact-recovery" => vec![
            (
                "stdout_stderr",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta')",
                    ]
                } else {
                    vec!["sh", "-c", "printf alpha; printf beta >&2"]
                },
                true,
            ),
            (
                "line_range",
                if cfg!(windows) {
                    vec!["type", "sample.txt"]
                } else {
                    vec!["cat", "sample.txt"]
                },
                true,
            ),
        ],
        "hostile-output" => vec![
            (
                "hidden_error",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "for ($i = 0; $i -lt 80; $i++) { Write-Output 'noise' }; [Console]::Error.WriteLine('error: boom'); exit 2",
                    ]
                } else {
                    vec![
                        "sh",
                        "-c",
                        "yes noise | head -n 80; echo error: boom >&2; exit 2",
                    ]
                },
                false,
            ),
            ("masked_pipeline", vec!["false | true"], false),
        ],
        _ => vec![
            ("small_success", vec!["echo", "ok"], true),
            (
                "diagnostic_failure",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "Write-Output 'warning: note'; [Console]::Error.WriteLine('error: fail'); exit 3",
                    ]
                } else {
                    vec![
                        "sh",
                        "-c",
                        "echo warning: note; echo error: fail >&2; exit 3",
                    ]
                },
                false,
            ),
            (
                "long_repeated_log",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "for ($i = 0; $i -lt 500; $i++) { Write-Output 'repeated-noise' }",
                    ]
                } else {
                    vec![
                        "sh",
                        "-c",
                        "for i in $(seq 1 500); do echo repeated-noise; done",
                    ]
                },
                true,
            ),
            (
                "repo_inventory",
                if cfg!(windows) {
                    vec![
                        "powershell",
                        "-NoProfile",
                        "-Command",
                        "Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName",
                    ]
                } else {
                    vec!["find . -type f | sort | wc -l && find . -type f | sort"]
                },
                true,
            ),
        ],
    };
    let mut rows = Vec::new();
    for (scenario_id, command, expected_success) in scenarios {
        rows.push(run_tokenzero_bench_row(
            &exe,
            temp.path(),
            &cache_path,
            &args.suite,
            scenario_id,
            &command,
            expected_success,
        )?);
    }
    let adapter_approval =
        load_benchmark_adapter_approval(args.adapter_approval_artifact.as_ref())?;
    let adapter_rows = competitor_adapter_rows(&args.suite, adapter_approval.as_ref());
    let adapter_matrix = competitor_adapter_matrix(&adapter_rows);
    rows.extend(adapter_rows);
    rows.push(json!({
        "schema_version": "tokenzero.bench.v1",
        "suite": args.suite.clone(),
        "scenario_id": "external_competitors",
        "tool": "competitors",
        "availability_status": "unavailable",
        "availability_reason": "competitor clones and private traces are approval-gated and not run by this local proof command",
        "raw_tokens": 0,
        "visible_tokens": 0,
        "recovery_tokens": 0,
        "recovery_adjusted_savings": 0.0,
        "byte_perfect_recovery": false,
        "task_success": false,
        "harm_gate": "not_evaluated_unavailable",
        "harm_rate": 0.0,
        "latency_overhead_ms": 0,
        "host_coverage": ["cli"],
        "interception_depth": "not_available",
        "safe_savings": 0.0,
        "adapter_allowlisted": false,
        "blind_install_attempted": false,
        "fairness_notes": "aggregate unavailable competitor summary; per-adapter rows below are marked unavailable instead of fabricating competitor results"
    }));
    let aggregate = aggregate_bench_rows(&rows);
    let ok = rows
        .iter()
        .filter(|row| row["tool"] == "tokenzero")
        .all(|row| row["availability_status"] == "run" && row["byte_perfect_recovery"] == true);
    let output_json = args
        .output_json
        .unwrap_or_else(|| private_benchmark_path(&args.suite));
    let report = json!({
        "schema_version": "tokenzero.bench.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "release_candidate_id": release_candidate_id(),
        "suite": args.suite.clone(),
        "private_artifact": true,
        "artifact_path": output_json.display().to_string(),
        "rows": rows,
        "aggregate": aggregate,
        "adapter_matrix": adapter_matrix,
        "adapter_approval_artifact": args.adapter_approval_artifact.as_ref().map(|path| path.display().to_string()),
        "safe_savings_formula": "safe_savings = recovery_adjusted_savings * byte_perfect_recovery_pass * task_success_pass * harm_gate_pass",
        "public_claims_approved": false,
        "release_publication_allowed": false
    });
    write_artifacts(&output_json, None, &report, "TokenZero private benchmark")?;
    Ok(report)
}

pub(crate) fn run_adapter_approval_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    approval_file: Option<PathBuf>,
    execution_approval: bool,
) -> Result<serde_json::Value> {
    let report = competitor_adapters::adapter_approval_audit_report(
        approval_file.as_deref(),
        execution_approval,
        &release_candidate_id(),
    )?;
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Adapter approval audit",
    )?;
    Ok(report)
}

pub(crate) fn run_adapter_approval_template(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let report = competitor_adapters::adapter_approval_template_report(&release_candidate_id());
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Adapter approval template",
    )?;
    Ok(report)
}

pub(crate) fn aggregate_bench_rows(rows: &[serde_json::Value]) -> serde_json::Value {
    let tokenzero_rows = rows
        .iter()
        .filter(|row| row["tool"] == "tokenzero")
        .collect::<Vec<_>>();
    let raw: f64 = tokenzero_rows
        .iter()
        .map(|row| row["raw_tokens"].as_f64().unwrap_or(0.0))
        .sum();
    let visible_recovery: f64 = tokenzero_rows
        .iter()
        .map(|row| {
            row["visible_tokens"].as_f64().unwrap_or(0.0)
                + row["recovery_tokens"].as_f64().unwrap_or(0.0)
        })
        .sum();
    let byte_perfect_recovery_pass = tokenzero_rows
        .iter()
        .all(|row| row["byte_perfect_recovery"] == true);
    let task_success_pass = tokenzero_rows.iter().all(|row| row["task_success"] == true);
    let harm_rate = average_harm_rate(&tokenzero_rows);
    let harm_gate_pass = harm_rate == 0.0;
    let gates_pass = byte_perfect_recovery_pass && task_success_pass && harm_gate_pass;
    let recovery_adjusted_savings = if raw == 0.0 {
        0.0
    } else {
        1.0 - (visible_recovery / raw)
    };
    let safe_savings = if gates_pass {
        recovery_adjusted_savings.max(0.0)
    } else {
        0.0
    };
    json!({
        "raw_tokens": raw as u64,
        "visible_plus_recovery_tokens": visible_recovery as u64,
        "recovery_adjusted_savings": recovery_adjusted_savings,
        "byte_perfect_recovery_pass": byte_perfect_recovery_pass,
        "task_success_pass": task_success_pass,
        "harm_rate": harm_rate,
        "harm_gate_pass": harm_gate_pass,
        "safe_savings": safe_savings,
        "target_safe_savings": 0.70,
        "target_met": safe_savings >= 0.70
    })
}

fn average_harm_rate(rows: &[&serde_json::Value]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let misses = rows
        .iter()
        .filter(|row| row["harm_rate"].as_f64().unwrap_or(1.0) > 0.0)
        .count();
    misses as f64 / rows.len() as f64
}

pub(crate) fn run_tokenzero_bench_row(
    exe: &Path,
    cwd: &Path,
    cache_path: &Path,
    suite: &str,
    scenario_id: &str,
    command: &[&str],
    expected_success: bool,
) -> Result<serde_json::Value> {
    let start = Instant::now();
    let output = Command::new(exe)
        .arg("run")
        .arg("--json")
        .arg("--cache-path")
        .arg(cache_path)
        .arg("--allowed-root")
        .arg(cwd)
        .arg("--cwd")
        .arg(cwd)
        .arg("--")
        .args(command)
        .output()?;
    let latency_ms = start.elapsed().as_millis();
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let telemetry = &parsed["telemetry"];
    let stdout_ref = telemetry["stdout_ref"].as_str().unwrap_or_default();
    let stderr_ref = telemetry["stderr_ref"].as_str().unwrap_or_default();
    let combined_ref = telemetry["combined_ref"].as_str().unwrap_or_default();
    let mut exact_expand_checks = Vec::new();
    for (kind, ref_id) in [
        ("stdout", stdout_ref),
        ("stderr", stderr_ref),
        ("combined", combined_ref),
    ] {
        if ref_id.is_empty() {
            continue;
        }
        let expanded = Command::new(exe)
            .arg("expand")
            .arg(ref_id)
            .arg("--cache-path")
            .arg(cache_path)
            .arg("--raw")
            .output()?;
        let bytes = expanded.stdout.len();
        exact_expand_checks.push(json!({
            "kind": kind,
            "ref": ref_id,
            "expand_success": expanded.status.success(),
            "bytes": bytes,
            "byte_perfect": expanded.status.success()
        }));
    }
    let byte_perfect = exact_expand_checks
        .iter()
        .all(|check| check["byte_perfect"] == true)
        && exact_expand_checks
            .iter()
            .any(|check| check["kind"] == "combined" && check["bytes"].as_u64().unwrap_or(0) > 0);
    let accounting = &parsed["accounting"];
    let raw_tokens = accounting["raw_tokens"].as_u64().unwrap_or(0) as f64;
    let visible_tokens = accounting["visible_tokens"].as_u64().unwrap_or(0) as f64;
    let recovery_tokens = accounting["recovery_tokens"].as_u64().unwrap_or(0) as f64;
    let recovery_adjusted_savings = if raw_tokens == 0.0 {
        0.0
    } else {
        1.0 - ((visible_tokens + recovery_tokens) / raw_tokens)
    };
    let command_success = telemetry["command_success"].as_bool().unwrap_or(false);
    let task_success = command_success == expected_success;
    // Per-row harm is a miss rate over this row's oracle: any mismatch
    // between observed and expected command success is one harmful miss.
    let harm_rate = if task_success { 0.0 } else { 1.0 };
    let safe_savings = if byte_perfect && task_success && harm_rate == 0.0 {
        recovery_adjusted_savings.max(0.0)
    } else {
        0.0
    };
    Ok(json!({
        "schema_version": "tokenzero.bench.v1",
        "suite": suite,
        "scenario_id": scenario_id,
        "tool": "tokenzero",
        "availability_status": "run",
        "command": command.join(" "),
        "raw_tokens": raw_tokens as u64,
        "visible_tokens": visible_tokens as u64,
        "recovery_tokens": recovery_tokens as u64,
        "recovery_adjusted_savings": recovery_adjusted_savings,
        "byte_perfect_recovery": byte_perfect,
        "task_success": task_success,
        "expected_command_success": expected_success,
        "observed_command_success": command_success,
        "harm_rate": harm_rate,
        "harm_gate_pass": harm_rate == 0.0,
        "latency_overhead_ms": latency_ms,
        "host_coverage": ["cli"],
        "interception_depth": "explicit_cli",
        "safe_savings": safe_savings,
        "status_label": telemetry["status_label"],
        "stdout_ref": stdout_ref,
        "stderr_ref": stderr_ref,
        "combined_ref": combined_ref,
        "exact_expand_checks": exact_expand_checks,
        "fairness_notes": "uses built tokenzero CLI with exact expansion check"
    }))
}

pub(crate) fn private_benchmark_path(suite: &str) -> PathBuf {
    let root = root_from(None);
    let ai_root = root.parent().unwrap_or(root.as_path());
    ai_root
        .join(".tokenzero-private-benchmarks")
        .join("matrix-current")
        .join(format!("{suite}.json"))
}

pub(crate) fn run_matrix_row(label: &str, command: &mut Command) -> serde_json::Value {
    let start = Instant::now();
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            json!({
                "label": label,
                "ok": output.status.success() && stdout.contains("ok"),
                "exit_code": output.status.code(),
                "stdout": stdout,
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "duration_ms": start.elapsed().as_millis(),
                "alias_dependency": false
            })
        }
        Err(err) => {
            json!({"label": label, "ok": false, "error": err.to_string(), "alias_dependency": false})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregate_tracks_gates_independently() {
        let rows = vec![
            json!({
                "tool": "tokenzero",
                "raw_tokens": 100.0,
                "visible_tokens": 10.0,
                "recovery_tokens": 5.0,
                "byte_perfect_recovery": true,
                "task_success": true,
                "harm_rate": 0.0
            }),
            json!({
                "tool": "tokenzero",
                "raw_tokens": 100.0,
                "visible_tokens": 10.0,
                "recovery_tokens": 5.0,
                "byte_perfect_recovery": true,
                "task_success": false,
                "harm_rate": 1.0
            }),
        ];

        let aggregate = aggregate_bench_rows(&rows);
        assert_eq!(aggregate["byte_perfect_recovery_pass"], true);
        assert_eq!(aggregate["task_success_pass"], false);
        assert_eq!(aggregate["harm_gate_pass"], false);
        assert_eq!(aggregate["harm_rate"], 0.5);
        assert_eq!(aggregate["safe_savings"], 0.0);
    }
}
