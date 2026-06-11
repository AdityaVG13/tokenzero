use anyhow::Result;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn run_reach(root: PathBuf, output_json: Option<PathBuf>) -> Result<serde_json::Value> {
    let agents = root.join("AGENTS.md");
    let wrapper_audit = installed_tokenzero_command_audit();
    let wrapper_intercepted = wrapper_audit["resolved_is_current_exe"] == true;
    let wrapper_evidence = wrapper_audit["resolved_path"]
        .as_str()
        .filter(|path| !path.is_empty())
        .unwrap_or("tokenzero command not found on PATH")
        .to_string();
    let wrapper_repair_action = if wrapper_intercepted {
        "PATH tokenzero resolves to the current executable"
    } else {
        "use the current worktree release binary for verification or run an explicitly approved install apply before relying on global tokenzero"
    };
    let release_verification_binary = wrapper_audit["current_exe"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let rows = vec![
        json!({
            "host": "Codex",
            "surface": "AGENTS.md",
            "intercepted": agents.exists(),
            "bypassed": !agents.exists(),
            "unsupported": false,
            "repairable": true,
            "repair_action": if agents.exists() { "thin local policy pointer detected" } else { "add TokenZero pointer to AGENTS.md or run install plan" },
            "evidence": agents.display().to_string()
        }),
        json!({
            "host": "Claude Code",
            "surface": "CLAUDE.md / instructions",
            "intercepted": false,
            "bypassed": true,
            "unsupported": false,
            "repairable": true,
            "repair_action": "run tokenzero install --plan --instructions before applying any global write",
            "evidence": "plan-only; no global mutation performed"
        }),
        json!({
            "host": "Cursor",
            "surface": "MCP",
            "intercepted": false,
            "bypassed": true,
            "unsupported": false,
            "repairable": true,
            "repair_action": "configure TokenZero MCP explicitly; no daemon required",
            "evidence": "plan-only; no host config mutated"
        }),
        json!({
            "host": "Gemini",
            "surface": "CLI instructions",
            "intercepted": false,
            "bypassed": true,
            "unsupported": false,
            "repairable": true,
            "repair_action": "add local instructions or MCP route where supported",
            "evidence": "plan-only; no host config mutated"
        }),
        json!({
            "host": "Copilot",
            "surface": "MCP / editor integration",
            "intercepted": false,
            "bypassed": true,
            "unsupported": false,
            "repairable": true,
            "repair_action": "configure MCP or editor task wrapper; no daemon required",
            "evidence": "plan-only; no host config mutated"
        }),
        json!({
            "host": "OpenCode",
            "surface": "shell wrapper",
            "intercepted": false,
            "bypassed": true,
            "unsupported": false,
            "repairable": true,
            "repair_action": "use tokenzero run/read/find/tree explicitly or install a local wrapper plan",
            "evidence": "plan-only; no host config mutated"
        }),
        json!({
            "host": "Local shell",
            "surface": "tokenzero command",
            "intercepted": wrapper_intercepted,
            "bypassed": !wrapper_intercepted,
            "unsupported": false,
            "repairable": true,
            "repair_action": wrapper_repair_action,
            "evidence": wrapper_evidence,
            "details": wrapper_audit
        }),
    ];
    let report = json!({
        "schema_version": "tokenzero.reach.v1",
        "status": "ok",
        "ok": true,
        "root": root.display().to_string(),
        "daemon_required": false,
        "global_writes": false,
        "installed_wrapper_audit": wrapper_audit,
        "global_tokenzero_release_verification_trusted": wrapper_intercepted,
        "approved_install_required_for_global_update": !wrapper_intercepted,
        "release_verification_binary": release_verification_binary,
        "rows": rows
    });
    if let Some(output) = output_json {
        write_json_artifact(&output, &report)?;
    }
    Ok(report)
}

pub(crate) fn installed_tokenzero_command_audit() -> serde_json::Value {
    let current_exe = std::env::current_exe().ok();
    let candidates = tokenzero_path_candidates();
    let resolved_path = candidates.first().cloned();
    let resolved_is_current_exe = current_exe
        .as_ref()
        .zip(resolved_path.as_ref())
        .is_some_and(|(current, resolved)| same_path_for_audit(current, resolved));
    let current_exe_on_path = current_exe.as_ref().is_some_and(|current| {
        candidates
            .iter()
            .any(|candidate| same_path_for_audit(current, candidate))
    });
    let status = if candidates.is_empty() {
        "missing"
    } else if resolved_is_current_exe {
        "current_exe"
    } else {
        "external_or_wrapper"
    };
    json!({
        "schema_version": "tokenzero.installed_wrapper_audit.v1",
        "status": status,
        "command": "tokenzero",
        "current_exe": current_exe
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        "resolved_path": resolved_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
        "candidate_paths": candidates
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        "candidate_count": candidates.len(),
        "resolved_is_current_exe": resolved_is_current_exe,
        "current_exe_on_path": current_exe_on_path,
        "approved_install_required_for_global_update": !resolved_is_current_exe,
        "daemon_required": false,
        "global_writes": false
    })
}

fn tokenzero_path_candidates() -> Vec<PathBuf> {
    let Some(path_var) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    let command_names = tokenzero_command_names();
    let mut candidates = Vec::new();
    for dir in std::env::split_paths(&path_var) {
        for name in &command_names {
            let candidate = dir.join(name);
            if is_tokenzero_command_candidate(&candidate) {
                push_unique_audit_path(&mut candidates, candidate);
            }
        }
    }
    candidates
}

fn is_tokenzero_command_candidate(candidate: &Path) -> bool {
    let Ok(metadata) = candidate.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn tokenzero_command_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec![
            "tokenzero.exe",
            "tokenzero.cmd",
            "tokenzero.bat",
            "tokenzero.ps1",
            "tokenzero",
        ]
    } else {
        vec!["tokenzero"]
    }
}

fn push_unique_audit_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths
        .iter()
        .any(|existing| same_path_for_audit(existing, &candidate))
    {
        paths.push(candidate);
    }
}

fn same_path_for_audit(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.display()
            .to_string()
            .eq_ignore_ascii_case(&right.display().to_string())
    } else {
        left == right
    }
}

fn write_json_artifact(output_json: &Path, report: &serde_json::Value) -> Result<()> {
    if let Some(parent) = output_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output_json, serde_json::to_string_pretty(report)? + "\n")?;
    Ok(())
}

#[cfg(test)]
mod tests;
