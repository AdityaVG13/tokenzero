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
        findings.push(doctor_finding(
            "tz-root-missing", "error", "root_exists", "doctor root does not exist",
            serde_json::json!({ "path": root.display().to_string(), "exists": false }),
            false, false, None, "Pass --root pointing at an existing project directory.",
        ));
    } else if !root_is_dir {
        findings.push(doctor_finding(
            "tz-root-not-directory", "error", "root_is_directory", "doctor root is not a directory",
            serde_json::json!({ "path": root.display().to_string(), "is_directory": false }),
            false, false, None, "Pass --root pointing at a directory.",
        ));
    }
    if !cache_parent_exists {
        let path = cache_parent
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| String::from(""));
        let recommended = if cache_parent_fixable {
            serde_json::json!(["tokenzero", "doctor", "--dry-run", "--fix", "--json"])
        } else {
            cache_parent
                .as_ref()
                .map(|p| serde_json::json!(["mkdir", "-p", p.display().to_string()]))
                .unwrap_or_else(|| serde_json::json!([]))
        };
        findings.push(doctor_finding(
            "tz-cache-parent-missing",
            "info",
            "cache_parent_exists",
            "recovery cache parent does not exist yet",
            serde_json::json!({ "path": path, "exists": false }),
            cache_parent_fixable,
            cache_parent_fixable,
            Some(recommended),
            "Create the cache parent first if this preflight must be clean before the first cache-writing command.",
        ));
    }
    let has_blocking_finding = findings.iter().any(finding_is_error);
    let cache_parent_path = cache_parent
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| String::from(""));
    let checks = vec![
        doctor_check("root_exists", root_exists, if root_exists { "ok" } else { "error" }, root.display().to_string()),
        doctor_check("root_is_directory", root_is_dir, if root_is_dir { "ok" } else { "error" }, root.display().to_string()),
        doctor_check("cache_parent_exists", cache_parent_exists, if cache_parent_exists { "ok" } else { "info" }, cache_parent_path),
        doctor_check("core_runtime_rust", true, "ok", "compiled Rust binary"),
        doctor_check("mcp_server_entrypoint_declared", true, "ok", "tokenzero mcp-server"),
    ];
    let next_steps = if has_blocking_finding {
        vec![doctor_next_step(
            "fix_blocking_findings",
            "tokenzero doctor --json --root <existing-directory>",
            "doctor refuses to claim health while the root is invalid",
        )]
    } else if findings.is_empty() {
        vec![doctor_next_step(
            "no_action_required",
            "tokenzero doctor --runtime --json",
            "run this only when a runtime plan probe is needed",
        )]
    } else {
        vec![doctor_next_step(
            "review_informational_findings",
            "tokenzero doctor --json",
            "only non-blocking preflight findings were detected",
        )]
    };
    let exit_code = if has_blocking_finding { 1 } else { 0 };
    let blocking_findings = findings.iter().filter(|f| finding_is_error(f)).count();
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
            "auto_fixable": findings.iter().filter(|f| f["auto_fix"].as_bool().unwrap_or(false)).count(),
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
            "parent": cache_parent.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| String::from("")),
            "parent_exists": cache_parent_exists
        },
        "mcp": { "ready": true, "server": "tokenzero mcp-server" },
        "runtime": { "required_for_core": true }
    })
}

pub fn doctor_fix(root: &Path, cache_path: Option<&Path>, dry_run: bool) -> serde_json::Value {
    let cache = doctor_cache_path(root, cache_path);
    let cache_parent = doctor_cache_parent(root, cache_path);
    let before = doctor(root, cache_path);
    if before["ok"] == false {
        let mut report = doctor_fix_base("refused", false, 4, dry_run, false, 0);
        report["findings"] = before["findings"].clone();
        report["refusal"] = serde_json::json!({
            "reason": "blocking findings must be resolved before fix mode mutates",
            "next_command": "tokenzero doctor --json --root <existing-directory>"
        });
        return report;
    }
    let Some(cache_parent) = cache_parent else {
        return doctor_fix_refused(dry_run, "cache path has no parent", &before);
    };
    if cache_parent.exists() {
        let mut report = doctor_fix_base("ok", true, 0, dry_run, false, 0);
        report["actions_planned"] = serde_json::json!([]);
        report["summary"] = serde_json::json!("cache parent already exists; no action taken");
        report["findings_before"] = before["findings"].clone();
        report["findings_after"] = doctor(root, cache_path)["findings"].clone();
        return report;
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
        "writes_to": [cache_parent.display().to_string(), root.join(".doctor").display().to_string()],
        "undo": "doctor undo <run-id>",
        "estimated_actions": 1
    });
    if dry_run {
        let mut report = doctor_fix_base("ok", true, 0, true, false, 0);
        report["actions_planned"] = serde_json::json!([planned_action]);
        report["findings_before"] = before["findings"].clone();
        report["summary"] = serde_json::json!("dry-run only; no filesystem changes were made");
        return report;
    }

    let lock = match DoctorLock::acquire(root) {
        Ok(lock) => lock,
        Err(err) if err.kind() == ErrorKind::WouldBlock => {
            let mut report = doctor_fix_base("concurrency_lost", false, 5, false, false, 0);
            report["actions_planned"] = serde_json::json!([planned_action]);
            report["refusal"] = serde_json::json!({
                "reason": "doctor mutation lock is held",
                "error": err.to_string()
            });
            return report;
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
    let mut report = doctor_fix_base("ok", true, 0, false, true, 1);
    report["run_id"] = serde_json::json!(run_id);
    report["run_dir"] = serde_json::json!(run_dir.display().to_string());
    report["actions"] = serde_json::json!(actions);
    report["actions_planned"] = serde_json::json!([planned_action]);
    report["findings_before"] = before["findings"].clone();
    report["findings_after"] = after["findings"].clone();
    report["undo_command"] = serde_json::json!(format!("tokenzero doctor undo {run_id} --json"));
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
                        "artifact": label, "path": path.display().to_string(), "error": err.to_string()
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
                return doctor_undo_status("failed", false, 3, run_id, Some(format!("could not resolve latest run: {err}")));
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
            return doctor_undo_status("failed", false, 3, &resolved_run_id, Some(format!("could not acquire doctor lock: {err}")));
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
                return doctor_undo_status("failed", false, 3, &resolved_run_id, Some(format!("could not parse actions.jsonl: {err}")));
            }
        }
    }
    let mut restored = Vec::new();
    for action in actions.iter().rev() {
        if action.fixer_id != DOCTOR_FIXER_CACHE_PARENT || action.op != "create_dir" {
            drop(lock);
            return doctor_undo_status(
                "failed",
                false,
                3,
                &resolved_run_id,
                Some(format!("unsupported undo action {} for {}", action.op, action.path)),
            );
        }
        let target = root.join(&action.path);
        if !target.exists() {
            restored.push(serde_json::json!({ "path": action.path, "status": "already_absent" }));
            continue;
        }
        if !target.is_dir() {
            drop(lock);
            return doctor_undo_failed(&resolved_run_id, &action.path, "created path is not a directory");
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
                return doctor_undo_failed(&resolved_run_id, &action.path, &format!("could not inspect directory: {err}"));
            }
        }
        let quarantine = run_dir.join("quarantine").join(&action.path);
        if let Some(parent) = quarantine.parent() {
            if let Err(err) = fs::create_dir_all(parent) {
                drop(lock);
                return doctor_undo_failed(&resolved_run_id, &action.path, &format!("could not create quarantine parent: {err}"));
            }
        }
        if let Err(err) = fs::rename(&target, &quarantine) {
            drop(lock);
            return doctor_undo_failed(&resolved_run_id, &action.path, &format!("could not quarantine created directory: {err}"));
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
            return doctor_ls_report(root, &runs_dir, "ok", true, 0, 0, vec![], None);
        }
        Err(err) => {
            return doctor_ls_report(
                root,
                &runs_dir,
                "failed",
                false,
                74,
                0,
                vec![],
                Some(format!("could not list doctor runs: {err}")),
            );
        }
    };
    let mut runs = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(file_type) = entry.file_type() else { continue; };
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
        let started_at_unix = run_id.split_once("__").and_then(|(prefix, _)| prefix.parse::<u64>().ok());
        runs.push(serde_json::json!({
            "run_id": run_id,
            "started_at_unix": started_at_unix,
            "path": run_dir.display().to_string(),
            "latest": latest.as_deref() == Some(run_id.as_str()),
            "status": report.as_ref().and_then(|value| value.get("status")).and_then(Value::as_str).unwrap_or("unknown"),
            "exit_code": report.as_ref().and_then(|value| value.get("exit_code")).and_then(Value::as_i64),
            "action_count": action_count,
            "has_report": report.is_some(),
            "has_undo": run_dir.join("undo.json").exists(),
            "undo_command": format!("tokenzero doctor undo {run_id} --json")
        }));
    }
    runs.sort_by(|left, right| {
        right["run_id"].as_str().unwrap_or("").cmp(left["run_id"].as_str().unwrap_or(""))
    });
    doctor_ls_report(root, &runs_dir, "ok", true, 0, runs.len(), runs, None)
}

fn doctor_ls_report(
    root: &Path, runs_dir: &Path, status: &str, ok: bool, exit_code: i64,
    run_count: usize, runs: Vec<Value>, error: Option<String>,
) -> Value {
    let mut value = serde_json::json!({
        "schema_version": "tokenzero.doctor.ls.v1",
        "status": status, "ok": ok, "exit_code": exit_code,
        "root": root.display().to_string(),
        "runs_dir": runs_dir.display().to_string(),
        "run_count": run_count, "runs": runs
    });
    if let Some(error) = error {
        value["error"] = serde_json::json!(error);
    }
    value
}

fn doctor_fix_base(status: &str, ok: bool, exit_code: i64, dry_run: bool, mutates: bool, actions_taken: i64) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.fix.v1",
        "status": status, "ok": ok, "exit_code": exit_code, "mode": "fix",
        "dry_run": dry_run, "mutates": mutates, "actions_taken": actions_taken
    })
}

pub(crate) fn doctor_fix_refused(
    dry_run: bool,
    reason: &str,
    report: &serde_json::Value,
) -> serde_json::Value {
    let mut value = doctor_fix_base("refused", false, 4, dry_run, false, 0);
    value["findings"] = report["findings"].clone();
    value["refusal"] = serde_json::json!({
        "reason": reason,
        "safe_alternative": "tokenzero doctor explain tz-cache-parent-missing --json"
    });
    value
}

pub(crate) fn doctor_fix_io_error(
    dry_run: bool,
    phase: &str,
    err: std::io::Error,
) -> serde_json::Value {
    let mut value = doctor_fix_base("failed", false, 3, dry_run, false, 0);
    value["error"] = serde_json::json!({
        "phase": phase,
        "message": err.to_string()
    });
    value
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

fn doctor_undo_status(status: &str, ok: bool, exit_code: i64, run_id: &str, error: Option<String>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "schema_version": "tokenzero.doctor.undo.v1",
        "status": status, "ok": ok, "exit_code": exit_code, "run_id": run_id
    });
    if let Some(error) = error {
        value["error"] = serde_json::json!(error);
    }
    value
}

pub(crate) fn doctor_mutate_create_dir(
    root: &Path,
    run_dir: &Path,
    run_id: &str,
    path: &Path,
) -> std::io::Result<DoctorActionRecord> {
    if path.exists() {
        return Err(Error::new(ErrorKind::AlreadyExists, format!("{} already exists", path.display())));
    }
    if !doctor_cache_parent_fixable(root, Some(path)) {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("{} is outside safe doctor write scopes", path.display()),
        ));
    }
    let rel = doctor_rel_path(root, path);
    let before_hash = sha256("missing");
    let backup_marker = run_dir.join("backups").join(format!("{}.missing.json", rel.replace('/', "__")));
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
    let after_hash = if path.exists() && path.is_dir() { sha256("dir:empty") } else { sha256("missing") };
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
    let write_action = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new().create(true).append(true).open(run_dir.join("actions.jsonl"))?;
        serde_json::to_writer(&mut file, &action).map_err(|err| Error::other(format!("serialize action: {err}")))?;
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

pub(crate) fn fixable_cache_parent_finding(report: &serde_json::Value) -> Option<&serde_json::Value> {
    report["findings"].as_array()?.iter().find(|finding| {
        finding["id"] == DOCTOR_FIXER_CACHE_PARENT && finding["fix_supported"].as_bool().unwrap_or(false)
    })
}

const DOCTOR_COMMANDS: &[(&str, &str, bool, bool)] = &[
    ("doctor --json", "read-only install and cache health report", false, true),
    ("doctor diagnose --json", "explicit spelling for the default read-only diagnose mode", false, true),
    ("doctor health", "cheap liveness summary for schedulers and agents", false, true),
    ("doctor capabilities --json", "machine-readable doctor contract", false, true),
    ("doctor robot-docs", "paste-ready agent handbook", false, false),
    ("doctor explain <finding-id>", "expand a current or known doctor finding", false, true),
    ("doctor --robot-triage --json", "single-call triage summary for agents", false, true),
    ("doctor --dry-run --fix --json", "plan the cache-parent repair without writing", false, true),
    ("doctor --fix --json", "repair the missing cache parent through the doctor mutate chokepoint", true, true),
    ("doctor fix --json", "explicit spelling for doctor --fix --json", true, true),
    ("doctor undo <run-id> --json", "restore the cache-parent create-dir fixer by quarantining the created empty directory", true, true),
    ("doctor ls --json", "list local doctor run artifacts with run ids, exit codes, and action counts", false, true),
    ("doctor --runtime --json", "read-only report plus local runtime command plan probe", false, true),
];

const DOCTOR_DETECTORS: &[(&str, &str, &str, &str, &str, u64, bool, bool)] = &[
    ("tz-root-missing", "root_exists", "workspace_root", "error", "doctor root path does not exist", 1, true, false),
    ("tz-root-not-directory", "root_is_directory", "workspace_root", "error", "doctor root path exists but is not a directory", 1, true, false),
    ("tz-cache-parent-missing", "cache_parent_exists", "cache", "info", "recovery cache parent directory does not exist yet", 1, true, true),
    ("tz-core-runtime-rust", "core_runtime_rust", "runtime", "ok", "core runtime is the compiled Rust binary", 0, true, false),
    ("tz-mcp-server-entrypoint-declared", "mcp_server_entrypoint_declared", "mcp", "ok", "MCP server entrypoint is declared as tokenzero mcp-server", 0, true, false),
];

const DOCTOR_MANUAL_REMEDIATIONS: &[(&str, &str, &str)] = &[
    ("tz-root-missing", "Run tokenzero doctor --json --root <existing-directory>.", "The doctor cannot invent or create the project root safely."),
    ("tz-root-not-directory", "Run tokenzero doctor --json --root <directory>.", "The doctor root must be a directory so all checks have a bounded scope."),
    ("tz-cache-parent-missing", "Run tokenzero doctor --dry-run --fix --json, then tokenzero doctor --fix --json if the write set is acceptable.", "The doctor can repair this only when the cache parent is inside the root and the immediate parent already exists."),
];

const DOCTOR_EXIT_CODE_ROWS: &[(i64, &str, &str, &str)] = &[
    (0, "ok", "success_or_healthy", "healthy or informational findings only"),
    (1, "blocked", "findings_present_no_fix", "blocking doctor finding; parse findings[] and next_steps[]"),
    (2, "partial", "fix_partial", "fix attempted but only some actions completed"),
    (3, "rolled_back_or_restore_failed", "fix_failed_or_undo_failed", "fix failed and rolled back, or undo could not restore safely"),
    (4, "refused_unsafe", "refused_unsafe", "doctor refused an unsafe or unsupported operation"),
    (5, "concurrency_lost", "concurrency_lost", "another doctor process holds the mutation lock"),
    (6, "online_required", "online_required", "a detector or fixer requires explicit --online consent"),
    (64, "usage_error", "usage_error", "unknown flag or malformed invocation"),
    (66, "no_input", "no_input", "target path does not exist or is not usable"),
    (73, "cannot_create_output", "cannot_create_output", "doctor could not create a requested report or run artifact"),
    (74, "io_error", "io_error", "filesystem I/O error during read-only diagnosis"),
];

const KNOWN_DOCTOR_FINDINGS: &[(&str, &str, &str, &str, &str, bool, &str)] = &[
    ("tz-root-missing", "error", "root_exists", "doctor root does not exist", "tokenzero doctor --json --root <existing-directory>", false, "the doctor cannot infer the intended project root"),
    ("tz-root-not-directory", "error", "root_is_directory", "doctor root is not a directory", "tokenzero doctor --json --root <directory>", false, "doctor checks require a bounded directory root"),
    ("tz-cache-parent-missing", "info", "cache_parent_exists", "recovery cache parent does not exist yet", "tokenzero doctor --dry-run --fix --json", true, "fix mode can create the cache parent when it is inside the root and the immediate parent exists"),
];

pub fn doctor_capabilities() -> serde_json::Value {
    let commands: Vec<Value> = DOCTOR_COMMANDS.iter().map(|(name, description, mutates, json)| {
        serde_json::json!({ "name": name, "description": description, "mutates": mutates, "json": json })
    }).collect();
    let detectors: Vec<Value> = DOCTOR_DETECTORS.iter().map(|(id, check, subsystem, severity, description, cost, auto_detected, auto_fixed)| {
        serde_json::json!({
            "id": id, "check": check, "subsystem": subsystem, "severity": severity,
            "description": description, "estimated_cost_ms": cost, "online_required": false,
            "auto_detected": auto_detected, "auto_fixed": auto_fixed
        })
    }).collect();
    let manual_remediations: Vec<Value> = DOCTOR_MANUAL_REMEDIATIONS.iter().map(|(id, instruction, reason)| {
        serde_json::json!({ "id": id, "instruction": instruction, "reason": reason })
    }).collect();
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
        "commands": commands,
        "detectors": detectors,
        "fixers": [{
            "id": "tz-cache-parent-missing", "subsystem": "cache",
            "description": "create the missing cache parent directory inside the doctor root",
            "detector_id": "tz-cache-parent-missing",
            "writes_to": ["<configured-cache-parent>/", "<root>/.doctor/"],
            "ops": ["CreateDir"], "dry_run": true, "undo": true, "online_required": false,
            "preconditions": [
                "root exists and is a directory",
                "cache parent is inside root",
                "cache parent immediate parent already exists",
                "doctor lock is available"
            ]
        }],
        "manual_remediations": manual_remediations,
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
    serde_json::Value::Array(
        DOCTOR_EXIT_CODE_ROWS.iter().map(|(code, label, canonical_label, meaning)| {
            serde_json::json!({ "code": code, "label": label, "canonical_label": canonical_label, "meaning": meaning })
        }).collect(),
    )
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
        return doctor_explain_ok(finding_id, true, finding.clone(), report["next_steps"].clone());
    }
    if let Some(known) = known_doctor_finding(finding_id) {
        return doctor_explain_ok(
            finding_id,
            false,
            known,
            serde_json::json!([{
                "priority": 1,
                "action": "rerun_diagnose",
                "command": "tokenzero doctor --json",
                "reason": "the finding is known but not present in the current diagnose output"
            }]),
        );
    }
    serde_json::json!({
        "schema_version": "tokenzero.doctor.explain.v1",
        "status": "not_found",
        "ok": false,
        "exit_code": 1,
        "finding_id": finding_id,
        "known_finding_ids": ["tz-root-missing", "tz-root-not-directory", "tz-cache-parent-missing"],
        "next_steps": [{
            "priority": 1,
            "action": "list_capabilities",
            "command": "tokenzero doctor capabilities --json",
            "reason": "capabilities lists every detector and finding id this read-only doctor knows"
        }]
    })
}

fn doctor_explain_ok(finding_id: &str, current: bool, finding: Value, next_steps: Value) -> Value {
    serde_json::json!({
        "schema_version": "tokenzero.doctor.explain.v1",
        "status": "ok", "ok": true, "finding_id": finding_id, "current": current,
        "finding": finding, "next_steps": next_steps,
        "capabilities_command": "tokenzero doctor capabilities --json"
    })
}

pub fn doctor_robot_triage(root: &Path, cache_path: Option<&Path>) -> serde_json::Value {
    let report = doctor(root, cache_path);
    let actions_planned = fixable_cache_parent_finding(&report)
        .map(|finding| serde_json::json!([{
            "fixer_id": DOCTOR_FIXER_CACHE_PARENT,
            "finding_id": DOCTOR_FIXER_CACHE_PARENT,
            "description": "create missing cache parent directory",
            "path": finding["evidence"]["path"],
            "recommended_command": "tokenzero doctor --dry-run --fix --json"
        }]))
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
    String::from(r###"# TokenZero Doctor Robot Guide

Canonical read-only commands:
- `tokenzero doctor --json` diagnoses local root/cache/runtime health.
- `tokenzero doctor health` prints a one-line liveness summary.
- `tokenzero doctor capabilities --json` prints the machine-readable doctor contract.
- `tokenzero doctor explain <finding-id>` expands a current or known finding.
- `tokenzero doctor --robot-triage --json` returns summary, findings, planned actions, and next command in one JSON object.
- `tokenzero doctor --dry-run --fix --json` plans the cache-parent repair without writing.
- `tokenzero doctor --fix --json` creates the missing cache parent through the doctor mutate chokepoint.
- `tokenzero doctor undo <run-id> --json` restores the cache-parent create-dir fixer when the directory is still empty.
- `tokenzero doctor ls --json` lists local doctor run artifacts and undo commands.

EXIT CODES:
- `0`: healthy or informational findings only.
- `1`: blocking finding; parse `findings[]` and `next_steps[]`.
- `2`: fix partially completed.
- `3`: fix rollback or undo failed.
- `4`: unsafe or unsupported operation refused.
- `5`: another doctor process holds the mutation lock.
- `6`: explicit `--online` consent is required.
- `64`: usage error.
- `66`: no usable input path.
- `73`: cannot create an output artifact.
- `74`: read-only diagnosis hit an I/O error.

JSON contract:
- Stdout is data only for `--json` commands.
- Stderr is empty unless process-level errors occur.
- Every JSON object includes `schema_version`.
- `capabilities.detectors[]` is the source of known finding ids.

This doctor will NEVER do:
- mutate project state during diagnose, health, capabilities, robot-docs, explain, or robot-triage.
- run network probes by default.
- edit cache contents; the only current fixer creates a missing cache parent directory.
- write outside declared `write_scopes`.

Next move for agents:
1. Run `tokenzero doctor --json`.
2. If `tz-cache-parent-missing` is present with `fix_supported=true`, run `tokenzero doctor --dry-run --fix --json`.
3. Use `tokenzero doctor explain <finding-id>` for evidence and remediation detail.
4. After `--fix`, save the returned `run_id`; use `tokenzero doctor undo <run-id> --json` to restore.
"###)
}

pub(crate) fn known_doctor_finding(finding_id: &str) -> Option<serde_json::Value> {
    KNOWN_DOCTOR_FINDINGS.iter().find(|(id, ..)| *id == finding_id).map(
        |(id, severity, check, summary, command, auto_fixable, reason)| {
            serde_json::json!({
                "id": id, "severity": severity, "check": check, "summary": summary,
                "remediation": { "command": command, "auto_fixable": auto_fixable, "reason": reason }
            })
        },
    )
}

fn doctor_finding(
    id: &str, severity: &str, check: &str, summary: &str, evidence: Value,
    auto_fix: bool, fix_supported: bool, recommended_argv: Option<Value>, next_step: &str,
) -> Value {
    let mut value = serde_json::json!({
        "id": id, "severity": severity, "status": "detected", "check": check, "summary": summary,
        "evidence": evidence, "auto_fix": auto_fix, "fix_supported": fix_supported, "next_step": next_step
    });
    if let Some(argv) = recommended_argv {
        value["recommended_argv"] = argv;
    }
    value
}

fn doctor_check(id: &str, ok: bool, severity: &str, evidence: impl Into<String>) -> Value {
    serde_json::json!({ "id": id, "ok": ok, "severity": severity, "evidence": evidence.into() })
}

fn doctor_next_step(action: &str, command: &str, reason: &str) -> Value {
    serde_json::json!({ "priority": 1, "action": action, "command": command, "reason": reason })
}

fn finding_is_error(finding: &Value) -> bool {
    finding.get("severity").and_then(Value::as_str).is_some_and(|severity| severity == "error")
}

pub(crate) const DOCTOR_CONTRACT_VERSION: &str = "1.2";
pub(crate) const DOCTOR_FIXER_CACHE_PARENT: &str = "tz-cache-parent-missing";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DoctorActionRecord {
    pub(crate) schema_version: String, pub(crate) path: String, pub(crate) op: String,
    pub(crate) before_hash: String, pub(crate) after_hash: String,
    pub(crate) before_exists: bool, pub(crate) after_exists: bool,
    pub(crate) run_id: String, pub(crate) fixer_id: String, pub(crate) ok: bool,
    pub(crate) backup_path: Option<String>, pub(crate) quarantine_path: Option<String>,
}

pub(crate) struct DoctorLock {
    pub(crate) file: fs::File,
}

impl DoctorLock {
    pub(crate) fn acquire(root: &Path) -> std::io::Result<Self> {
        let lock_dir = root.join(".doctor");
        fs::create_dir_all(&lock_dir)?;
        let lock_path = lock_dir.join("doctor.lock");
        let mut file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path)?;
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
    cache_path.map(PathBuf::from).unwrap_or_else(|| root.join(".tokenzero/recovery-cache.json"))
}

pub(crate) fn doctor_cache_parent(root: &Path, cache_path: Option<&Path>) -> Option<PathBuf> {
    doctor_cache_path(root, cache_path).parent().map(Path::to_path_buf)
}

pub(crate) fn doctor_cache_parent_fixable(root: &Path, cache_parent: Option<&Path>) -> bool {
    let Some(cache_parent) = cache_parent else { return false; };
    root.exists()
        && root.is_dir()
        && !cache_parent.exists()
        && cache_parent.parent().is_some_and(Path::exists)
        && path_within_root(root, cache_parent).unwrap_or(false)
}

pub(crate) fn doctor_rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

pub(crate) fn doctor_run_id(root: &Path, cache: &Path) -> String {
    let unix = now_unix();
    let hash = sha256(&format!("{}:{}:{unix}", root.display(), cache.display()));
    format!("{unix}__{}", &hash[..6])
}
