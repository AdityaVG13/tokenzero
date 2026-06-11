use assert_cmd::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_grok_install_promised_invocations_plan_grok_targets() {
    let dir = tempdir().unwrap();

    let install = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install",
            "--mcp",
            "--grok",
            "--global",
            "--plan",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let json: Value = serde_json::from_slice(&install.stdout).unwrap();
    assert_eq!(json["status"], "planned");
    assert_eq!(json["dry_run"], true);
    let writes = json["writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.grok/config.toml")
    }));
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.config/tokenzero/agents/grok.mcp.json")
    }));
    assert!(!writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.claude.json")
    }));

    let init = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "init",
            "--agent",
            "grok",
            "--global",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let json: Value = serde_json::from_slice(&init.stdout).unwrap();
    assert_eq!(json["status"], "planned");
    assert_eq!(json["dry_run"], true);
    let writes = json["writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.grok/config.toml")
    }));
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.config/tokenzero/agents/grok.mcp.json")
    }));
    assert!(!writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.claude.json")
    }));
}

#[test]
fn cli_install_hooks_and_shims_flags_plan_scoped_surfaces() {
    let dir = tempdir().unwrap();

    let plan = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install",
            "--hooks",
            "--shims",
            "--global",
            "--plan",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let json: Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(json["status"], "planned");
    assert_eq!(json["dry_run"], true);
    let writes = json["writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.claude/settings.json")
            && row["capability"] == "hooks"
            && row["action"] == "merge"
    }));
    #[cfg(unix)]
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.tokenzero/shims/cat")
            && row["capability"] == "shim"
    }));

    // Hooks writes are claude-scoped: a grok-only plan must not list any
    // .claude path even with --hooks requested.
    let grok = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install",
            "--hooks",
            "--agent",
            "grok",
            "--global",
            "--plan",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        grok.status.success(),
        "{}",
        String::from_utf8_lossy(&grok.stderr)
    );
    let grok_json: Value = serde_json::from_slice(&grok.stdout).unwrap();
    assert!(
        !grok_json["writes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["path"]
                .as_str()
                .unwrap()
                .replace('\\', "/")
                .contains("/.claude")),
        "{grok_json}"
    );
}

#[test]
fn cli_clients_docs_commands_are_wired_to_install_surfaces() {
    let dir = tempdir().unwrap();

    let detect = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "detect",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        detect.status.success(),
        "{}",
        String::from_utf8_lossy(&detect.stderr)
    );
    let detect_json: Value = serde_json::from_slice(&detect.stdout).unwrap();
    assert_eq!(detect_json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(detect_json["status"], "missing");
    assert_eq!(detect_json["summary"]["raw_bypass_risk"], true);
    assert!(
        detect_json["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["path"]
                .as_str()
                .unwrap()
                .replace('\\', "/")
                .ends_with("/.config/tokenzero/agents/codex.mcp.json"))
    );

    let status = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "client-status",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status_json: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(status_json["command"], "clients detect");

    let plan = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "plan",
            "--profile",
            "standard",
            "--agent",
            "grok",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        plan.status.success(),
        "{}",
        String::from_utf8_lossy(&plan.stderr)
    );
    let plan_json: Value = serde_json::from_slice(&plan.stdout).unwrap();
    assert_eq!(plan_json["schema_version"], "tokenzero.clients.plan.v1");
    assert_eq!(plan_json["status"], "planned");
    assert_eq!(plan_json["profile"], "standard");
    let writes = plan_json["writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.grok/config.toml")
    }));
    assert!(!writes.iter().any(|row| {
        row["path"]
            .as_str()
            .unwrap()
            .replace('\\', "/")
            .ends_with("/.claude.json")
    }));

    let doctor = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "doctor",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor_json["schema_version"], "tokenzero.clients.v1");
    assert!(
        doctor_json["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "tz-clients-missing")
    );
}

#[test]
fn cli_clients_detect_rejects_broken_toml_command_despite_tokenzero_table() {
    let dir = tempdir().unwrap();
    let codex = dir.path().join(".codex/config.toml");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(
        &codex,
        "[mcp_servers.tokenzero]\ncommand = \"/bin/false\"\nargs = [\"mcp-server\"]\n",
    )
    .unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "detect",
            "--agent",
            "codex",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let codex_row = json["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["path"]
                .as_str()
                .unwrap()
                .replace('\\', "/")
                .ends_with(".codex/config.toml")
        })
        .unwrap();
    assert_eq!(codex_row["state"], "mixed");
    assert_eq!(codex_row["installed"], false);
    assert!(codex_row["checks"].as_array().unwrap().iter().any(|check| {
        check["name"] == "mcp_command_targets_installed_runtime" && check["ok"] == false
    }));
}

#[test]
fn cli_clients_detect_reports_applied_grok_surfaces_as_installed() {
    let dir = tempdir().unwrap();
    let apply = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install",
            "--mcp",
            "--grok",
            "--global",
            "--apply",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "detect",
            "--agent",
            "grok",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        json["status"], "installed",
        "detect status not installed; surfaces: {:#}",
        json["surfaces"]
    );
    assert_eq!(json["summary"]["raw_bypass_risk"], false);
    for suffix in [
        ".config/tokenzero/agents/grok.mcp.json",
        ".grok/config.toml",
    ] {
        let row = json["surfaces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["path"]
                    .as_str()
                    .unwrap()
                    .replace('\\', "/")
                    .ends_with(suffix)
            })
            .unwrap();
        assert_eq!(row["state"], "installed", "{suffix}: {row:#}");
        assert!(
            row["checks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|check| check["ok"] == true)
        );
    }
}

#[test]
fn cli_clients_rollback_alias_restores_install_manifest() {
    let dir = tempdir().unwrap();
    let apply = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "install",
            "--mcp",
            "--grok",
            "--global",
            "--apply",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );

    let rollback = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "rollback",
            "latest",
            "--root",
            dir.path().to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        rollback.status.success(),
        "{}",
        String::from_utf8_lossy(&rollback.stderr)
    );
    let json: Value = serde_json::from_slice(&rollback.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.rollback.v1");
    assert_eq!(json["status"], "ok");
}

#[test]
fn cli_clients_scan_detects_harnesses_without_writing() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
    std::fs::create_dir_all(home.path().join(".config/zed")).unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "clients",
            "scan",
            "--root",
            &home.path().display().to_string(),
            "--json",
        ])
        // Deterministic on any machine: no PATH binaries leak in.
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(json["command"], "clients scan");
    let detected = json["detected"].as_array().unwrap();
    assert!(
        detected
            .iter()
            .any(|agent| agent["agent"] == "gemini" && agent["supported"] == true),
        "{json}"
    );
    assert!(
        detected
            .iter()
            .any(|agent| agent["agent"] == "zed" && agent["supported"] == false),
        "{json}"
    );
    assert!(
        json["next_step"]
            .as_str()
            .unwrap()
            .contains("--agent gemini"),
        "{json}"
    );
    // Detection never writes anything into the scanned home.
    assert!(!home.path().join(".tokenzero").exists());
}
