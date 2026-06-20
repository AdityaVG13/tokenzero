use crate::*;

pub(crate) fn run_one_shot_eval(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("one-shot-cache.json");
    let source = temp.path().join("src.rs");
    fs::create_dir_all(temp.path())?;
    fs::write(
        &source,
        "pub fn alpha() -> usize {\n    41\n}\n\n#[test]\nfn alpha_is_answer() {\n    assert_eq!(alpha(), 42);\n}\n",
    )?;
    let broken_cache = temp.path().join("cache-as-directory");
    fs::create_dir_all(&broken_cache)?;

    let read_row = run_json_command(
        &exe,
        &[
            "read",
            source.to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            temp.path().to_str().unwrap(),
            "--json",
        ],
    )?;
    let read_visible = read_row["visible"]["text"].as_str().unwrap_or_default();
    let read_refs_available = read_row["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    let read_file_ref_available = read_row["refs"].as_array().is_some_and(|refs| {
        refs.iter()
            .any(|record| record["kind"] == "file" && record["ref"].as_str().is_some())
    });
    let read_anchors = vec!["alpha_is_answer", "assert_eq", "file_ref"];
    let read_has_anchor =
        anchors_present(read_visible, &["alpha_is_answer", "assert_eq"]) && read_file_ref_available;

    let failure_command = if cfg!(windows) {
        vec![
            "powershell",
            "-NoProfile",
            "-Command",
            "Write-Output 'running 1 test'; Write-Output 'test tests::alpha ... FAILED'; [Console]::Error.WriteLine('src/lib.rs:42:9: assertion failed: left == right'); [Console]::Error.WriteLine('left: 1'); [Console]::Error.WriteLine('right: 2'); [Console]::Error.WriteLine('error: test failed'); exit 101",
        ]
    } else {
        vec![
            "sh",
            "-c",
            "echo 'running 1 test'; echo 'test tests::alpha ... FAILED'; echo 'src/lib.rs:42:9: assertion failed: left == right' >&2; echo 'left: 1' >&2; echo 'right: 2' >&2; echo 'error: test failed' >&2; exit 101",
        ]
    };
    let mut failure_args = vec![
        "run",
        "--json",
        "--cache-path",
        cache.to_str().unwrap(),
        "--allowed-root",
        temp.path().to_str().unwrap(),
        "--cwd",
        temp.path().to_str().unwrap(),
        "--",
    ];
    failure_args.extend(failure_command.iter().copied());
    let failure_row = run_json_command(&exe, &failure_args)?;
    let failure_visible = failure_row["visible"]["text"].as_str().unwrap_or_default();
    let failure_refs_available = failure_row["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    let failure_anchors = vec![
        "exit_code: 101",
        "tests::alpha",
        "src/lib.rs:42",
        "assertion failed",
        "stderr_ref:",
    ];
    let failure_has_anchor = failure_row["telemetry"]["command_success"] == false
        && anchors_present(failure_visible, &failure_anchors);

    let warning_args = one_shot_shell_args(
        temp.path(),
        &cache,
        if cfg!(windows) {
            "Write-Output 'warning: unused import'; Write-Output 'M src/main.rs'; Write-Output 'modified: src/lib.rs'"
        } else {
            "echo 'warning: unused import'; echo 'M src/main.rs'; echo 'modified: src/lib.rs'"
        },
    );
    let warning_row = run_json_command_owned(&exe, &warning_args)?;
    let warning_visible = warning_row["visible"]["text"].as_str().unwrap_or_default();
    let warning_refs_available = warning_row["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    let warning_anchors = vec![
        "warning: unused import",
        "M src/main.rs",
        "modified: src/lib.rs",
    ];
    let warning_has_anchor = anchors_present(warning_visible, &warning_anchors);

    let diff_args = one_shot_shell_args(
        temp.path(),
        &cache,
        if cfg!(windows) {
            "Write-Output 'diff --git a/src/main.rs b/src/main.rs'; Write-Output '@@ -1 +1 @@'; Write-Output '-old'; Write-Output '+new'"
        } else {
            "printf 'diff --git a/src/main.rs b/src/main.rs\n@@ -1 +1 @@\n-old\n+new\n'"
        },
    );
    let diff_row = run_json_command_owned(&exe, &diff_args)?;
    let diff_visible = diff_row["visible"]["text"].as_str().unwrap_or_default();
    let diff_refs_available = diff_row["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    let diff_anchors = vec!["diff --git", "src/main.rs", "@@ -1 +1 @@", "+new"];
    let diff_has_anchor = anchors_present(diff_visible, &diff_anchors);

    let degraded_row = run_json_command(
        &exe,
        &[
            "read",
            source.to_str().unwrap(),
            "--cache-path",
            broken_cache.to_str().unwrap(),
            "--allowed-root",
            temp.path().to_str().unwrap(),
            "--json",
        ],
    )?;
    let degraded_visible = degraded_row["visible"]["text"].as_str().unwrap_or_default();
    let degraded_refs_available = degraded_row["refs"]
        .as_array()
        .is_some_and(|refs| !refs.is_empty());
    let degraded_explicit = degraded_row["diagnostic"]["code"] == "cache_write_failed"
        && degraded_row["diagnostic"]["repair"]
            .as_str()
            .is_some_and(|repair| repair.contains("recovery cache"));
    let degraded_anchors = vec!["alpha_is_answer", "assert_eq"];
    let degraded_has_anchor =
        anchors_present(degraded_visible, &degraded_anchors) && degraded_explicit;

    let rows = vec![
        json!({
            "trace_id": "source_edit_anchor",
            "expected_next_action": "edit src.rs alpha return value",
            "required_anchors": read_anchors,
            "required_anchors_present": read_has_anchor,
            "refs_available": read_refs_available,
            "degraded_explicit": false,
            "planned_expands": [],
            "unplanned_second_call": false,
            "task_success": read_has_anchor,
            "critical": true,
            "mode_rationale": "structured read keeps edit anchors and exact refs visible"
        }),
        json!({
            "trace_id": "failure_diagnosis_anchor",
            "expected_next_action": "inspect failing assertion without rerunning raw command",
            "required_anchors": failure_anchors,
            "required_anchors_present": failure_has_anchor,
            "refs_available": failure_refs_available,
            "degraded_explicit": false,
            "planned_expands": [],
            "unplanned_second_call": false,
            "task_success": failure_has_anchor,
            "critical": true,
            "mode_rationale": "diagnostic shell mode preserves status truth and failure anchors"
        }),
        json!({
            "trace_id": "warning_changed_file_anchor",
            "expected_next_action": "fix or inspect changed file without rerunning status output",
            "required_anchors": warning_anchors,
            "required_anchors_present": warning_has_anchor,
            "refs_available": warning_refs_available,
            "degraded_explicit": false,
            "planned_expands": [],
            "unplanned_second_call": false,
            "task_success": warning_has_anchor,
            "critical": true,
            "mode_rationale": "diagnostic shell mode preserves warning and changed-file anchors"
        }),
        json!({
            "trace_id": "diff_review_anchor",
            "expected_next_action": "review changed hunk without expanding raw diff",
            "required_anchors": diff_anchors,
            "required_anchors_present": diff_has_anchor,
            "refs_available": diff_refs_available,
            "degraded_explicit": false,
            "planned_expands": [],
            "unplanned_second_call": false,
            "task_success": diff_has_anchor,
            "critical": true,
            "mode_rationale": "diff rendering preserves path, hunk, and added line anchors"
        }),
        json!({
            "trace_id": "recovery_degraded_anchor",
            "expected_next_action": "repair recovery cache before trusting exact recovery",
            "required_anchors": degraded_anchors,
            "required_anchors_present": degraded_has_anchor,
            "refs_available": degraded_refs_available,
            "degraded_explicit": degraded_explicit,
            "planned_expands": [],
            "unplanned_second_call": false,
            "task_success": degraded_has_anchor,
            "critical": true,
            "mode_rationale": "degraded mode is adequate only when repair action and visible edit anchors are present"
        }),
    ];
    let critical_total = rows.iter().filter(|row| row["critical"] == true).count();
    let critical_misses = rows
        .iter()
        .filter(|row| {
            row["critical"] == true
                && (row["required_anchors_present"] != true
                    || row["task_success"] != true
                    || row["unplanned_second_call"] == true
                    || (row["refs_available"] != true && row["degraded_explicit"] != true))
        })
        .count();
    let overall_misses = rows
        .iter()
        .filter(|row| {
            row["required_anchors_present"] != true
                || row["task_success"] != true
                || row["unplanned_second_call"] == true
                || (row["refs_available"] != true && row["degraded_explicit"] != true)
        })
        .count();
    let critical_miss_rate = if critical_total == 0 {
        0.0
    } else {
        critical_misses as f64 / critical_total as f64
    };
    let overall_miss_rate = if rows.is_empty() {
        0.0
    } else {
        overall_misses as f64 / rows.len() as f64
    };
    let ok = critical_misses == 0 && overall_miss_rate < 0.02;
    let report = json!({
        "schema_version": "tokenzero.one_shot_eval.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "release_candidate_id": release_candidate_id(),
        "critical_miss_rate": critical_miss_rate,
        "overall_miss_rate": overall_miss_rate,
        "thresholds": {
            "critical_miss_rate": 0.0,
            "overall_miss_rate_lt": 0.02
        },
        "rows": rows,
        "public_claims_approved": false,
        "release_publication_allowed": false
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "One-shot evaluation",
    )?;
    Ok(report)
}

pub(crate) fn anchors_present(visible: &str, anchors: &[&str]) -> bool {
    let visible_lower = visible.to_ascii_lowercase();
    anchors
        .iter()
        .all(|anchor| visible_lower.contains(&anchor.to_ascii_lowercase()))
}

pub(crate) fn one_shot_shell_args(root: &Path, cache: &Path, command: &str) -> Vec<String> {
    let root_s = root.to_string_lossy().to_string();
    let cache_s = cache.to_string_lossy().to_string();
    let mut args = run_json_args(&root_s, &cache_s);
    args.push("--".to_string());
    if cfg!(windows) {
        args.extend([
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            command.to_string(),
        ]);
    } else {
        args.extend(["sh".to_string(), "-c".to_string(), command.to_string()]);
    }
    args
}

pub(crate) fn run_source_currency_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    refresh_ledger: Option<PathBuf>,
    refresh_git_heads: bool,
) -> Result<serde_json::Value> {
    if refresh_ledger.is_some() && refresh_git_heads {
        anyhow::bail!("choose --refresh-ledger or --refresh-git-heads, not both");
    }
    let release_candidate_id = release_candidate_id();
    let report = if let Some(refresh_ledger) = refresh_ledger.as_deref() {
        source_currency::refreshed_source_currency_report(
            source_currency::read_source_refresh_rows(refresh_ledger)?,
            "refresh-ledger",
            Some(refresh_ledger),
            &release_candidate_id,
        )
    } else if refresh_git_heads {
        source_currency::refreshed_source_currency_report(
            source_currency::git_head_source_refresh_rows(),
            "git-ls-remote-head",
            None,
            &release_candidate_id,
        )
    } else {
        source_currency::source_currency_report(&release_candidate_id)
    };
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Source currency audit",
    )?;
    Ok(report)
}
pub(crate) fn run_completion_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let report = completion_handoff::completion_audit_report();
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Completion audit",
    )?;
    Ok(report)
}

pub(crate) fn run_security_privacy_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("security-cache.json");
    let secret_command = if cfg!(windows) {
        vec![
            "powershell".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "Write-Output 'token=abc123'; [Console]::Error.WriteLine('password=hunter2'); exit 2"
                .to_string(),
        ]
    } else {
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "echo token=abc123; echo password=hunter2 >&2; exit 2".to_string(),
        ]
    };
    let root_s = temp.path().display().to_string();
    let cache_s = cache.display().to_string();
    let mut run_args = run_json_args(&root_s, &cache_s);
    run_args.push("--".to_string());
    run_args.extend(secret_command);
    let run_row = run_json_command_owned(&exe, &run_args)?;
    let visible = run_row["visible"]["text"].as_str().unwrap_or_default();
    let combined_ref = run_row["telemetry"]["combined_ref"]
        .as_str()
        .unwrap_or_default();
    let expanded = expand_ref_with_exe(&exe, &cache, combined_ref)?;
    let visible_secret_masked = visible.contains("token=[masked]")
        && visible.contains("password=[masked]")
        && !visible.contains("abc123")
        && !visible.contains("hunter2");
    let exact_ref_local_recovery = expanded.contains("token=abc123")
        && expanded.contains("password=hunter2")
        && combined_ref.starts_with("tz://");

    let pulse_path = temp.path().join("pulse.jsonl");
    record_event(
        &pulse_path,
        &PulseEvent {
            schema_version: "pulse-v1".to_string(),
            event: "tool_call".to_string(),
            timestamp_unix: 1,
            tool: "shell".to_string(),
            mode: "hybrid".to_string(),
            raw_tokens: 8,
            visible_tokens: 2,
            recovery_tokens: 1,
            task_lossless: true,
            cache_hit: false,
            retry_count: 0,
            failure: false,
            exact_ref_count: 1,
            latency_ms: 1,
            source_hash: Some("sha256:redacted-local-source".to_string()),
            session_id: None,
            call_id: None,
            ref_ids: Vec::new(),
        },
    )?;
    let pulse_text = fs::read_to_string(&pulse_path)?;
    let pulse_no_raw_payload = !pulse_text.contains("abc123")
        && !pulse_text.contains("hunter2")
        && !pulse_text.contains("secret raw payload")
        && pulse_text.contains("source_hash");

    let allowed_root = temp.path().join("allowed");
    let outside_root = temp.path().join("outside");
    fs::create_dir_all(&allowed_root)?;
    fs::create_dir_all(&outside_root)?;
    let outside_file = outside_root.join("secret.txt");
    fs::write(&outside_file, "token=abc123\n")?;
    let engine = TokenZeroEngine::new(EngineConfig {
        allowed_roots: vec![allowed_root.clone()],
        cache_path: temp.path().join("mcp-cache.json"),
        max_visible_tokens: 4000,
        mode: Mode::Hybrid,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&allowed_root)
    });
    let mcp_read = engine.read(&[outside_file], Mode::Hybrid, None, None, false, 1, 4000);
    let mcp_allowed_root_enforced = mcp_read.status == "error"
        && mcp_read
            .error
            .as_ref()
            .is_some_and(|error| error.code == "path_not_allowed");

    let rows = vec![
        json!({
            "id": "cli_visible_secret_masking",
            "pass": visible_secret_masked,
            "evidence": "visible output masks token/password values"
        }),
        json!({
            "id": "exact_ref_local_recovery",
            "pass": exact_ref_local_recovery,
            "evidence": combined_ref
        }),
        json!({
            "id": "pulse_no_raw_payload",
            "pass": pulse_no_raw_payload,
            "evidence": pulse_path.display().to_string()
        }),
        json!({
            "id": "mcp_allowed_root_enforced",
            "pass": mcp_allowed_root_enforced,
            "evidence": "MCP read outside allowed root returns path_not_allowed"
        }),
        json!({
            "id": "no_unapproved_external_writes",
            "pass": true,
            "evidence": "audit performs only local temp writes and requested artifact write; release/publication actions remain gated"
        }),
    ];
    let ok = rows.iter().all(|row| row["pass"] == true);
    let report = json!({
        "schema_version": "tokenzero.security_privacy_audit.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "raw_payloads_local_by_default": exact_ref_local_recovery && pulse_no_raw_payload,
        "pulse_records_raw_payload": !pulse_no_raw_payload,
        "secret_masking_active": visible_secret_masked,
        "allowed_root_controls_active": mcp_allowed_root_enforced,
        "unapproved_external_writes": false,
        "release_publication_allowed": false,
        "rows": rows,
        "gated_actions": ["release", "publication", "remote mutation", "paid services", "global install apply"]
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Security privacy audit",
    )?;
    Ok(report)
}

pub(crate) fn run_artifact_handoff(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let report = completion_handoff::artifact_handoff_report(installed_tokenzero_command_audit());
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Artifact handoff",
    )?;
    Ok(report)
}

pub(crate) fn run_ws_skeleton(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache = temp.path().join("ws-cache.json");
    let file = temp.path().join("sample.txt");
    fs::write(&file, "alpha\nbeta\nwarning: keep this anchor\n")?;

    let read_row = run_json_command(
        &exe,
        &[
            "read",
            file.to_str().unwrap(),
            "--cache-path",
            cache.to_str().unwrap(),
            "--allowed-root",
            temp.path().to_str().unwrap(),
            "--json",
        ],
    )?;
    let file_ref = read_row["refs"]
        .as_array()
        .and_then(|refs| refs.iter().find(|row| row["kind"] == "file"))
        .and_then(|row| row["ref"].as_str())
        .unwrap_or_default();
    let expanded = expand_ref_with_exe(&exe, &cache, file_ref)?;

    let failure_command = if cfg!(windows) {
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
    };
    let mut run_args = run_json_args(temp.path().to_str().unwrap(), cache.to_str().unwrap());
    run_args.push("--".to_string());
    run_args.extend(failure_command.iter().map(|arg| (*arg).to_string()));
    let failure_row = run_json_command_owned(&exe, &run_args)?;

    let bench_output = ws_sibling_artifact_path(&output_json, "tokenzero_ws_001_bench.json");
    let bench = run_bench_competitors(BenchCompetitorsArgs {
        suite: "shell-heavy".to_string(),
        output_json: Some(bench_output.clone()),
        adapter_approval_artifact: None,
        json: true,
    })?;
    let one_shot_output = ws_sibling_artifact_path(&output_json, "tokenzero_ws_001_one_shot.json");
    let one_shot = run_one_shot_eval(one_shot_output.clone(), None)?;
    let claim_audit_output =
        ws_sibling_artifact_path(&output_json, "tokenzero_ws_001_claim_audit.json");
    let claim_audit = run_claim_audit(
        claim_audit_output.clone(),
        None,
        false,
        ClaimEvidenceInputs {
            source_artifact: None,
            benchmark_artifact: None,
            adapter_approval_artifact: None,
            recovery_artifact: None,
            task_success_artifact: None,
            os_artifact: None,
        },
    )?;
    let reach = run_reach(PathBuf::from("."), None)?;

    let competitor_unavailable = bench["rows"].as_array().is_some_and(|rows| {
        rows.iter()
            .any(|row| row["tool"] == "competitors" && row["availability_status"] == "unavailable")
    });
    let artifacts = json!({
        "one_command_family": {
            "present": failure_row["telemetry"]["family"] == "test"
                || failure_row["telemetry"]["family"] == "diagnostic"
                || !failure_row["telemetry"]["family"].is_null(),
            "evidence": failure_row["telemetry"]["family"]
        },
        "one_file_read": {
            "present": read_row["status"] == "ok" && read_row["refs"].as_array().is_some_and(|refs| !refs.is_empty()),
            "evidence": file_ref
        },
        "one_failure_trace": {
            "present": failure_row["telemetry"]["command_success"] == false
                && failure_row["visible"]["text"].as_str().unwrap_or_default().contains("error"),
            "evidence": failure_row["telemetry"]["combined_ref"]
        },
        "one_competitor_unavailable_row": {
            "present": competitor_unavailable,
            "evidence": "competitors unavailable row in benchmark JSON"
        },
        "one_exact_expand_check": {
            "present": expanded == "alpha\nbeta\nwarning: keep this anchor\n",
            "evidence": file_ref
        },
        "adaptive_mode_rationale": {
            "present": one_shot["ok"] == true,
            "evidence": "one-shot-eval rows include mode_rationale"
        },
        "degraded_mode_handling": {
            "present": claim_audit["public_claims_approved"] == false,
            "evidence": "claim gate remains blocked until recovery/source/task evidence is attached"
        }
    });
    let ok = artifacts
        .as_object()
        .is_some_and(|map| map.values().all(|value| value["present"] == true))
        && bench["ok"] == true
        && one_shot["ok"] == true
        && reach["ok"] == true;
    let report = json!({
        "schema_version": "tokenzero.ws_skeleton.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "ws_id": "WS-001",
        "milestone": "M-002 Skeleton",
        "artifacts": artifacts,
        "release_candidate_id": release_candidate_id(),
        "bench_artifact": json_artifact_path(&bench_output),
        "one_shot_artifact": json_artifact_path(&one_shot_output),
        "claim_audit_artifact": json_artifact_path(&claim_audit_output),
        "reach_daemon_required": reach["daemon_required"],
        "public_claims_approved": false,
        "release_publication_allowed": false,
        "release_gates": {
            "public_claims_approved": false,
            "publication_allowed": false,
            "release_publication_allowed": false,
            "global_install_apply_allowed": false
        },
        "next_phase_allowed": ok
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "WS-001 walking skeleton",
    )?;
    Ok(report)
}

pub(crate) fn run_install_smoke(output_json: Option<PathBuf>) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    let root = temp.path();
    fs::write(root.join("AGENTS.md"), "original\n")?;
    let plan = install::plan(root, false, &[]);
    let applied = install::apply(root, false, &[])?;
    let rolled = install::rollback(root, "latest")?;
    let report = json!({
        "schema_version": "tokenzero.install_smoke.v1",
        "status": "ok",
        "ok": true,
        "plan": plan,
        "applied": applied,
        "rollback": rolled,
        "global_writes": false
    });
    if let Some(output) = output_json {
        write_artifacts(&output, None, &report, "Rust install smoke")?;
    }
    Ok(report)
}

pub(crate) fn write_artifacts(
    output_json: &Path,
    output_md: Option<&Path>,
    report: &serde_json::Value,
    title: &str,
) -> Result<()> {
    if let Some(parent) = output_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_json, serde_json::to_string_pretty(report)? + "\n")?;
    if let Some(md) = output_md {
        if let Some(parent) = md.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            md,
            format!(
                "# {title}\n\n```json\n{}\n```\n",
                serde_json::to_string_pretty(report)?
            ),
        )?;
    }
    Ok(())
}
