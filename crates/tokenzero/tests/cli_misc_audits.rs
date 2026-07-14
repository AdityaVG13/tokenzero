mod common;
use common::*;

use serde_json::Value;
use tempfile::tempdir;

#[test]
fn cli_ws_skeleton_rollup_links_required_artifacts() {
    let dir = tempdir().unwrap();
    let output_json = "results/current/tokenzero_ws_001.json";
    let output = assert_success(
        tokenzero_cmd().current_dir(dir.path())
            .env("TOKENZERO_RELEASE_CANDIDATE_ID", "rc-ws-skeleton")
            .args(["ws-skeleton", "--output-json", output_json, "--json"])
            .output().unwrap(),
        "ws-skeleton",
    );
    let json = parse_json_stdout(&output);
    assert_eq!(json["schema_version"], "tokenzero.ws_skeleton.v1");
    let ws_id = json["ws_id"].as_str().expect("ws_id should be a string");
    assert!(!ws_id.is_empty(), "ws_id should be non-empty");
    assert!(ws_id.starts_with("WS-"), "ws_id should start with WS-, got {ws_id}");
    let suffix = &ws_id[3..];
    assert!(!suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()), "ws_id should match WS-<digits> pattern, got {ws_id}");
    assert_eq!(json["ok"], true);
    assert_eq!(json["release_candidate_id"], "rc-ws-skeleton");
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    for field in ["bench_artifact", "one_shot_artifact", "claim_audit_artifact"] {
        let path = json[field].as_str().unwrap();
        assert!(path.starts_with("results/current/"), "{field} should use a durable current artifact path, got {path}");
        assert!(!path.contains('\\') && !path.contains(':'), "{field} should be an OS-neutral relative JSON path, got {path}");
        assert!(!path.contains(".."), "{field} must not contain path traversal, got {path}");
    }
    for artifact in [
        "one_command_family", "one_file_read", "one_failure_trace",
        "one_competitor_unavailable_row", "one_exact_expand_check",
        "adaptive_mode_rationale", "degraded_mode_handling",
    ] {
        assert_eq!(json["artifacts"][artifact]["present"], true, "{artifact}");
    }
    assert_eq!(json["release_gates"]["public_claims_approved"], false);
    assert!(dir.path().join(output_json).exists());
    let disk: Value = serde_json::from_slice(&std::fs::read(dir.path().join(output_json)).unwrap())
        .expect("output_json on disk should be valid JSON");
    assert_eq!(disk["schema_version"], "tokenzero.ws_skeleton.v1");
}

#[test]
fn cli_exact_recovery_audit_covers_all_core_tool_families() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("recovery-audit.json");
    let json = run_tokenzero_json(&[
        "exact-recovery-audit", "--output-json", output.to_str().unwrap(), "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.exact_recovery_audit.v1");
    assert_eq!(json["ok"], true);
    let normal = json["normal_rows"].as_array().unwrap();
    let degraded = json["degraded_rows"].as_array().unwrap();
    assert_eq!(normal.len(), degraded.len(), "normal_rows count ({}) must equal degraded_rows count ({})", normal.len(), degraded.len());
    for tool in ["read", "find", "tree", "shell", "ingest"] {
        let row = find_row_by(normal, "tool", tool);
        assert_eq!(row["all_refs_recover"], true, "{tool}");
        assert!(row["refs_checked"].as_u64().unwrap() >= 2, "{tool} refs_checked should be >= 2, got {}", row["refs_checked"]);
        let row = find_row_by(degraded, "tool", tool);
        assert_eq!(row["degraded"], true, "{tool}");
        assert_eq!(row["refs_available"], false, "{tool}");
        assert!(row["repair_action"].as_str().unwrap().contains("recovery cache"));
    }
    assert!(output.exists());
}

#[test]
fn cli_protected_anchor_audit_preserves_failure_corpus() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("anchors.json");
    let json = run_tokenzero_json(&[
        "protected-anchor-audit", "--output-json", output.to_str().unwrap(), "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.protected_anchor_audit.v1");
    assert_eq!(json["ok"], true);
    assert_eq!(json["anchor_recall"], 1.0);
    let rows = json["rows"].as_array().unwrap();
    assert!(rows.len() >= 3, "expected at least 3 rows, got {}", rows.len());
    for row in rows {
        assert_eq!(row["pass"], true, "{}", row["id"]);
        assert!(row["missing"].as_array().unwrap().is_empty(), "{}", row["id"]);
        let reference = row["combined_ref"].as_str().unwrap();
        assert!(reference.starts_with("tz://"));
        assert!(reference.starts_with("tz://blob/"), "combined_ref should start with tz://blob/, got {reference}");
        assert!(!reference["tz://blob/".len()..].is_empty(), "combined_ref hash portion after tz://blob/ must be non-empty, got {reference}");
    }
    assert!(output.exists());
}
