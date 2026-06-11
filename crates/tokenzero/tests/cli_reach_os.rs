use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_reach_reports_daemonless_host_surfaces() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "Use TokenZero.\n").unwrap();
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["reach", "--root", dir.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.reach.v1");
    assert_eq!(json["daemon_required"], false);
    assert_eq!(
        json["global_tokenzero_release_verification_trusted"],
        json["installed_wrapper_audit"]["resolved_is_current_exe"]
    );
    assert_eq!(
        json["approved_install_required_for_global_update"],
        !json["global_tokenzero_release_verification_trusted"]
            .as_bool()
            .unwrap()
    );
    assert_eq!(json["installed_wrapper_audit"]["daemon_required"], false);
    assert_eq!(json["installed_wrapper_audit"]["global_writes"], false);
    assert!(
        json["release_verification_binary"]
            .as_str()
            .unwrap()
            .contains("tokenzero")
    );
    assert!(json["rows"].as_array().unwrap().iter().any(|row| {
        row["host"] == "Codex" && row["surface"] == "AGENTS.md" && row["intercepted"] == true
    }));
    assert!(json["rows"].as_array().unwrap().iter().any(|row| {
        row["host"] == "Copilot"
            && row["bypassed"] == true
            && row["repair_action"].as_str().unwrap().contains("MCP")
    }));
    let shell_row = json["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["host"] == "Local shell" && row["surface"] == "tokenzero command")
        .expect("local shell tokenzero command row");
    assert_eq!(shell_row["unsupported"], false);
    assert_eq!(shell_row["repairable"], true);
    assert_eq!(shell_row["details"]["daemon_required"], false);
    assert_eq!(shell_row["details"]["global_writes"], false);
    assert!(
        shell_row["details"]["current_exe"]
            .as_str()
            .unwrap()
            .contains("tokenzero")
    );
    assert!(shell_row["details"]["status"].as_str().is_some());
}

#[cfg(unix)]
#[test]
fn cli_reach_ignores_non_executable_path_shadow() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("AGENTS.md"), "Use TokenZero.\n").unwrap();
    let stale_dir = dir.path().join("stale");
    std::fs::create_dir(&stale_dir).unwrap();
    let stale = stale_dir.join("tokenzero");
    std::fs::write(&stale, "#!/bin/sh\nexit 99\n").unwrap();
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o644)).unwrap();

    let tokenzero = assert_cmd::cargo::cargo_bin("tokenzero");
    let path = std::env::join_paths([stale_dir.as_path(), tokenzero.parent().unwrap()])
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let output = Command::new(&tokenzero)
        .env("PATH", path)
        .args(["reach", "--root", dir.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    let audit = &json["installed_wrapper_audit"];
    let stale = stale.display().to_string();

    assert_eq!(audit["resolved_is_current_exe"], true);
    assert_ne!(audit["resolved_path"].as_str().unwrap(), stale);
    assert!(
        !audit["candidate_paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate.as_str() == Some(stale.as_str()))
    );
}

#[test]
fn cli_os_reach_audit_blocks_public_os_claim_until_all_release_oses_run() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("os-reach.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "os-reach-audit",
            "--output-json",
            output_json.to_str().unwrap(),
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
    assert_eq!(json["schema_version"], "tokenzero.os_reach_audit.v1");
    assert_eq!(json["ok"], true);
    assert_eq!(json["daemon_required"], false);
    assert_eq!(json["global_writes"], false);
    assert_eq!(json["public_os_claim_approved"], false);
    assert!(
        json["os_rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| { row["os"] == std::env::consts::OS && row["shell_matrix"] == "run" })
    );
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("not run"))
    );
    assert_eq!(json["install_smoke"]["ok"], true);
    assert_eq!(json["install_smoke"]["global_writes"], false);
    let core_surfaces = json["core_surfaces"].as_array().unwrap();
    for surface in ["install", "doctor", "shell", "mcp", "cache_pack"] {
        let row = core_surfaces
            .iter()
            .find(|row| row["surface"] == surface)
            .unwrap_or_else(|| panic!("missing {surface} core surface"));
        assert_eq!(row["ok"], true, "{surface}");
        assert_eq!(row["daemon_required"], false, "{surface}");
        assert_eq!(row["global_writes"], false, "{surface}");
    }
    assert!(output_json.exists());
}

#[test]
fn cli_os_reach_audit_merges_external_os_artifacts_but_requires_release_approval() {
    let dir = tempdir().unwrap();
    let current_os = std::env::consts::OS;
    let external_artifacts = ["windows", "linux", "macos"]
        .into_iter()
        .filter(|os| *os != current_os)
        .map(|os| {
            let path = dir.path().join(format!("{os}-os-artifact.json"));
            std::fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({
                    "schema_version": "tokenzero.os_release_artifact.v1",
                    "release_candidate_id": "rc-fixture",
                    "os": os,
                    "shell_matrix": "run",
                    "install_smoke": "run",
                    "daemon_required": false,
                    "global_writes": false,
                    "evidence": "synthetic reviewed CI artifact"
                }))
                .unwrap(),
            )
            .unwrap();
            (os, path)
        })
        .collect::<Vec<_>>();

    let mut args = vec!["os-reach-audit".to_string()];
    for (_, path) in &external_artifacts {
        args.push("--os-artifact".to_string());
        args.push(path.to_str().unwrap().to_string());
    }
    args.push("--json".to_string());
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args(&args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["all_release_oses_run"], true);
    assert_eq!(json["public_os_claim_approved"], false);
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("explicit release approval not granted"))
    );
    for (os, _) in &external_artifacts {
        let row = json["os_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["os"] == *os)
            .unwrap_or_else(|| panic!("missing {os} row"));
        assert_eq!(row["claim_ready"], true, "{os}");
        assert_eq!(row["artifact_source"], "external", "{os}");
        assert_eq!(row["daemon_required"], false, "{os}");
        assert_eq!(row["global_writes"], false, "{os}");
    }

    let mut approved_args = args;
    approved_args.insert(1, "--release-approval".to_string());
    let approved = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args(&approved_args)
        .output()
        .unwrap();
    assert!(
        approved.status.success(),
        "{}",
        String::from_utf8_lossy(&approved.stderr)
    );
    let approved_json: Value = serde_json::from_slice(&approved.stdout).unwrap();
    assert_eq!(approved_json["all_release_oses_run"], true);
    assert_eq!(approved_json["public_os_claim_approved"], true);
    assert_eq!(
        approved_json["blocked_reasons"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn cli_os_reach_audit_rejects_mixed_release_candidate_os_artifacts() {
    let dir = tempdir().unwrap();
    let current_os = std::env::consts::OS;
    let external_artifacts = ["windows", "linux", "macos"]
        .into_iter()
        .filter(|os| *os != current_os)
        .enumerate()
        .map(|(idx, os)| {
            let path = dir.path().join(format!("{os}-os-artifact.json"));
            std::fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({
                    "schema_version": "tokenzero.os_release_artifact.v1",
                    "release_candidate_id": if idx == 0 { "rc-fixture" } else { "rc-other" },
                    "os": os,
                    "shell_matrix": "run",
                    "install_smoke": "run",
                    "daemon_required": false,
                    "global_writes": false,
                    "evidence": "synthetic reviewed CI artifact"
                }))
                .unwrap(),
            )
            .unwrap();
            path
        })
        .collect::<Vec<_>>();

    let mut args = vec![
        "os-reach-audit".to_string(),
        "--release-approval".to_string(),
    ];
    for path in &external_artifacts {
        args.push("--os-artifact".to_string());
        args.push(path.to_str().unwrap().to_string());
    }
    args.push("--json".to_string());
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-fixture")
        .args(&args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["all_release_oses_run"], true);
    assert_eq!(json["public_os_claim_approved"], false);
    assert_eq!(json["same_release_candidate"], false);
    assert!(
        json["blocked_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "OS release artifacts are not from the same release candidate")
    );
}

#[test]
fn cli_os_release_artifact_generates_mergeable_current_host_artifact() {
    let dir = tempdir().unwrap();
    let artifact_path = dir.path().join("current-os-release-artifact.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "os-release-artifact",
            "--output-json",
            artifact_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let artifact: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&artifact_path).unwrap()).unwrap();
    let current_os = std::env::consts::OS;
    assert_eq!(
        artifact["schema_version"],
        "tokenzero.os_release_artifact.v1"
    );
    assert_eq!(artifact["os"], current_os);
    assert_eq!(artifact["shell_matrix"], "run");
    assert_eq!(artifact["install_smoke"], "run");
    assert_eq!(artifact["daemon_required"], false);
    assert_eq!(artifact["global_writes"], false);
    assert_eq!(artifact["claim_ready"], true);
    assert_eq!(artifact["release_publication_allowed"], false);
    assert!(artifact["evidence"].as_str().unwrap().contains("local"));
    let core_surfaces = artifact["core_surfaces"].as_array().unwrap();
    for surface in ["install", "doctor", "shell", "mcp", "cache_pack"] {
        let row = core_surfaces
            .iter()
            .find(|row| row["surface"] == surface)
            .unwrap_or_else(|| panic!("missing {surface} core surface"));
        assert_eq!(row["ok"], true, "{surface}");
        assert_eq!(row["daemon_required"], false, "{surface}");
        assert_eq!(row["global_writes"], false, "{surface}");
    }

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "os-reach-audit",
            "--os-artifact",
            artifact_path.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let reach: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let current_row = reach["os_rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["os"] == current_os)
        .unwrap();
    assert_eq!(current_row["claim_ready"], true);
    assert_eq!(reach["public_os_claim_approved"], false);
}
