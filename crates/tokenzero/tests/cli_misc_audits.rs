use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_ws_skeleton_rollup_links_required_artifacts() {
    let dir = tempdir().unwrap();
    let output_json = "results/current/tokenzero_ws_001.json";
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .current_dir(dir.path())
        .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-ws-skeleton")
        .args(["ws-skeleton", "--output-json", output_json, "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.ws_skeleton.v1");
    assert_eq!(json["ws_id"], "WS-001");
    assert_eq!(json["ok"], true);
    assert_eq!(json["release_candidate_id"], "rc-ws-skeleton");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    for path_field in [
        "bench_artifact",
        "one_shot_artifact",
        "claim_audit_artifact",
    ] {
        let path = json[path_field].as_str().unwrap();
        assert!(
            path.starts_with("results/current/"),
            "{path_field} should use a durable current artifact path, got {path}"
        );
        assert!(
            !path.contains('\\') && !path.contains(':'),
            "{path_field} should be an OS-neutral relative JSON path, got {path}"
        );
    }
    for artifact in [
        "one_command_family",
        "one_file_read",
        "one_failure_trace",
        "one_competitor_unavailable_row",
        "one_exact_expand_check",
        "adaptive_mode_rationale",
        "degraded_mode_handling",
    ] {
        assert_eq!(json["artifacts"][artifact]["present"], true, "{artifact}");
    }
    assert_eq!(json["release_gates"]["public_claims_approved"], false);
    assert!(dir.path().join(output_json).exists());
}

#[test]
fn cli_exact_recovery_audit_covers_all_core_tool_families() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("recovery-audit.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "exact-recovery-audit",
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
    assert_eq!(json["schema_version"], "tokenzero.exact_recovery_audit.v1");
    assert_eq!(json["ok"], true);
    for tool in ["read", "find", "tree", "shell", "ingest"] {
        let row = json["normal_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing normal row for {tool}"));
        assert_eq!(row["all_refs_recover"], true, "{tool}");
        assert!(row["refs_checked"].as_u64().unwrap() > 0, "{tool}");

        let degraded = json["degraded_rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing degraded row for {tool}"));
        assert_eq!(degraded["degraded"], true, "{tool}");
        assert_eq!(degraded["refs_available"], false, "{tool}");
        assert!(
            degraded["repair_action"]
                .as_str()
                .unwrap()
                .contains("recovery cache")
        );
    }
    assert!(output_json.exists());
}

#[test]
fn cli_protected_anchor_audit_preserves_failure_corpus() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("anchors.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "protected-anchor-audit",
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
    assert_eq!(
        json["schema_version"],
        "tokenzero.protected_anchor_audit.v1"
    );
    assert_eq!(json["ok"], true);
    assert_eq!(json["anchor_recall"], 1.0);
    for row in json["rows"].as_array().unwrap() {
        assert_eq!(row["pass"], true, "{}", row["id"]);
        assert!(
            row["missing"].as_array().unwrap().is_empty(),
            "{}",
            row["id"]
        );
        assert!(row["combined_ref"].as_str().unwrap().starts_with("tz://"));
    }
    assert!(output_json.exists());
}

#[test]
fn cli_explain_runtime_preserves_compound_shell_command() {
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "run",
            "--explain-runtime",
            "--runtime-platform",
            "linux",
            "--",
            "echo ok | cat",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["execution_mode"], "shell");
    assert_eq!(json["shell"], "/bin/sh");
    assert_eq!(json["shell_arg"], "-c");
    assert_eq!(json["alias_dependency"], false);
}
