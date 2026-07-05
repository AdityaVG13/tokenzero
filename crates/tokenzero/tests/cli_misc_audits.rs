mod common;
use common::*;

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

    // ws_id must be non-empty and match WS-\d+ pattern
    let ws_id = json["ws_id"].as_str().expect("ws_id should be a string");
    assert!(!ws_id.is_empty(), "ws_id should be non-empty");
    assert!(
        ws_id.starts_with("WS-"),
        "ws_id should start with WS-, got {ws_id}"
    );
    let suffix = &ws_id[3..];
    assert!(
        !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()),
        "ws_id should match WS-<digits> pattern, got {ws_id}"
    );

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
        // No path traversal
        assert!(
            !path.contains(".."),
            "{path_field} must not contain path traversal, got {path}"
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

    // Re-read output_json from disk and verify it is valid JSON
    let disk_contents = std::fs::read(dir.path().join(output_json)).unwrap();
    let disk_json: Value =
        serde_json::from_slice(&disk_contents).expect("output_json on disk should be valid JSON");
    assert_eq!(disk_json["schema_version"], "tokenzero.ws_skeleton.v1");
}

#[test]
fn cli_exact_recovery_audit_covers_all_core_tool_families() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("recovery-audit.json");
    let json = run_tokenzero_json(&[
        "exact-recovery-audit",
        "--output-json",
        output_json.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.exact_recovery_audit.v1");
    assert_eq!(json["ok"], true);

    let normal_rows = json["normal_rows"].as_array().unwrap();
    let degraded_rows = json["degraded_rows"].as_array().unwrap();

    // One normal row and one degraded row per tool
    assert_eq!(
        normal_rows.len(),
        degraded_rows.len(),
        "normal_rows count ({}) must equal degraded_rows count ({})",
        normal_rows.len(),
        degraded_rows.len()
    );

    for tool in ["read", "find", "tree", "shell", "ingest"] {
        let row = normal_rows
            .iter()
            .find(|row| row["tool"] == tool)
            .unwrap_or_else(|| panic!("missing normal row for {tool}"));
        assert_eq!(row["all_refs_recover"], true, "{tool}");
        // Each normal row must check at least 2 refs
        assert!(
            row["refs_checked"].as_u64().unwrap() >= 2,
            "{tool} refs_checked should be >= 2, got {}",
            row["refs_checked"]
        );

        let degraded = degraded_rows
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
    let json = run_tokenzero_json(&[
        "protected-anchor-audit",
        "--output-json",
        output_json.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(
        json["schema_version"],
        "tokenzero.protected_anchor_audit.v1"
    );
    assert_eq!(json["ok"], true);
    assert_eq!(json["anchor_recall"], 1.0);

    let rows = json["rows"].as_array().unwrap();
    // At least 3 rows
    assert!(
        rows.len() >= 3,
        "expected at least 3 rows, got {}",
        rows.len()
    );

    for row in rows {
        assert_eq!(row["pass"], true, "{}", row["id"]);
        assert!(
            row["missing"].as_array().unwrap().is_empty(),
            "{}",
            row["id"]
        );
        let combined_ref = row["combined_ref"].as_str().unwrap();
        assert!(combined_ref.starts_with("tz://"));
        // combined_ref must have a non-empty hash portion after tz://blob/
        assert!(
            combined_ref.starts_with("tz://blob/"),
            "combined_ref should start with tz://blob/, got {combined_ref}"
        );
        let hash = &combined_ref["tz://blob/".len()..];
        assert!(
            !hash.is_empty(),
            "combined_ref hash portion after tz://blob/ must be non-empty, got {combined_ref}"
        );
    }
    assert!(output_json.exists());
}
