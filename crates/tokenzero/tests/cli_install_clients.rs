mod common;
use common::*;

use serde_json::Value;
use std::{fs, path::Path};
use tempfile::tempdir;

fn path_norm(row: &Value) -> String { row["path"].as_str().unwrap().replace('\\', "/") }
fn ends_with(rows: &[Value], suffix: &str) -> bool {
    rows.iter().any(|row| path_norm(row).ends_with(suffix))
}
fn root_json(root: &Path, args: &[&str]) -> Value {
    let mut full = args.to_vec();
    full.extend(["--root", root.to_str().unwrap(), "--json"]);
    run_tokenzero_json(&full)
}
fn assert_grok_writes(rows: &[Value]) {
    assert!(ends_with(rows, "/.grok/config.toml"));
    assert!(ends_with(rows, "/.config/tokenzero/agents/grok.mcp.json"));
    assert!(!ends_with(rows, "/.claude.json"));
}

#[test]
fn cli_grok_install_promised_invocations_plan_grok_targets() {
    let dir = tempdir().unwrap();
    for args in [
        &["install", "--mcp", "--grok", "--global", "--plan"][..],
        &["init", "--agent", "grok", "--global"][..],
    ] {
        let json = root_json(dir.path(), args);
        assert_eq!(json["status"], "planned", "{args:?}");
        assert_eq!(json["dry_run"], true, "{args:?}");
        assert_grok_writes(json["writes"].as_array().unwrap());
    }
}

#[test]
fn cli_install_hooks_and_shims_flags_plan_scoped_surfaces() {
    let dir = tempdir().unwrap();
    let plan = root_json(dir.path(), &["install", "--hooks", "--shims", "--global", "--plan"]);
    assert_eq!(plan["status"], "planned");
    assert_eq!(plan["dry_run"], true);
    let writes = plan["writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| {
        path_norm(row).ends_with("/.claude/settings.json")
            && row["capability"] == "hooks" && row["action"] == "merge"
    }));
    #[cfg(unix)]
    assert!(writes.iter().any(|row| {
        path_norm(row).ends_with("/.tokenzero/shims/cat") && row["capability"] == "shim"
    }));
    let grok = root_json(dir.path(), &["install", "--hooks", "--agent", "grok", "--global", "--plan"]);
    assert!(!grok["writes"].as_array().unwrap().iter().any(|row| path_norm(row).contains("/.claude")), "{grok}");
}

#[test]
fn cli_clients_docs_commands_are_wired_to_install_surfaces() {
    let dir = tempdir().unwrap();
    let detect = root_json(dir.path(), &["clients", "detect"]);
    assert_eq!(detect["schema_version"], "tokenzero.clients.v1");
    assert_eq!(detect["status"], "missing");
    assert_eq!(detect["summary"]["raw_bypass_risk"], true);
    assert!(detect["surfaces"].as_array().unwrap().iter().any(|row| {
        path_norm(row).ends_with("/.config/tokenzero/agents/codex.mcp.json")
    }));
    let status = root_json(dir.path(), &["client-status"]);
    assert_eq!(status["schema_version"], "tokenzero.clients.v1");
    assert_eq!(status["command"], "clients detect");
    let plan = root_json(dir.path(), &["clients", "plan", "--profile", "standard", "--agent", "grok"]);
    assert_eq!(plan["schema_version"], "tokenzero.clients.plan.v1");
    assert_eq!(plan["status"], "planned");
    assert_eq!(plan["profile"], "standard");
    let writes = plan["writes"].as_array().unwrap();
    assert!(ends_with(writes, "/.grok/config.toml"));
    assert!(!ends_with(writes, "/.claude.json"));
    let doctor = root_json(dir.path(), &["clients", "doctor"]);
    assert_eq!(doctor["schema_version"], "tokenzero.clients.v1");
    assert!(doctor["findings"].as_array().unwrap().iter().any(|row| row["id"] == "tz-clients-missing"));
}

#[test]
fn cli_clients_detect_rejects_broken_toml_command_despite_tokenzero_table() {
    let dir = tempdir().unwrap();
    let codex = dir.path().join(".codex/config.toml");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::write(&codex, "[mcp_servers.tokenzero]\ncommand = \"/bin/false\"\nargs = [\"mcp-server\"]\n").unwrap();
    let json = root_json(dir.path(), &["clients", "detect", "--agent", "codex"]);
    let row = json["surfaces"].as_array().unwrap().iter()
        .find(|row| path_norm(row).ends_with(".codex/config.toml")).unwrap();
    assert_eq!(row["state"], "mixed");
    assert_eq!(row["installed"], false);
    assert!(row["checks"].as_array().unwrap().iter().any(|check| {
        check["name"] == "mcp_command_targets_installed_runtime" && check["ok"] == false
    }));
}

#[test]
fn cli_clients_detect_reports_applied_grok_surfaces_as_installed() {
    let dir = tempdir().unwrap();
    root_json(dir.path(), &["install", "--mcp", "--grok", "--global", "--apply"]);
    let json = root_json(dir.path(), &["clients", "detect", "--agent", "grok"]);
    assert_eq!(json["status"], "installed", "detect status not installed; surfaces: {:#}", json["surfaces"]);
    assert_eq!(json["summary"]["raw_bypass_risk"], false);
    for suffix in [".config/tokenzero/agents/grok.mcp.json", ".grok/config.toml"] {
        let row = json["surfaces"].as_array().unwrap().iter()
            .find(|row| path_norm(row).ends_with(suffix)).unwrap();
        assert_eq!(row["state"], "installed", "{suffix}: {row:#}");
        assert!(row["checks"].as_array().unwrap().iter().all(|check| check["ok"] == true));
    }
}

#[test]
fn cli_clients_rollback_alias_restores_install_manifest() {
    let dir = tempdir().unwrap();
    root_json(dir.path(), &["install", "--mcp", "--grok", "--global", "--apply"]);
    let json = root_json(dir.path(), &["clients", "rollback", "latest"]);
    assert_eq!(json["schema_version"], "tokenzero.rollback.v1");
    assert_eq!(json["status"], "ok");
}

#[test]
fn cli_clients_scan_detects_harnesses_without_writing() {
    let home = tempdir().unwrap();
    fs::create_dir_all(home.path().join(".gemini")).unwrap();
    fs::create_dir_all(home.path().join(".config/zed")).unwrap();
    let output = assert_success(
        tokenzero_cmd().args(["clients", "scan", "--root", &home.path().display().to_string(), "--json"])
            .env("PATH", "").output().unwrap(),
        "clients scan",
    );
    let json = parse_json_stdout(&output);
    assert_eq!(json["schema_version"], "tokenzero.clients.v1");
    assert_eq!(json["command"], "clients scan");
    let detected = json["detected"].as_array().unwrap();
    assert!(detected.iter().any(|a| a["agent"] == "gemini" && a["supported"] == true), "{json}");
    assert!(detected.iter().any(|a| a["agent"] == "zed" && a["supported"] == false), "{json}");
    assert!(json["next_step"].as_str().unwrap().contains("--agent gemini"), "{json}");
    assert!(!home.path().join(".tokenzero").exists());
}
