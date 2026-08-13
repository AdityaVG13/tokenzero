use super::*;

#[test]
fn inspect_artifact_missing_returns_reason_and_invalid() {
    let check = inspect_artifact(
        Path::new("/no/such/artifact.json"),
        Some("tokenzero.claim_audit.v1"),
        None,
        &[],
        false,
        true,
        false,
    );
    assert!(!check.present);
    assert!(!check.readable);
    assert!(!check.valid);
    assert_eq!(check.reasons, ["artifact missing"]);
}

#[test]
fn inspect_artifact_schema_match_and_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artifact.json");
    fs::write(&path, r#"{"schema_version":"tokenzero.claim_audit.v1"}"#).unwrap();

    let ok = inspect_artifact(
        &path,
        Some("tokenzero.claim_audit.v1"),
        None,
        &[],
        false,
        true,
        false,
    );
    assert!(ok.present);
    assert!(ok.readable);
    assert!(ok.valid);
    assert_eq!(ok.schema_matches, json!(true));
    assert!(ok.reasons.is_empty());

    let bad = inspect_artifact(&path, Some("other.v1"), None, &[], false, true, false);
    assert!(!bad.valid);
    assert_eq!(bad.schema_matches, json!(false));
    assert!(
        bad.reasons
            .iter()
            .any(|reason| reason == "schema_version mismatch"),
        "{:?}",
        bad.reasons
    );
}

#[test]
fn inspect_artifact_skips_parse_without_schema_or_json_flag() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, "not json").unwrap();
    let check = inspect_artifact(&path, None, None, &[], false, false, false);
    assert!(check.present);
    assert!(check.readable);
    assert!(check.valid);
    assert!(check.reasons.is_empty());
    assert_eq!(check.schema_matches, Json::Null);
}

#[test]
fn inspect_artifact_unreadable_json_sets_schema_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("artifact.json");
    fs::write(&path, "not json").unwrap();
    let check = inspect_artifact(
        &path,
        Some("tokenzero.claim_audit.v1"),
        None,
        &[],
        false,
        true,
        false,
    );
    assert!(check.present);
    assert!(check.readable);
    assert!(!check.valid);
    assert_eq!(check.schema_matches, json!(false));
    assert!(
        check
            .reasons
            .iter()
            .any(|reason| reason == "artifact JSON unreadable"),
        "{:?}",
        check.reasons
    );
}
