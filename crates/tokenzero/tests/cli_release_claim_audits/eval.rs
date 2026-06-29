use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use tempfile::tempdir;



#[test]
fn cli_one_shot_eval_reports_zero_critical_misses_with_refs() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("one-shot.json");
    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "one-shot-eval",
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
    assert_eq!(json["schema_version"], "tokenzero.one_shot_eval.v1");
    assert_eq!(json["critical_miss_rate"], 0.0);
    assert_eq!(json["overall_miss_rate"], 0.0);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    for trace_id in [
        "source_edit_anchor",
        "failure_diagnosis_anchor",
        "warning_changed_file_anchor",
        "diff_review_anchor",
        "recovery_degraded_anchor",
    ] {
        assert!(
            json["rows"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["trace_id"] == trace_id),
            "{trace_id}"
        );
    }
    assert!(json["rows"].as_array().unwrap().iter().all(|row| {
        row["planned_expands"].as_array().unwrap().is_empty()
            && row["unplanned_second_call"] == false
            && row["required_anchors_present"] == true
            && row["task_success"] == true
            && (row["refs_available"] == true || row["degraded_explicit"] == true)
    }));
    assert!(output_json.exists());
}
