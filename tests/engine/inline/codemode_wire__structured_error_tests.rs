use super::*;

#[test]
fn serialized_result_pins_minimum_and_legacy_envelope_fields() {
    let mut result = CodeModeResult::completed(Value::Null, Vec::new(), 0, 0, 0);
    result.set_visible_ack("0");
    let serialized = serde_json::to_value(result).expect("serialize CodeMode result");
    assert_eq!(
        serde_json::json!({
            "schema": serialized["schema"],
            "schema_version": serialized["schema_version"],
            "status": serialized["status"],
            "tool": serialized["tool"],
            "ack": serialized["ack"],
            "visible_ack": serialized["visible_ack"],
        }),
        serde_json::json!({
            "schema": "tokenzero.codemode.v1",
            "schema_version": "tokenzero.codemode.v1",
            "status": "completed",
            "tool": "codemode",
            "ack": "0",
            "visible_ack": "0",
        })
    );
}

#[test]
fn structured_json_error_keeps_punctuation_and_lists() {
    let result = CodeModeResult::error(
        r#"{"error":"unknown surface: framework","hint":"choose a supported surface","valid_surfaces":["authoring","constructors","ops"]}"#,
        0,
    );
    assert_eq!(result.ack, result.visible_ack);
    assert_eq!(
        result.to_line(),
        "codemode:error 9 ops=0 unknown surface: framework; hint: choose a supported surface; valid_surfaces: authoring, constructors, ops"
    );
}
