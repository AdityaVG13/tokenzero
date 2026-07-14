use tempfile::tempdir;

use super::common::*;

#[test]
fn cli_one_shot_eval_reports_zero_critical_misses_with_refs() {
    let dir = tempdir().unwrap();
    let output_json = dir.path().join("one-shot.json");
    let json = run_tokenzero_json(&[
        "one-shot-eval",
        "--output-json",
        output_json.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(json["schema_version"], "tokenzero.one_shot_eval.v1");
    assert_eq!(json["critical_miss_rate"], 0.0);
    assert_eq!(json["overall_miss_rate"], 0.0);
    assert_eq!(json["public_claims_approved"], false);
    assert_eq!(json["release_publication_allowed"], false);
    let rows = json["rows"].as_array().unwrap();
    for trace_id in [
        "source_edit_anchor",
        "failure_diagnosis_anchor",
        "warning_changed_file_anchor",
        "diff_review_anchor",
        "recovery_degraded_anchor",
    ] {
        assert!(rows.iter().any(|r| r["trace_id"] == trace_id), "{trace_id}");
    }
    assert!(rows.iter().all(|row| {
        row["planned_expands"].as_array().unwrap().is_empty()
            && row["unplanned_second_call"] == false
            && row["required_anchors_present"] == true
            && row["task_success"] == true
            && (row["refs_available"] == true || row["degraded_explicit"] == true)
    }));
    assert!(output_json.exists());
}
