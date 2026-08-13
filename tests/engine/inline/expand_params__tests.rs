use super::*;
use serde_json::json;

#[test]
fn from_codemode_args_accepts_positional_ref_string() {
    let params = ExpandParams::from_codemode_args(&[json!("tz://blob/abc")]).unwrap();
    assert_eq!(params.ref_id, "tz://blob/abc");
    assert!(!params.fresh);
}

#[test]
fn from_codemode_args_coerces_ref_object() {
    let params = ExpandParams::from_codemode_args(&[json!({
        "ref": "tz://blob/obj",
        "start_line": 2,
        "end_line": 4,
        "fresh": true,
    })])
    .unwrap();
    assert_eq!(params.ref_id, "tz://blob/obj");
    assert_eq!(params.start_line, Some(2));
    assert_eq!(params.end_line, Some(4));
    assert!(params.fresh);
}

#[test]
fn from_codemode_args_rejects_object_plus_trailing_opts() {
    let err =
        ExpandParams::from_codemode_args(&[json!({"ref": "tz://blob/x"}), json!({"fresh": true})])
            .unwrap_err();
    assert!(
        err.contains("object form takes a single"),
        "unexpected: {err}"
    );
}

#[test]
fn from_codemode_args_typed_error_names_signature_on_bad_shape() {
    let err = ExpandParams::from_codemode_args(&[json!(42)]).unwrap_err();
    assert!(
        err.contains("requires a tz:// ref string") && err.contains("got number"),
        "unexpected: {err}"
    );
}

#[test]
fn from_codemode_args_object_missing_ref_is_typed() {
    let err = ExpandParams::from_codemode_args(&[json!({"path": "nope.txt"})]).unwrap_err();
    assert!(err.contains("requires ref"), "unexpected: {err}");
}
