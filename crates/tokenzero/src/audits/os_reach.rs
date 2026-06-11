use crate::*;

pub(crate) fn run_shell_matrix(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let exe = std::env::current_exe()?;
    let temp = tempdir()?;
    let cache_arg = temp.path().join("matrix-cache.json");
    let mut rows = Vec::new();
    let mut direct = Command::new(&exe);
    direct
        .arg("run")
        .arg("--cache-path")
        .arg(&cache_arg)
        .arg("--")
        .arg("echo")
        .arg("ok");
    rows.push(run_matrix_row("direct", &mut direct));
    if cfg!(unix) {
        let mut env_cmd = Command::new("env");
        env_cmd
            .arg("-i")
            .arg(&exe)
            .arg("run")
            .arg("--cache-path")
            .arg(&cache_arg)
            .arg("--")
            .arg("echo")
            .arg("ok");
        rows.push(run_matrix_row("env-i", &mut env_cmd));
        for shell in ["/bin/sh", "/bin/bash", "/bin/zsh"] {
            if Path::new(shell).exists() {
                let mut cmd = Command::new(shell);
                cmd.arg("-c").arg(format!(
                    "{} run --cache-path {} -- echo ok",
                    exe.display(),
                    cache_arg.display()
                ));
                rows.push(run_matrix_row(&format!("{shell} -c"), &mut cmd));
            }
        }
        for shell in ["/bin/sh", "/bin/bash", "/bin/zsh"] {
            if Path::new(shell).exists() {
                let mut cmd = Command::new("env");
                cmd.arg("-i").arg(shell).arg("-c").arg(format!(
                    "{} run --cache-path {} -- echo ok",
                    exe.display(),
                    cache_arg.display()
                ));
                rows.push(run_matrix_row(&format!("env-i {shell} -c"), &mut cmd));
            }
        }
    }
    if cfg!(windows) {
        let args = vec![
            exe.display().to_string(),
            "run".to_string(),
            "--cache-path".to_string(),
            cache_arg.display().to_string(),
            "--".to_string(),
            "echo".to_string(),
            "ok".to_string(),
        ];
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(quote_for("cmd", &args));
        rows.push(run_matrix_row("cmd /C", &mut cmd));

        let mut powershell = Command::new("powershell");
        powershell
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!("& {}", quote_for("powershell", &args)));
        rows.push(run_matrix_row("powershell -NoProfile", &mut powershell));
    }
    let ok = rows.iter().all(|r| r["ok"] == true);
    let report = json!({
        "schema_version": "tokenzero.shell_matrix.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "rows": rows,
        "windows": if cfg!(windows) { "run" } else { "not_run_on_this_host" },
        "linux": if cfg!(target_os = "linux") { "run" } else { "not_run_on_this_host" },
        "macos": if cfg!(target_os = "macos") { "run" } else { "not_run_on_this_host" }
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "Rust shell matrix",
    )?;
    Ok(report)
}

pub(crate) fn run_os_reach_audit(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    root: PathBuf,
    os_artifacts: Vec<PathBuf>,
    release_approval: bool,
) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    let shell_matrix = run_shell_matrix(temp.path().join("shell-matrix.json"), None)?;
    let install_smoke = run_install_smoke(None)?;
    let reach = run_reach(root, None)?;
    let core_surfaces = run_core_surface_audit(&shell_matrix, &install_smoke)?;
    let external_artifacts = load_os_release_artifacts(&os_artifacts)?;
    let release_oses = ["windows", "linux", "macos"];
    let current_os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        std::env::consts::OS
    };
    let release_candidate_id = release_candidate_id();
    let os_rows = release_oses
        .iter()
        .map(|os| {
            let shell_status = shell_matrix[*os].as_str().unwrap_or("not_run_on_this_host");
            if *os == current_os {
                json!({
                    "os": os,
                    "current_host": true,
                    "artifact_source": "local",
                    "shell_matrix": shell_status,
                    "install_smoke": if install_smoke["ok"] == true { "run" } else { "not_run_on_this_host" },
                    "daemon_required": false,
                    "global_writes": false,
                    "release_candidate_id": release_candidate_id,
                    "claim_ready": shell_status == "run" && install_smoke["ok"] == true,
                    "evidence": "local release-path artifact"
                })
            } else if let Some(artifact) = external_artifacts.iter().find(|artifact| {
                artifact["os"].as_str().unwrap_or_default() == *os
                    && artifact["schema_version"] == "tokenzero.os_release_artifact.v1"
            }) {
                let shell_run = artifact["shell_matrix"] == "run";
                let install_run = artifact["install_smoke"] == "run";
                let daemon_required = artifact["daemon_required"].as_bool().unwrap_or(true);
                let global_writes = artifact["global_writes"].as_bool().unwrap_or(true);
                json!({
                    "os": os,
                    "current_host": false,
                    "artifact_source": "external",
                    "artifact_path": artifact["artifact_path"],
                    "shell_matrix": artifact["shell_matrix"],
                    "install_smoke": artifact["install_smoke"],
                    "daemon_required": daemon_required,
                    "global_writes": global_writes,
                    "release_candidate_id": artifact["release_candidate_id"],
                    "claim_ready": shell_run && install_run && !daemon_required && !global_writes,
                    "evidence": artifact["evidence"]
                })
            } else {
                json!({
                    "os": os,
                    "current_host": false,
                    "artifact_source": "missing",
                    "shell_matrix": "not_run_on_this_host",
                    "install_smoke": "not_run_on_this_host",
                    "daemon_required": false,
                    "global_writes": false,
                    "release_candidate_id": serde_json::Value::Null,
                    "claim_ready": false,
                    "evidence": "no artifact from this host"
                })
            }
        })
        .collect::<Vec<_>>();
    let all_release_oses_run = os_rows.iter().all(|row| row["claim_ready"] == true);
    let release_candidate_ids = os_rows
        .iter()
        .filter(|row| row["claim_ready"] == true)
        .filter_map(|row| row["release_candidate_id"].as_str())
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    let unique_release_candidate_ids =
        release_candidate_ids
            .iter()
            .fold(Vec::<String>::new(), |mut ids, id| {
                if !ids.iter().any(|existing| existing == id) {
                    ids.push(id.clone());
                }
                ids
            });
    let same_release_candidate = all_release_oses_run
        && release_candidate_ids.len() == release_oses.len()
        && unique_release_candidate_ids.len() == 1;
    let mut blocked_reasons = Vec::new();
    for row in &os_rows {
        if row["claim_ready"] != true {
            blocked_reasons.push(format!(
                "{} not run with shell and install artifacts",
                row["os"].as_str().unwrap_or("unknown")
            ));
        }
    }
    if all_release_oses_run && !same_release_candidate {
        blocked_reasons
            .push("OS release artifacts are not from the same release candidate".to_string());
    }
    if blocked_reasons.is_empty() && !release_approval {
        blocked_reasons.push("explicit release approval not granted".to_string());
    }
    let public_os_claim_approved =
        all_release_oses_run && same_release_candidate && release_approval;
    let ok = shell_matrix["ok"] == true
        && install_smoke["ok"] == true
        && install_smoke["global_writes"] == false
        && reach["daemon_required"] == false
        && reach["global_writes"] == false;
    let report = json!({
        "schema_version": "tokenzero.os_reach_audit.v1",
        "status": if ok { "ok" } else { "blocked" },
        "ok": ok,
        "release_candidate_id": release_candidate_id,
        "current_os": current_os,
        "daemon_required": false,
        "global_writes": false,
        "release_approval": release_approval,
        "public_os_claim_approved": public_os_claim_approved,
        "all_release_oses_run": all_release_oses_run,
        "same_release_candidate": same_release_candidate,
        "release_candidate_ids": unique_release_candidate_ids,
        "blocked_reasons": blocked_reasons,
        "external_artifact_count": external_artifacts.len(),
        "os_rows": os_rows,
        "shell_matrix": shell_matrix,
        "install_smoke": install_smoke,
        "core_surfaces": core_surfaces,
        "reach": reach
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "OS reach audit",
    )?;
    Ok(report)
}

pub(crate) fn run_os_release_artifact(
    output_json: PathBuf,
    output_md: Option<PathBuf>,
    root: PathBuf,
) -> Result<serde_json::Value> {
    let temp = tempdir()?;
    let shell_matrix = run_shell_matrix(temp.path().join("shell-matrix.json"), None)?;
    let install_smoke = run_install_smoke(None)?;
    let reach = run_reach(root, None)?;
    let core_surfaces = run_core_surface_audit(&shell_matrix, &install_smoke)?;
    let current_os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        std::env::consts::OS
    };
    let shell_run = shell_matrix[current_os].as_str().unwrap_or("not_run") == "run";
    let install_run = install_smoke["ok"] == true && install_smoke["global_writes"] == false;
    let daemon_required = reach["daemon_required"] != false
        || core_surfaces
            .iter()
            .any(|row| row["daemon_required"] != false);
    let global_writes = reach["global_writes"] != false
        || install_smoke["global_writes"] != false
        || core_surfaces
            .iter()
            .any(|row| row["global_writes"] != false);
    let core_surfaces_ok = core_surfaces.iter().all(|row| row["ok"] == true);
    let claim_ready =
        shell_run && install_run && core_surfaces_ok && !daemon_required && !global_writes;
    let report = json!({
        "schema_version": "tokenzero.os_release_artifact.v1",
        "status": if claim_ready { "ok" } else { "blocked" },
        "ok": claim_ready,
        "release_candidate_id": release_candidate_id(),
        "os": current_os,
        "shell_matrix": if shell_run { "run" } else { "not_run" },
        "install_smoke": if install_run { "run" } else { "not_run" },
        "daemon_required": daemon_required,
        "global_writes": global_writes,
        "claim_ready": claim_ready,
        "release_publication_allowed": false,
        "evidence": "local os-release-artifact command; not a public OS-agnostic claim",
        "shell_matrix_artifact": shell_matrix,
        "install_smoke_artifact": install_smoke,
        "core_surfaces": core_surfaces,
        "reach": reach,
    });
    write_artifacts(
        &output_json,
        output_md.as_deref(),
        &report,
        "OS release artifact",
    )?;
    Ok(report)
}

pub(crate) fn run_core_surface_audit(
    shell_matrix: &serde_json::Value,
    install_smoke: &serde_json::Value,
) -> Result<Vec<serde_json::Value>> {
    let temp = tempdir()?;
    let root = temp.path().to_path_buf();
    let doctor = doctor_report(&DoctorArgs {
        root: Some(root.clone()),
        cache_path: Some(root.join("recovery-cache.json")),
        runtime: true,
        json: true,
        robot_triage: false,
        fix: false,
        dry_run: false,
        explain: None,
        command: None,
    });
    let cache_engine = TokenZeroEngine::new(EngineConfig {
        allowed_roots: default_allowed_roots(&root),
        cache_path: root.join("recovery-cache.json"),
        max_visible_tokens: 4000,
        mode: Mode::Structured,
        shell_timeout: default_shell_timeout(),
        mcp_idle_timeout: None,
        ..EngineConfig::for_root(&root)
    });
    let cache_pack = cache_engine.cache_pack("agent");
    let mcp = run_mcp_artifact(root.join("mcp-smoke.json"), None, 1)?;
    Ok(vec![
        core_surface_row(
            "install",
            install_smoke["ok"] == true && install_smoke["global_writes"] == false,
            "install-smoke disposable local root",
            json!({
                "schema_version": install_smoke["schema_version"],
                "global_writes": install_smoke["global_writes"]
            }),
        ),
        core_surface_row(
            "doctor",
            doctor["ok"] == true,
            "doctor --runtime on disposable local root",
            json!({
                "schema_version": doctor["schema_version"],
                "root": doctor["root"]
            }),
        ),
        core_surface_row(
            "shell",
            shell_matrix["ok"] == true,
            "shell-matrix current host",
            json!({
                "schema_version": shell_matrix["schema_version"],
                "windows": shell_matrix["windows"],
                "linux": shell_matrix["linux"],
                "macos": shell_matrix["macos"]
            }),
        ),
        core_surface_row(
            "mcp",
            mcp["ok"] == true && mcp["unexpected_exits"] == 0,
            "mcp-smoke local stdio process",
            json!({
                "schema_version": mcp["schema_version"],
                "unexpected_exits": mcp["unexpected_exits"]
            }),
        ),
        core_surface_row(
            "cache_pack",
            cache_pack.status == "ok" || cache_pack.status == "degraded",
            "cache-pack local recovery cache",
            json!({
                "tool": cache_pack.tool,
                "status": cache_pack.status,
                "refs": cache_pack.refs.len()
            }),
        ),
    ])
}

pub(crate) fn core_surface_row(
    surface: &str,
    ok: bool,
    evidence: &str,
    details: serde_json::Value,
) -> serde_json::Value {
    json!({
        "surface": surface,
        "ok": ok,
        "daemon_required": false,
        "global_writes": false,
        "evidence": evidence,
        "details": details
    })
}

pub(crate) fn load_os_release_artifacts(paths: &[PathBuf]) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = Vec::new();
    for path in paths {
        let mut artifact: serde_json::Value = serde_json::from_slice(
            &fs::read(path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        artifact["artifact_path"] = json!(path.display().to_string());
        artifacts.push(artifact);
    }
    Ok(artifacts)
}
