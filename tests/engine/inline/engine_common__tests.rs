use super::*;

#[test]
fn capsule_error_response_is_structured() {
    let response = capsule_error_response("read", "synthetic invariant failure".to_string());
    let error = response.error.expect("structured tool error");
    assert_eq!(error.code, "capsule_omission_invalid");
    assert!(error.message.contains("synthetic invariant failure"));
}
