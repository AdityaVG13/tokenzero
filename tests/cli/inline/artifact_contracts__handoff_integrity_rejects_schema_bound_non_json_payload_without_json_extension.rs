use super::*;

#[test]
fn handoff_integrity_rejects_schema_bound_non_json_payload_without_json_extension() {
    let dir = tempfile::tempdir().unwrap();
    let artifact_path = dir.path().join("claim-audit.txt");
    fs::write(&artifact_path, "not json").unwrap();
    let artifact_path = artifact_path.to_string_lossy().into_owned();
    let artifacts = list! {handoff_artifact("claim_audit",&artifact_path, "claim audit fixture",)};

    let (matrix, all_required_present, all_required_valid) =
        handoff_artifact_integrity_matrix(&artifacts);

    assert!(all_required_present);
    assert!(!all_required_valid);
    let rows = matrix.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["id"], "claim_audit");
    assert_eq!(row["present"], true);
    assert_eq!(row["readable"], true);
    assert_eq!(row["expected_schema_version"], "tokenzero.claim_audit.v1");
    assert_eq!(row["schema_matches"], false);
    assert_eq!(row["valid"], false);
    assert!(
        row["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "artifact JSON unreadable")
    );
}
