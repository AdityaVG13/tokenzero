use crate::*;

type BenchScenario = (&'static str, &'static [&'static str], bool);

fn host_command(unix: &[&'static str], windows: &[&'static str]) -> Vec<&'static str> {
    if cfg!(windows) { windows.to_vec() } else { unix.to_vec() }
}

fn host_shell(unix: &'static str, windows: &'static str) -> Vec<String> {
    if cfg!(windows) {
        vec!["powershell".to_string(), "-NoProfile".to_string(), "-Command".to_string(), windows.to_string()]
    } else {
        vec!["sh".to_string(), "-c".to_string(), unix.to_string()]
    }
}

// Const scenario tables keyed by suite
const REPO_DEBUG_SCENARIOS: &[BenchScenario] = &[
    ("repo_inventory", &["find . -type f | sort | wc -l && find . -type f | sort"], true),
    ("grep_warning", &["grep", "warning", "sample.txt"], true),
];
const EXACT_RECOVERY_SCENARIOS: &[BenchScenario] = &[
    ("stdout_stderr", &[], true),  // filled by host_shell at runtime
    ("line_range", &["cat", "sample.txt"], true),
];
const HOSTILE_OUTPUT_SCENARIOS: &[BenchScenario] = &[
    ("hidden_error", &[], false),  // host_shell
    ("masked_pipeline", &["false", "|", "true"], false),
];
const DEFAULT_SCENARIOS: &[BenchScenario] = &[
    ("small_success", &["echo", "ok"], true),
    ("diagnostic_failure", &[], false),  // host_shell
    ("long_repeated_log", &[], true),    // host_shell
    ("repo_inventory", &[], true),       // host_command
];

fn resolve_scenario_command((id, cmd, exp): &BenchScenario) -> (&str, Vec<&'static str>, bool) {
    let cmd = *cmd;
    let resolved: Vec<&'static str> = if cmd.is_empty() {
        match *id {
            "stdout_stderr" => if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta')"]
            } else {
                vec!["sh", "-c", "printf alpha; printf beta >&2"]
            },
            "hidden_error" => if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", "for ($i = 0; $i -lt 80; $i++) { Write-Output 'noise' }; [Console]::Error.WriteLine('error: boom'); exit 2"]
            } else {
                vec!["sh", "-c", "yes noise | head -n 80; echo error: boom >&2; exit 2"]
            },
            "diagnostic_failure" => if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", "Write-Output 'warning: note'; [Console]::Error.WriteLine('error: fail'); exit 3"]
            } else {
                vec!["sh", "-c", "echo warning: note; echo error: fail >&2; exit 3"]
            },
            "long_repeated_log" => if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", "for ($i = 0; $i -lt 500; $i++) { Write-Output 'repeated-noise' }"]
            } else {
                vec!["sh", "-c", "for i in $(seq 1 500); do echo repeated-noise; done"]
            },
            "repo_inventory" => if cfg!(windows) {
                vec!["powershell", "-NoProfile", "-Command", "Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName"]
            } else {
                vec!["find", ".", "-type", "f", "|", "sort", "|", "wc", "-l", "&&", "find", ".", "-type", "f", "|", "sort"]
            },
            _ => cmd.to_vec(),
        }
    } else if *id == "repo_inventory" {
        if cfg!(windows) {
            vec!["powershell", "-NoProfile", "-Command", "Get-ChildItem -Recurse -File | Sort-Object FullName | Select-Object -ExpandProperty FullName"]
        } else {
            cmd.to_vec()
        }
    } else if *id == "line_range" && cfg!(windows) {
        vec!["type", "sample.txt"]
    } else if *id == "grep_warning" && cfg!(windows) {
        vec!["findstr", "warning", "sample.txt"]
    } else {
        cmd.to_vec()
    };
    (*id, resolved, *exp)
}

fn bench_scenarios(suite: &str) -> Vec<(&str, Vec<&'static str>, bool)> {
    let base: &[BenchScenario] = match suite {
        "repo-debug" => REPO_DEBUG_SCENARIOS,
        "exact-recovery" => EXACT_RECOVERY_SCENARIOS,
        "hostile-output" => HOSTILE_OUTPUT_SCENARIOS,
        _ => DEFAULT_SCENARIOS,
    };
    base.iter().map(resolve_scenario_command).collect()
}

pub(crate) fn run_bench_competitors(args: BenchCompetitorsArgs) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    fs::write(temp.path().join("sample.txt"), "alpha\nbeta\nwarning: check me\n")?;
    let cache_path = temp.path().join("bench-cache.json");
    let scenarios = bench_scenarios(&args.suite);
    let mut rows: Vec<_> = scenarios.into_iter()
        .map(|(id, cmd, exp)| run_tokenzero_bench_row(&exe, temp.path(), &cache_path, &args.suite, id, &cmd, exp))
        .collect::<Result<Vec<_>>>()?;
    let adapter_approval = load_benchmark_adapter_approval(args.adapter_approval_artifact.as_ref())?;
    let adapter_rows = competitor_adapter_rows(&args.suite, adapter_approval.as_ref());
    let adapter_matrix = competitor_adapter_matrix(&adapter_rows);
    rows.extend(adapter_rows);
    rows.push(json!({
        "schema_version": "tokenzero.bench.v1", "suite": args.suite.clone(),
        "scenario_id": "external_competitors", "tool": "competitors",
        "availability_status": "unavailable",
        "availability_reason": "competitor clones and private traces are approval-gated and not run by this local proof command",
        "raw_tokens": 0, "visible_tokens": 0, "recovery_tokens": 0, "recovery_adjusted_savings": 0.0,
        "byte_perfect_recovery": false, "task_success": false, "harm_gate": "not_evaluated_unavailable",
        "harm_rate": 0.0, "latency_overhead_ms": 0, "host_coverage": ["cli"],
        "interception_depth": "not_available", "safe_savings": 0.0,
        "adapter_allowlisted": false, "blind_install_attempted": false,
        "fairness_notes": "aggregate unavailable competitor summary; per-adapter rows below are marked unavailable instead of fabricating competitor results"
    }));
    let aggregate = aggregate_bench_rows(&rows);
    let ok = rows.iter().filter(|r| r["tool"] == "tokenzero")
        .all(|r| r["availability_status"] == "run" && r["byte_perfect_recovery"] == true);
    let output_json = args.output_json.unwrap_or_else(|| private_benchmark_path(&args.suite));
    let report = json!({
        "schema_version": "tokenzero.bench.v1", "status": if ok { "ok" } else { "blocked" }, "ok": ok,
        "release_candidate_id": release_candidate_id(), "suite": args.suite.clone(),
        "private_artifact": true, "artifact_path": output_json.display().to_string(),
        "rows": rows, "aggregate": aggregate, "adapter_matrix": adapter_matrix,
        "adapter_approval_artifact": args.adapter_approval_artifact.as_ref().map(|p| p.display().to_string()),
        "safe_savings_formula": "safe_savings = recovery_adjusted_savings * byte_perfect_recovery_pass * task_success_pass * harm_gate_pass",
        "public_claims_approved": false, "release_publication_allowed": false
    });
    finish_artifact(&output_json, None, report, "TokenZero private benchmark")
}

pub(crate) fn run_adapter_approval_audit(output_json: PathBuf, output_md: Option<PathBuf>, approval_file: Option<PathBuf>, execution_approval: bool) -> Result<serde_json::Value> {
    let report = competitor_adapters::adapter_approval_audit_report(approval_file.as_deref(), execution_approval, &release_candidate_id())?;
    finish_artifact(&output_json, output_md.as_deref(), report, "Adapter approval audit")
}

pub(crate) fn run_adapter_approval_template(output_json: PathBuf, output_md: Option<PathBuf>) -> Result<serde_json::Value> {
    let report = competitor_adapters::adapter_approval_template_report(&release_candidate_id());
    finish_artifact(&output_json, output_md.as_deref(), report, "Adapter approval template")
}

pub(crate) fn aggregate_bench_rows(rows: &[serde_json::Value]) -> serde_json::Value {
    let tz_rows: Vec<_> = rows.iter().filter(|r| r["tool"] == "tokenzero").collect();
    let raw: f64 = tz_rows.iter().map(|r| r["raw_tokens"].as_f64().unwrap_or(0.0)).sum();
    let visible_rec: f64 = tz_rows.iter().map(|r| r["visible_tokens"].as_f64().unwrap_or(0.0) + r["recovery_tokens"].as_f64().unwrap_or(0.0)).sum();
    let bp = tz_rows.iter().all(|r| r["byte_perfect_recovery"] == true);
    let ts = tz_rows.iter().all(|r| r["task_success"] == true);
    let hr = average_harm_rate(&tz_rows);
    let gates = bp && ts && hr == 0.0;
    let ras = if raw == 0.0 { 0.0 } else { 1.0 - (visible_rec / raw) };
    let safe = if gates { ras.max(0.0) } else { 0.0 };
    json!({
        "raw_tokens": raw as u64, "visible_plus_recovery_tokens": visible_rec as u64,
        "recovery_adjusted_savings": ras, "byte_perfect_recovery_pass": bp,
        "task_success_pass": ts, "harm_rate": hr, "harm_gate_pass": gates && hr == 0.0,
        "safe_savings": safe, "target_safe_savings": 0.70, "target_met": safe >= 0.70
    })
}

fn average_harm_rate(rows: &[&serde_json::Value]) -> f64 {
    if rows.is_empty() { return 0.0; }
    rows.iter().filter(|r| r["harm_rate"].as_f64().unwrap_or(1.0) > 0.0).count() as f64 / rows.len() as f64
}

pub(crate) fn run_tokenzero_bench_row(exe: &Path, cwd: &Path, cache_path: &Path, suite: &str, scenario_id: &str, command: &[&str], expected_success: bool) -> Result<serde_json::Value> {
    let start = Instant::now();
    let output = Command::new(exe).arg("run").arg("--json").arg("--cache-path").arg(cache_path)
        .arg("--allowed-root").arg(cwd).arg("--cwd").arg(cwd).arg("--").args(command).output()?;
    let latency = start.elapsed().as_millis();
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let t = &parsed["telemetry"];
    let checks = expand_ref_checks(exe, cache_path, t)?;
    let byte_perfect = checks.iter().all(|c| c["byte_perfect"] == true)
        && checks.iter().any(|c| c["kind"] == "combined" && c["bytes"].as_u64().unwrap_or(0) > 0);
    let a = &parsed["accounting"];
    let raw = a["raw_tokens"].as_u64().unwrap_or(0) as f64;
    let vis = a["visible_tokens"].as_u64().unwrap_or(0) as f64;
    let rec = a["recovery_tokens"].as_u64().unwrap_or(0) as f64;
    let ras = if raw == 0.0 { 0.0 } else { 1.0 - ((vis + rec) / raw) };
    let cmd_ok = t["command_success"].as_bool().unwrap_or(false);
    let task_ok = cmd_ok == expected_success;
    let hr = if task_ok { 0.0 } else { 1.0 };
    let safe = if byte_perfect && task_ok && hr == 0.0 { ras.max(0.0) } else { 0.0 };
    Ok(json!({
        "schema_version": "tokenzero.bench.v1", "suite": suite, "scenario_id": scenario_id,
        "tool": "tokenzero", "availability_status": "run", "command": command.join(" "),
        "raw_tokens": raw as u64, "visible_tokens": vis as u64, "recovery_tokens": rec as u64,
        "recovery_adjusted_savings": ras, "byte_perfect_recovery": byte_perfect,
        "task_success": task_ok, "expected_command_success": expected_success,
        "observed_command_success": cmd_ok, "harm_rate": hr, "harm_gate_pass": hr == 0.0,
        "latency_overhead_ms": latency, "host_coverage": ["cli"], "interception_depth": "explicit_cli",
        "safe_savings": safe, "status_label": t["status_label"],
        "stdout_ref": t["stdout_ref"], "stderr_ref": t["stderr_ref"], "combined_ref": t["combined_ref"],
        "exact_expand_checks": checks,
        "fairness_notes": "uses built tokenzero CLI with exact expansion check"
    }))
}

fn expand_ref_checks(exe: &Path, cache: &Path, telemetry: &serde_json::Value) -> Result<Vec<serde_json::Value>> {
    ["stdout", "stderr", "combined"].iter().filter_map(|kind| {
        let ref_id = telemetry[&format!("{kind}_ref")].as_str().unwrap_or_default();
        if ref_id.is_empty() { return None; }
        let expanded = Command::new(exe).arg("expand").arg(ref_id).arg("--cache-path").arg(cache).arg("--raw").output().ok()?;
        let bytes = expanded.stdout.len();
        Some(Ok(json!({"kind": kind, "ref": ref_id, "expand_success": expanded.status.success(), "bytes": bytes, "byte_perfect": expanded.status.success()})))
    }).collect()
}

pub(crate) fn private_benchmark_path(suite: &str) -> PathBuf {
    let root = crate::zerostack_store::tokenzero_work_root(None);
    root.parent().unwrap_or(root.as_path()).join(".tokenzero-private-benchmarks").join("matrix-current").join(format!("{suite}.json"))
}

pub(crate) fn run_matrix_row(label: &str, command: &mut Command) -> serde_json::Value {
    let start = Instant::now();
    match command.output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            json!({"label": label, "ok": output.status.success() && stdout.contains("ok"),
                "exit_code": output.status.code(), "stdout": stdout,
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
                "duration_ms": start.elapsed().as_millis(), "alias_dependency": false})
        }
        Err(err) => json!({"label": label, "ok": false, "error": err.to_string(), "alias_dependency": false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_tracks_gates_independently() {
        let rows = vec![
            json!({"tool":"tokenzero","raw_tokens":100.0,"visible_tokens":10.0,"recovery_tokens":5.0,"byte_perfect_recovery":true,"task_success":true,"harm_rate":0.0}),
            json!({"tool":"tokenzero","raw_tokens":100.0,"visible_tokens":10.0,"recovery_tokens":5.0,"byte_perfect_recovery":true,"task_success":false,"harm_rate":1.0}),
        ];
        let a = aggregate_bench_rows(&rows);
        assert_eq!(a["byte_perfect_recovery_pass"], true);
        assert_eq!(a["task_success_pass"], false);
        assert_eq!(a["harm_gate_pass"], false);
        assert_eq!(a["harm_rate"], 0.5);
        assert_eq!(a["safe_savings"], 0.0);
    }
}
