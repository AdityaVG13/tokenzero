use super::*;

#[test]
fn claim_gate_summary_keeps_release_candidate_detail_rows() {
    let gates = vec![
        json!({
            "id": "source_currency",
            "pass": true,
            "artifact_path": "results/current/tokenzero_source_currency.json",
            "reasons": [],
            "details": {}
        }),
        json!({
            "id": "release_candidate",
            "pass": false,
            "artifact_path": serde_json::Value::Null,
            "reasons": ["same-release-candidate evidence incomplete"],
            "details": {
                "release_candidate_ids": ["git-abc"],
                "artifacts": [{"artifact_id": "source_artifact"}]
            }
        }),
    ];

    let (passes, reasons, paths, release_candidate_ids, release_candidate_artifacts) =
        claim_gate_summary(&gates);

    assert_eq!(passes["source_currency"], true);
    assert_eq!(passes["release_candidate"], false);
    assert_eq!(
        reasons["release_candidate"][0],
        "same-release-candidate evidence incomplete"
    );
    assert_eq!(
        paths["source_currency"],
        "results/current/tokenzero_source_currency.json"
    );
    assert_eq!(release_candidate_ids, vec![json!("git-abc")]);
    assert_eq!(
        release_candidate_artifacts,
        vec![json!({"artifact_id": "source_artifact"})]
    );
}

#[test]
fn benchmark_run_rows_require_exact_expand_checks() {
    let row = json!({
        "tool": "competitor",
        "suite": "shell-heavy",
        "availability_status": "run",
        "fairness_notes": [],
        "raw_tokens": 10,
        "visible_tokens": 5,
        "recovery_tokens": 5,
        "safe_savings": 0.5,
        "harm_rate": 0.0,
        "task_success": true,
        "byte_perfect_recovery": true,
        "exact_expand_checks": []
    });
    let mut reasons = Vec::new();

    validate_benchmark_public_claim_row(&row, &mut reasons);

    assert_eq!(
        reasons,
        vec!["benchmark row has non-byte-perfect expand checks".to_string()]
    );
}
