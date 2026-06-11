use crate::*;

pub fn doctor(root: &Path, cache_path: Option<&Path>) -> serde_json::Value {
    let cache = doctor_cache_path(root, cache_path);
    let root_exists = root.exists();
    let root_is_dir = root.is_dir();
    let cache_parent = doctor_cache_parent(root, cache_path);
    let cache_parent_exists = cache_parent.as_ref().is_some_and(|p| p.exists());
    let cache_parent_fixable = doctor_cache_parent_fixable(root, cache_parent.as_deref());
    let mut findings = Vec::new();
    if !root_exists {
        findings.push(serde_json::json!({
            "id": "tz-root-missing",
            "severity": "error",
            "status": "detected",
            "check": "root_exists",
            "summary": "doctor root does not exist",
            "evidence": {
                "path": root.display().to_string(),
                "exists": false
            },
            "auto_fix": false,
            "fix_supported": false,
            "next_step": "Pass --root pointing at an existing project directory."
        }));
    } else if !root_is_dir {
        findings.push(serde_json::json!({
            "id": "tz-root-not-directory",
            "severity": "error",
            "status": "detected",
            "check": "root_is_directory",
            "summary": "doctor root is not a directory",
            "evidence": {
                "path": root.display().to_string(),
                "is_directory": false
            },
            "auto_fix": false,
            "fix_supported": false,
            "next_step": "Pass --root pointing at a directory."
        }));
    }
    if !cache_parent_exists {
        findings.push(serde_json::json!({
            "id": "tz-cache-parent-missing",
            "severity": "info",
            "status": "detected",
            "check": "cache_parent_exists",
            "summary": "recovery cache parent does not exist yet",
            "evidence": {
                "path": cache_parent
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| String::from("")),
                "exists": false
            },
            "auto_fix": cache_parent_fixable,
            "fix_supported": cache_parent_fixable,
            "recommended_argv": if cache_parent_fixable {
                serde_json::json!(["tokenzero", "doctor", "--dry-run", "--fix", "--json"])
            } else {
                cache_parent
                    .as_ref()
                    .map(|p| serde_json::json!(["mkdir", "-p", p.display().to_string()]))
                    .unwrap_or_else(|| serde_json::json!([]))
            },
            "next_step": "Create the cache parent first if this preflight must be clean before the first cache-writing command."
        }));
    }
    let has_blocking_finding = findings.iter().any(|finding| {
        finding
            .get("severity")
            .and_then(Value::as_str)
            .is_some_and(|severity| severity == "error")
    });
    let checks = vec![
        serde_json::json!({
            "id": "root_exists",
            "ok": root_exists,
            "severity": if root_exists { "ok" } else { "error" },
            "evidence": root.display().to_string()
        }),
        serde_json::json!({
            "id": "root_is_directory",
            "ok": root_is_dir,
            "severity": if root_is_dir { "ok" } else { "error" },
            "evidence": root.display().to_string()
        }),
        serde_json::json!({
            "id": "cache_parent_exists",
            "ok": cache_parent_exists,
            "severity": if cache_parent_exists { "ok" } else { "info" },
            "evidence": cache_parent
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| String::from(""))
        }),
        serde_json::json!({
            "id": "core_runtime_rust",
            "ok": true,
            "severity": "ok",
            "evidence": "compiled Rust binary"
        }),
        serde_json::json!({
            "id": "mcp_server_entrypoint_declared",
            "ok": true,
            "severity": "ok",
            "evidence": "tokenzero mcp-server"
        }),
    ];
    let next_steps = if has_blocking_finding {
        vec![serde_json::json!({
            "priority": 1,
            "action": "fix_blocking_findings",
            "command": "tokenzero doctor --json --root <existing-directory>",
            "reason": "doctor refuses to claim health while the root is invalid"
        })]
    } else if findings.is_empty() {
        vec![serde_json::json!({
            "priority": 1,
            "action": "no_action_required",
            "command": "tokenzero doctor --runtime --json",
            "reason": "run this only when a runtime plan probe is needed"
        })]
    } else {
        vec![serde_json::json!({
            "priority": 1,
            "action": "review_informational_findings",
            "command": "tokenzero doctor --json",
            "reason": "only non-blocking preflight findings were detected"
        })]
    };
    let exit_code = if has_blocking_finding { 1 } else { 0 };
    let blocking_findings = findings
        .iter()
        .filter(|finding| {
            finding
                .get("severity")
                .and_then(Value::as_str)
                .is_some_and(|severity| severity == "error")
        })
        .count();
    let informational_findings = findings.len().saturating_sub(blocking_findings);
    serde_json::json!({
        "schema_version": "tokenzero.doctor.v1",
        "tool": "tokenzero",
        "doctor_version": env!("CARGO_PKG_VERSION"),
        "doctor_contract_version": DOCTOR_CONTRACT_VERSION,
        "status": if has_blocking_finding { "blocked" } else { "ok" },
        "ok": !has_blocking_finding,
        "exit_code": exit_code,
        "root": root.display().to_string(),
        "mode": "diagnose",
        "mutates": false,
        "summary": {
            "total_findings": findings.len(),
            "blocking_findings": blocking_findings,
            "informational_findings": informational_findings,
            "auto_fixable": findings
                .iter()
                .filter(|finding| finding["auto_fix"].as_bool().unwrap_or(false))
                .count(),
            "online_required": 0
        },
        "finding_count": findings.len(),
        "findings": findings,
        "checks": checks,
        "next_steps": next_steps,
        "capabilities": doctor_capabilities(),
        "exit_codes": doctor_exit_codes(),
        "robot_docs": {
            "recommended_invocation": "tokenzero doctor --json",
            "health_invocation": "tokenzero doctor health",
            "capabilities_invocation": "tokenzero doctor capabilities --json",
            "robot_docs_invocation": "tokenzero doctor robot-docs",
            "explain_invocation": "tokenzero doctor explain <finding-id>",
            "runtime_probe_invocation": "tokenzero doctor --runtime --json",
            "stdout": "stable JSON only",
            "stderr": "empty unless process-level errors occur",
            "mutation_policy": "read-only by default; --fix only repairs tz-cache-parent-missing with backups and actions.jsonl"
        },
        "doctor_contract": {
            "default_read_only": true,
            "detect_then_fix": "diagnose is pure; fix mode re-runs detectors before mutating",
            "mutation_chokepoint": "doctor_mutate_create_dir",
            "backup_before_mutation": "missing-state marker is recorded before the create-dir mutation"
        },
        "core_runtime": {
            "language": "rust",
            "external_runtime_required": false,
            "daemon_required": false
        },
        "cache": {
            "path": cache.display().to_string(),
            "parent": cache_parent
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| String::from("")),
            "parent_exists": cache_parent_exists
        },
        "mcp": {
            "ready": true,
            "server": "tokenzero mcp-server"
        },
        "runtime": {
            "required_for_core": true
        }
    })
}

pub fn doctor_fix(root: &Path, cache_path: Option<&Path>, dry_run: bool) -> serde_json::Value {
    let cache = doctor_cache_path(root, cache_path);
    let cache_parent = doctor_cache_parent(root, cache_path);
    let before = doctor(root, cache_path);
    if before["ok"] == false {
        return serde_json::json!({
            "schema_version": "tokenzero.doctor.fix.v1",
            "status": "refused",
            "ok": false,
            "exit_code": 4,
            "mode": "fix",
            "dry_run": dry_run,
            "mutates": false,
            "actions_taken": 0,
            "findings": before["findings"],
            "refusal": {
                "reason": "blocking findings must be resolved before fix mode mutates",
                "next_command": "tokenzero doctor --json --root <existing-directory>"
            }
        });
    }
    let Some(cache_parent) = cache_parent else {
        return doctor_fix_refused(dry_run, "cache path has no parent", &before);
    };
    if cache_parent.exists() {
        return serde_json::json!({
            "schema_version": "tokenzero.doctor.fix.v1",
            "status": "ok",
            "ok": true,
            "exit_code": 0,
            "mode": "fix",
            "dry_run": dry_run,
            "mutates": false,
            "actions_taken": 0,
            "actions_planned": [],
            "summary": "cache parent already exists; no action taken",
            "findings_before": before["findings"],
            "findings_after": doctor(root, cache_path)["findings"]
        });
    }
    if !doctor_cache_parent_fixable(root, Some(&cache_parent)) {
        return doctor_fix_refused(
            dry_run,
            "cache parent is outside the root, root is invalid, or the immediate parent is missing",
            &before,
        );
    }

    let rel = doctor_rel_path(root, &cache_parent);
    let planned_action = serde_json::json!({
        "fixer_id": DOCTOR_FIXER_CACHE_PARENT,
        "finding_id": DOCTOR_FIXER_CACHE_PARENT,
        "op": "create_dir",
        "path": rel,
        "absolute_path": cache_parent.display().to_string(),
        "writes_to": [
            cache_parent.display().to_string(),
            root.join(".doctor").display().to_string()
        ],
        "undo": "doctor undo <run-id>",
        "estimated_actions": 1
    });
    if dry_run {
        return serde_json::json!({
            "schema_version": "tokenzero.doctor.fix.v1",
            "status": "ok",
            "ok": true,
            "exit_code": 0,
            "mode": "fix",
            "dry_run": true,
            "mutates": false,
            "actions_taken": 0,
            "actions_planned": [planned_action],
            "findings_before": before["findings"],
            "summary": "dry-run only; no filesystem changes were made"
        });
    }

    let lock = match DoctorLock::acquire(root) {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.fix.v1",
                "status": "concurrency_lost",
                "ok": false,
                "exit_code": 5,
                "mode": "fix",
                "dry_run": false,
                "mutates": false,
                "actions_taken": 0,
                "actions_planned": [planned_action],
                "refusal": {
                    "reason": "doctor mutation lock is held",
                    "error": err.to_string()
                }
            });
        }
        Err(err) => return doctor_fix_io_error(false, "acquire doctor lock", err),
    };

    let run_id = doctor_run_id(root, &cache);
    let run_dir = root.join(".doctor/runs").join(&run_id);
    if let Err(err) = fs::create_dir_all(run_dir.join("backups")) {
        drop(lock);
        return doctor_fix_io_error(false, "create run artifact directory", err);
    }

    let action = match doctor_mutate_create_dir(root, &run_dir, &run_id, &cache_parent) {
        Ok(action) => action,
        Err(err) => {
            drop(lock);
            return doctor_fix_io_error(false, "create cache parent", err);
        }
    };
    let after = doctor(root, cache_path);
    let actions = vec![serde_json::to_value(&action).unwrap_or_else(|_| serde_json::json!({}))];
    let mut report = serde_json::json!({
        "schema_version": "tokenzero.doctor.fix.v1",
        "status": "ok",
        "ok": true,
        "exit_code": 0,
        "mode": "fix",
        "dry_run": false,
        "mutates": true,
        "run_id": run_id,
        "run_dir": run_dir.display().to_string(),
        "actions_taken": 1,
        "actions": actions,
        "actions_planned": [planned_action],
        "findings_before": before["findings"],
        "findings_after": after["findings"],
        "undo_command": format!("tokenzero doctor undo {run_id} --json")
    });
    let mut artifact_errors = Vec::new();
    match serde_json::to_vec_pretty(&report) {
        Ok(bytes) => {
            for (label, path, content) in [
                ("report", run_dir.join("report.json"), bytes.as_slice()),
                ("stdout", run_dir.join("stdout.json"), bytes.as_slice()),
                ("latest", root.join(".doctor/latest"), run_id.as_bytes()),
            ] {
                if let Err(err) = atomic_write(&path, content) {
                    artifact_errors.push(serde_json::json!({
                        "artifact": label,
                        "path": path.display().to_string(),
                        "error": err.to_string()
                    }));
                }
            }
        }
        Err(err) => artifact_errors.push(serde_json::json!({
            "artifact": "report",
            "error": format!("serialize report: {err}")
        })),
    }
    if !artifact_errors.is_empty() {
        report["status"] = serde_json::json!("partial");
        report["ok"] = serde_json::json!(false);
        report["exit_code"] = serde_json::json!(2);
        report["artifact_errors"] = serde_json::Value::Array(artifact_errors);
        if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
            let _ = atomic_write(&run_dir.join("report.json"), &bytes);
            let _ = atomic_write(&run_dir.join("stdout.json"), &bytes);
        }
    }
    drop(lock);
    report
}

pub fn doctor_undo(root: &Path, run_id: &str) -> serde_json::Value {
    let resolved_run_id = if run_id == "latest" {
        match fs::read_to_string(root.join(".doctor/latest")) {
            Ok(id) => id.trim().to_string(),
            Err(err) => {
                return serde_json::json!({
                    "schema_version": "tokenzero.doctor.undo.v1",
                    "status": "failed",
                    "ok": false,
                    "exit_code": 3,
                    "run_id": run_id,
                    "error": format!("could not resolve latest run: {err}")
                });
            }
        }
    } else {
        run_id.to_string()
    };
    let lock = match DoctorLock::acquire(root) {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.undo.v1",
                "status": "concurrency_lost",
                "ok": false,
                "exit_code": 5,
                "run_id": resolved_run_id,
                "error": err.to_string()
            });
        }
        Err(err) => {
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.undo.v1",
                "status": "failed",
                "ok": false,
                "exit_code": 3,
                "run_id": resolved_run_id,
                "error": format!("could not acquire doctor lock: {err}")
            });
        }
    };
    let run_dir = root.join(".doctor/runs").join(&resolved_run_id);
    let actions_path = run_dir.join("actions.jsonl");
    let content = match fs::read_to_string(&actions_path) {
        Ok(content) => content,
        Err(err) => {
            drop(lock);
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.undo.v1",
                "status": "failed",
                "ok": false,
                "exit_code": 3,
                "run_id": resolved_run_id,
                "actions_path": actions_path.display().to_string(),
                "error": format!("could not read actions.jsonl: {err}")
            });
        }
    };
    let mut actions = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<DoctorActionRecord>(line) {
            Ok(action) => actions.push(action),
            Err(err) => {
                drop(lock);
                return serde_json::json!({
                    "schema_version": "tokenzero.doctor.undo.v1",
                    "status": "failed",
                    "ok": false,
                    "exit_code": 3,
                    "run_id": resolved_run_id,
                    "error": format!("could not parse actions.jsonl: {err}")
                });
            }
        }
    }
    let mut restored = Vec::new();
    for action in actions.iter().rev() {
        if action.fixer_id != DOCTOR_FIXER_CACHE_PARENT || action.op != "create_dir" {
            drop(lock);
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.undo.v1",
                "status": "failed",
                "ok": false,
                "exit_code": 3,
                "run_id": resolved_run_id,
                "error": format!("unsupported undo action {} for {}", action.op, action.path)
            });
        }
        let target = root.join(&action.path);
        if !target.exists() {
            restored.push(serde_json::json!({
                "path": action.path,
                "status": "already_absent"
            }));
            continue;
        }
        if !target.is_dir() {
            drop(lock);
            return doctor_undo_failed(
                &resolved_run_id,
                &action.path,
                "created path is not a directory",
            );
        }
        match fs::read_dir(&target) {
            Ok(mut entries) => {
                if entries.next().is_some() {
                    drop(lock);
                    return doctor_undo_failed(
                        &resolved_run_id,
                        &action.path,
                        "created directory is no longer empty; refusing to move later user data",
                    );
                }
            }
            Err(err) => {
                drop(lock);
                return doctor_undo_failed(
                    &resolved_run_id,
                    &action.path,
                    &format!("could not inspect directory: {err}"),
                );
            }
        }
        let quarantine = run_dir.join("quarantine").join(&action.path);
        if let Some(parent) = quarantine.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                drop(lock);
                return doctor_undo_failed(
                    &resolved_run_id,
                    &action.path,
                    &format!("could not create quarantine parent: {err}"),
                );
            }
        }
        if let Err(err) = fs::rename(&target, &quarantine) {
            drop(lock);
            return doctor_undo_failed(
                &resolved_run_id,
                &action.path,
                &format!("could not quarantine created directory: {err}"),
            );
        }
        restored.push(serde_json::json!({
            "path": action.path,
            "status": "restored_absent",
            "quarantine_path": quarantine.display().to_string()
        }));
    }
    let report = serde_json::json!({
        "schema_version": "tokenzero.doctor.undo.v1",
        "status": "ok",
        "ok": true,
        "exit_code": 0,
        "run_id": resolved_run_id,
        "mutates": true,
        "restored": restored
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        let _ = atomic_write(&run_dir.join("undo.json"), &bytes);
    }
    drop(lock);
    report
}

pub fn doctor_ls(root: &Path) -> serde_json::Value {
    let latest = fs::read_to_string(root.join(".doctor/latest"))
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty());
    let runs_dir = root.join(".doctor/runs");
    let entries = match fs::read_dir(&runs_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => {
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.ls.v1",
                "status": "ok",
                "ok": true,
                "exit_code": 0,
                "root": root.display().to_string(),
                "runs_dir": runs_dir.display().to_string(),
                "run_count": 0,
                "runs": []
            });
        }
        Err(err) => {
            return serde_json::json!({
                "schema_version": "tokenzero.doctor.ls.v1",
                "status": "failed",
                "ok": false,
                "exit_code": 74,
                "root": root.display().to_string(),
                "runs_dir": runs_dir.display().to_string(),
                "error": format!("could not list doctor runs: {err}")
            });
        }
    };
    let mut runs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if !file_type.is_dir() {
            continue;
        }
        let run_id = entry.file_name().to_string_lossy().to_string();
        let run_dir = entry.path();
        let report = fs::read_to_string(run_dir.join("report.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        let action_count = fs::read_to_string(run_dir.join("actions.jsonl"))
            .ok()
            .map(|text| text.lines().filter(|line| !line.trim().is_empty()).count())
            .unwrap_or(0);
        let started_at_unix = run_id
            .split_once("__")
            .and_then(|(prefix, _)| prefix.parse::<u64>().ok());
        runs.push(serde_json::json!({
            "run_id": run_id,
            "started_at_unix": started_at_unix,
            "path": run_dir.display().to_string(),
            "latest": latest.as_deref() == Some(run_id.as_str()),
            "status": report
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            "exit_code": report
                .as_ref()
                .and_then(|value| value.get("exit_code"))
                .and_then(Value::as_i64),
            "action_count": action_count,
            "has_report": report.is_some(),
            "has_undo": run_dir.join("undo.json").exists(),
            "undo_command": format!("tokenzero doctor undo {run_id} --json")
        }));
    }
    runs.sort_by(|left, right| {
        right["run_id"]
            .as_str()
            .unwrap_or("")
            .cmp(left["run_id"].as_str().unwrap_or(""))
    });
    serde_json::json!({
        "schema_version": "tokenzero.doctor.ls.v1",
        "status": "ok",
        "ok": true,
        "exit_code": 0,
        "root": root.display().to_string(),
        "runs_dir": runs_dir.display().to_string(),
        "run_count": runs.len(),
        "runs": runs
    })
}

pub(crate) fn doctor_fix_refused(
    dry_run: bool,
    reason: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.fix.v1",
        "status": "refused",
        "ok": false,
        "exit_code": 4,
        "mode": "fix",
        "dry_run": dry_run,
        "mutates": false,
        "actions_taken": 0,
        "findings": report["findings"],
        "refusal": {
            "reason": reason,
            "safe_alternative": "tokenzero doctor explain tz-cache-parent-missing --json"
        }
    })
}

pub(crate) fn doctor_fix_io_error(
    dry_run: bool,
    phase: &str,
    err: std::io::Error,
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.fix.v1",
        "status": "failed",
        "ok": false,
        "exit_code": 3,
        "mode": "fix",
        "dry_run": dry_run,
        "mutates": false,
        "actions_taken": 0,
        "error": {
            "phase": phase,
            "message": err.to_string()
        }
    })
}

pub(crate) fn doctor_undo_failed(run_id: &str, path: &str, reason: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.undo.v1",
        "status": "failed",
        "ok": false,
        "exit_code": 3,
        "run_id": run_id,
        "path": path,
        "reason": reason
    })
}

pub(crate) fn doctor_mutate_create_dir(
    root: &Path,
    run_dir: &Path,
    run_id: &str,
    path: &Path,
) -> std::io::Result<DoctorActionRecord> {
    if path.exists() {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            format!("{} already exists", path.display()),
        ));
    }
    if !doctor_cache_parent_fixable(root, Some(path)) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("{} is outside safe doctor write scopes", path.display()),
        ));
    }
    let rel = doctor_rel_path(root, path);
    let before_hash = sha256("missing");
    let backup_marker = run_dir
        .join("backups")
        .join(format!("{}.missing.json", rel.replace('/', "__")));
    let backup = serde_json::json!({
        "schema_version": "tokenzero.doctor.backup_marker.v1",
        "path": rel,
        "existed": false,
        "before_hash": before_hash.clone(),
        "fixer_id": DOCTOR_FIXER_CACHE_PARENT
    });
    let backup_bytes = serde_json::to_vec_pretty(&backup)
        .map_err(|err| Error::other(format!("serialize backup marker: {err}")))?;
    atomic_write(&backup_marker, &backup_bytes)?;
    fs::create_dir(path)?;
    let after_hash = if path.exists() && path.is_dir() {
        sha256("dir:empty")
    } else {
        sha256("missing")
    };
    let quarantine = run_dir.join("quarantine").join(&rel);
    let action = DoctorActionRecord {
        schema_version: "tokenzero.doctor.action.v1".to_string(),
        path: rel,
        op: "create_dir".to_string(),
        before_hash,
        after_hash,
        before_exists: false,
        after_exists: true,
        run_id: run_id.to_string(),
        fixer_id: DOCTOR_FIXER_CACHE_PARENT.to_string(),
        ok: true,
        backup_path: Some(backup_marker.display().to_string()),
        quarantine_path: Some(quarantine.display().to_string()),
    };
    let actions_path = run_dir.join("actions.jsonl");
    let write_action = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(actions_path)?;
        serde_json::to_writer(&mut file, &action)
            .map_err(|err| Error::other(format!("serialize action: {err}")))?;
        file.write_all(b"\n")?;
        file.sync_all()
    })();
    if let Err(err) = write_action {
        if let Some(parent) = quarantine.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(path, &quarantine);
        return Err(err);
    }
    Ok(action)
}

pub(crate) fn fixable_cache_parent_finding(
    report: &serde_json::Value,
) -> Option<&serde_json::Value> {
    report["findings"].as_array()?.iter().find(|finding| {
        finding["id"] == DOCTOR_FIXER_CACHE_PARENT
            && finding["fix_supported"].as_bool().unwrap_or(false)
    })
}

pub fn doctor_capabilities() -> serde_json::Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.capabilities.v1",
        "tool": "tokenzero",
        "tool_version": env!("CARGO_PKG_VERSION"),
        "doctor_version": env!("CARGO_PKG_VERSION"),
        "doctor_contract_version": DOCTOR_CONTRACT_VERSION,
        "platform": {
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH
        },
        "subsystems": ["workspace_root", "cache", "runtime", "mcp"],
        "commands": [
            {
                "name": "doctor --json",
                "description": "read-only install and cache health report",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor diagnose --json",
                "description": "explicit spelling for the default read-only diagnose mode",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor health",
                "description": "cheap liveness summary for schedulers and agents",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor capabilities --json",
                "description": "machine-readable doctor contract",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor robot-docs",
                "description": "paste-ready agent handbook",
                "mutates": false,
                "json": false
            },
            {
                "name": "doctor explain <finding-id>",
                "description": "expand a current or known doctor finding",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor --robot-triage --json",
                "description": "single-call triage summary for agents",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor --dry-run --fix --json",
                "description": "plan the cache-parent repair without writing",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor --fix --json",
                "description": "repair the missing cache parent through the doctor mutate chokepoint",
                "mutates": true,
                "json": true
            },
            {
                "name": "doctor fix --json",
                "description": "explicit spelling for doctor --fix --json",
                "mutates": true,
                "json": true
            },
            {
                "name": "doctor undo <run-id> --json",
                "description": "restore the cache-parent create-dir fixer by quarantining the created empty directory",
                "mutates": true,
                "json": true
            },
            {
                "name": "doctor ls --json",
                "description": "list local doctor run artifacts with run ids, exit codes, and action counts",
                "mutates": false,
                "json": true
            },
            {
                "name": "doctor --runtime --json",
                "description": "read-only report plus local runtime command plan probe",
                "mutates": false,
                "json": true
            }
        ],
        "detectors": [
            {
                "id": "tz-root-missing",
                "check": "root_exists",
                "subsystem": "workspace_root",
                "severity": "error",
                "description": "doctor root path does not exist",
                "estimated_cost_ms": 1,
                "online_required": false,
                "auto_detected": true,
                "auto_fixed": false
            },
            {
                "id": "tz-root-not-directory",
                "check": "root_is_directory",
                "subsystem": "workspace_root",
                "severity": "error",
                "description": "doctor root path exists but is not a directory",
                "estimated_cost_ms": 1,
                "online_required": false,
                "auto_detected": true,
                "auto_fixed": false
            },
            {
                "id": "tz-cache-parent-missing",
                "check": "cache_parent_exists",
                "subsystem": "cache",
                "severity": "info",
                "description": "recovery cache parent directory does not exist yet",
                "estimated_cost_ms": 1,
                "online_required": false,
                "auto_detected": true,
                "auto_fixed": true
            },
            {
                "id": "tz-core-runtime-rust",
                "check": "core_runtime_rust",
                "subsystem": "runtime",
                "severity": "ok",
                "description": "core runtime is the compiled Rust binary",
                "estimated_cost_ms": 0,
                "online_required": false,
                "auto_detected": true,
                "auto_fixed": false
            },
            {
                "id": "tz-mcp-server-entrypoint-declared",
                "check": "mcp_server_entrypoint_declared",
                "subsystem": "mcp",
                "severity": "ok",
                "description": "MCP server entrypoint is declared as tokenzero mcp-server",
                "estimated_cost_ms": 0,
                "online_required": false,
                "auto_detected": true,
                "auto_fixed": false
            }
        ],
        "fixers": [
            {
                "id": "tz-cache-parent-missing",
                "subsystem": "cache",
                "description": "create the missing cache parent directory inside the doctor root",
                "detector_id": "tz-cache-parent-missing",
                "writes_to": ["<configured-cache-parent>/", "<root>/.doctor/"],
                "ops": ["CreateDir"],
                "dry_run": true,
                "undo": true,
                "online_required": false,
                "preconditions": [
                    "root exists and is a directory",
                    "cache parent is inside root",
                    "cache parent immediate parent already exists",
                    "doctor lock is available"
                ]
            }
        ],
        "manual_remediations": [
            {
                "id": "tz-root-missing",
                "instruction": "Run tokenzero doctor --json --root <existing-directory>.",
                "reason": "The doctor cannot invent or create the project root safely."
            },
            {
                "id": "tz-root-not-directory",
                "instruction": "Run tokenzero doctor --json --root <directory>.",
                "reason": "The doctor root must be a directory so all checks have a bounded scope."
            },
            {
                "id": "tz-cache-parent-missing",
                "instruction": "Run tokenzero doctor --dry-run --fix --json, then tokenzero doctor --fix --json if the write set is acceptable.",
                "reason": "The doctor can repair this only when the cache parent is inside the root and the immediate parent already exists."
            }
        ],
        "exit_codes": doctor_exit_codes(),
        "env_vars": {
            "TOKENZERO_ROOT": "default root override for TokenZero commands",
            "NO_COLOR": "disable ANSI color in human-facing output"
        },
        "supports_fix": true,
        "supports_undo": true,
        "write_scopes": ["<root>/.doctor/", "<configured-cache-parent>/"],
        "online_probes": false,
        "run_artifact_schema": "tokenzero.doctor.run.v1",
        "report_schema": "tokenzero.doctor.v1",
        "negative_space": [
            "diagnose never mutates project state",
            "no network probes run by default",
            "fix only creates the missing cache parent; it never edits cache contents",
            "stdout is data only for JSON subcommands"
        ]
    })
}

pub fn doctor_exit_codes() -> serde_json::Value {
    serde_json::json!([
        {
            "code": 0,
            "label": "ok",
            "canonical_label": "success_or_healthy",
            "meaning": "healthy or informational findings only"
        },
        {
            "code": 1,
            "label": "blocked",
            "canonical_label": "findings_present_no_fix",
            "meaning": "blocking doctor finding; parse findings[] and next_steps[]"
        },
        {
            "code": 2,
            "label": "partial",
            "canonical_label": "fix_partial",
            "meaning": "fix attempted but only some actions completed"
        },
        {
            "code": 3,
            "label": "rolled_back_or_restore_failed",
            "canonical_label": "fix_failed_or_undo_failed",
            "meaning": "fix failed and rolled back, or undo could not restore safely"
        },
        {
            "code": 4,
            "label": "refused_unsafe",
            "canonical_label": "refused_unsafe",
            "meaning": "doctor refused an unsafe or unsupported operation"
        },
        {
            "code": 5,
            "label": "concurrency_lost",
            "canonical_label": "concurrency_lost",
            "meaning": "another doctor process holds the mutation lock"
        },
        {
            "code": 6,
            "label": "online_required",
            "canonical_label": "online_required",
            "meaning": "a detector or fixer requires explicit --online consent"
        },
        {
            "code": 64,
            "label": "usage_error",
            "canonical_label": "usage_error",
            "meaning": "unknown flag or malformed invocation"
        },
        {
            "code": 66,
            "label": "no_input",
            "canonical_label": "no_input",
            "meaning": "target path does not exist or is not usable"
        },
        {
            "code": 73,
            "label": "cannot_create_output",
            "canonical_label": "cannot_create_output",
            "meaning": "doctor could not create a requested report or run artifact"
        },
        {
            "code": 74,
            "label": "io_error",
            "canonical_label": "io_error",
            "meaning": "filesystem I/O error during read-only diagnosis"
        }
    ])
}

pub fn doctor_explain(
    root: &Path,
    cache_path: Option<&Path>,
    finding_id: &str,
) -> serde_json::Value {
    let report = doctor(root, cache_path);
    let current_finding = report["findings"].as_array().and_then(|findings| {
        findings
            .iter()
            .find(|finding| finding["id"].as_str() == Some(finding_id))
    });
    if let Some(finding) = current_finding {
        return serde_json::json!({
            "schema_version": "tokenzero.doctor.explain.v1",
            "status": "ok",
            "ok": true,
            "finding_id": finding_id,
            "current": true,
            "finding": finding,
            "next_steps": report["next_steps"],
            "capabilities_command": "tokenzero doctor capabilities --json"
        });
    }

    if let Some(known) = known_doctor_finding(finding_id) {
        return serde_json::json!({
            "schema_version": "tokenzero.doctor.explain.v1",
            "status": "ok",
            "ok": true,
            "finding_id": finding_id,
            "current": false,
            "finding": known,
            "next_steps": [{
                "priority": 1,
                "action": "rerun_diagnose",
                "command": "tokenzero doctor --json",
                "reason": "the finding is known but not present in the current diagnose output"
            }],
            "capabilities_command": "tokenzero doctor capabilities --json"
        });
    }

    serde_json::json!({
        "schema_version": "tokenzero.doctor.explain.v1",
        "status": "not_found",
        "ok": false,
        "exit_code": 1,
        "finding_id": finding_id,
        "known_finding_ids": [
            "tz-root-missing",
            "tz-root-not-directory",
            "tz-cache-parent-missing"
        ],
        "next_steps": [{
            "priority": 1,
            "action": "list_capabilities",
            "command": "tokenzero doctor capabilities --json",
            "reason": "capabilities lists every detector and finding id this read-only doctor knows"
        }]
    })
}

pub fn doctor_robot_triage(root: &Path, cache_path: Option<&Path>) -> serde_json::Value {
    let report = doctor(root, cache_path);
    let actions_planned = fixable_cache_parent_finding(&report)
        .map(|finding| {
            serde_json::json!([{
                "fixer_id": DOCTOR_FIXER_CACHE_PARENT,
                "finding_id": DOCTOR_FIXER_CACHE_PARENT,
                "description": "create missing cache parent directory",
                "path": finding["evidence"]["path"],
                "recommended_command": "tokenzero doctor --dry-run --fix --json"
            }])
        })
        .unwrap_or_else(|| serde_json::json!([]));
    let recommended_command = if actions_planned.as_array().is_some_and(|v| !v.is_empty()) {
        "tokenzero doctor --dry-run --fix --json"
    } else {
        report["next_steps"]
            .as_array()
            .and_then(|steps| steps.first())
            .and_then(|step| step.get("command"))
            .and_then(Value::as_str)
            .unwrap_or("tokenzero doctor --json")
    };
    serde_json::json!({
        "schema_version": "tokenzero.doctor.robot_triage.v1",
        "status": report["status"],
        "ok": report["ok"],
        "summary": report["summary"],
        "findings": report["findings"],
        "actions_planned": actions_planned,
        "recommended_command": recommended_command,
        "capabilities_url": "tokenzero doctor capabilities --json",
        "robot_docs_command": "tokenzero doctor robot-docs",
        "mutation_policy": "read-only by default; run --dry-run --fix before --fix"
    })
}

pub fn doctor_robot_docs() -> String {
    [
        "# TokenZero Doctor Robot Guide",
        "",
        "Canonical read-only commands:",
        "- `tokenzero doctor --json` diagnoses local root/cache/runtime health.",
        "- `tokenzero doctor health` prints a one-line liveness summary.",
        "- `tokenzero doctor capabilities --json` prints the machine-readable doctor contract.",
        "- `tokenzero doctor explain <finding-id>` expands a current or known finding.",
        "- `tokenzero doctor --robot-triage --json` returns summary, findings, planned actions, and next command in one JSON object.",
        "- `tokenzero doctor --dry-run --fix --json` plans the cache-parent repair without writing.",
        "- `tokenzero doctor --fix --json` creates the missing cache parent through the doctor mutate chokepoint.",
        "- `tokenzero doctor undo <run-id> --json` restores the cache-parent create-dir fixer when the directory is still empty.",
        "- `tokenzero doctor ls --json` lists local doctor run artifacts and undo commands.",
        "",
        "EXIT CODES:",
        "- `0`: healthy or informational findings only.",
        "- `1`: blocking finding; parse `findings[]` and `next_steps[]`.",
        "- `2`: fix partially completed.",
        "- `3`: fix rollback or undo failed.",
        "- `4`: unsafe or unsupported operation refused.",
        "- `5`: another doctor process holds the mutation lock.",
        "- `6`: explicit `--online` consent is required.",
        "- `64`: usage error.",
        "- `66`: no usable input path.",
        "- `73`: cannot create an output artifact.",
        "- `74`: read-only diagnosis hit an I/O error.",
        "",
        "JSON contract:",
        "- Stdout is data only for `--json` commands.",
        "- Stderr is empty unless process-level errors occur.",
        "- Every JSON object includes `schema_version`.",
        "- `capabilities.detectors[]` is the source of known finding ids.",
        "",
        "This doctor will NEVER do:",
        "- mutate project state during diagnose, health, capabilities, robot-docs, explain, or robot-triage.",
        "- run network probes by default.",
        "- edit cache contents; the only current fixer creates a missing cache parent directory.",
        "- write outside declared `write_scopes`.",
        "",
        "Next move for agents:",
        "1. Run `tokenzero doctor --json`.",
        "2. If `tz-cache-parent-missing` is present with `fix_supported=true`, run `tokenzero doctor --dry-run --fix --json`.",
        "3. Use `tokenzero doctor explain <finding-id>` for evidence and remediation detail.",
        "4. After `--fix`, save the returned `run_id`; use `tokenzero doctor undo <run-id> --json` to restore.",
        "",
    ]
    .join("\n")
}

pub(crate) fn known_doctor_finding(finding_id: &str) -> Option<serde_json::Value> {
    let finding = match finding_id {
        "tz-root-missing" => serde_json::json!({
            "id": "tz-root-missing",
            "severity": "error",
            "check": "root_exists",
            "summary": "doctor root does not exist",
            "remediation": {
                "command": "tokenzero doctor --json --root <existing-directory>",
                "auto_fixable": false,
                "reason": "the doctor cannot infer the intended project root"
            }
        }),
        "tz-root-not-directory" => serde_json::json!({
            "id": "tz-root-not-directory",
            "severity": "error",
            "check": "root_is_directory",
            "summary": "doctor root is not a directory",
            "remediation": {
                "command": "tokenzero doctor --json --root <directory>",
                "auto_fixable": false,
                "reason": "doctor checks require a bounded directory root"
            }
        }),
        "tz-cache-parent-missing" => serde_json::json!({
            "id": "tz-cache-parent-missing",
            "severity": "info",
            "check": "cache_parent_exists",
            "summary": "recovery cache parent does not exist yet",
            "remediation": {
                "command": "tokenzero doctor --dry-run --fix --json",
                "auto_fixable": true,
                "reason": "fix mode can create the cache parent when it is inside the root and the immediate parent exists"
            }
        }),
        _ => return None,
    };
    Some(finding)
}

pub(crate) const DOCTOR_CONTRACT_VERSION: &str = "1.2";
pub(crate) const DOCTOR_FIXER_CACHE_PARENT: &str = "tz-cache-parent-missing";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DoctorActionRecord {
    pub(crate) schema_version: String,
    pub(crate) path: String,
    pub(crate) op: String,
    pub(crate) before_hash: String,
    pub(crate) after_hash: String,
    pub(crate) before_exists: bool,
    pub(crate) after_exists: bool,
    pub(crate) run_id: String,
    pub(crate) fixer_id: String,
    pub(crate) ok: bool,
    pub(crate) backup_path: Option<String>,
    pub(crate) quarantine_path: Option<String>,
}

pub(crate) struct DoctorLock {
    pub(crate) file: fs::File,
}

impl DoctorLock {
    pub(crate) fn acquire(root: &Path) -> std::io::Result<Self> {
        let lock_dir = root.join(".doctor");
        fs::create_dir_all(&lock_dir)?;
        let lock_path = lock_dir.join("doctor.lock");
        let mut file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        match FileExt::try_lock(&file) {
            Ok(()) => {
                file.set_len(0)?;
                writeln!(&mut file, "{}", std::process::id())?;
                file.sync_all()?;
                Ok(Self { file })
            }
            Err(TryLockError::WouldBlock) => Err(Error::new(
                ErrorKind::WouldBlock,
                format!("doctor lock is already held at {}", lock_path.display()),
            )),
            Err(TryLockError::Error(err)) => Err(err),
        }
    }
}

impl Drop for DoctorLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub(crate) fn doctor_cache_path(root: &Path, cache_path: Option<&Path>) -> PathBuf {
    cache_path
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(".tokenzero/recovery-cache.json"))
}

pub(crate) fn doctor_cache_parent(root: &Path, cache_path: Option<&Path>) -> Option<PathBuf> {
    doctor_cache_path(root, cache_path)
        .parent()
        .map(Path::to_path_buf)
}

pub(crate) fn doctor_cache_parent_fixable(root: &Path, cache_parent: Option<&Path>) -> bool {
    let Some(cache_parent) = cache_parent else {
        return false;
    };
    root.exists()
        && root.is_dir()
        && !cache_parent.exists()
        && cache_parent.parent().is_some_and(Path::exists)
        && path_within_root(root, cache_parent).unwrap_or(false)
}

pub(crate) fn doctor_rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn doctor_run_id(root: &Path, cache: &Path) -> String {
    let unix = now_unix();
    let seed = format!("{}:{}:{unix}", root.display(), cache.display());
    let hash = sha256(&seed);
    format!("{unix}__{}", &hash[..6])
}
