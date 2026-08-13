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

#[test]
fn classify_error_kind_maps_needles_overlaps_and_unknowns() {
    let cases = [
        ("mutating binding denied", "policy"),
        ("MUTATION refused", "policy"),
        ("edit denied", "policy"),
        ("sandbox: timeout", "sandbox"),
        ("SANDBOX: uppercase prefix", "sandbox"),
        ("access denied", "sandbox"),
        ("quickjs boom", "sandbox"),
        ("parse error at 1", "validation"),
        ("invalid json", "validation"),
        ("empty plan", "validation"),
        ("missing method", "validation"),
        ("requires a steps array", "validation"),
        ("missing required argument", "validation"),
        ("outside allowed roots", "substrate"),
        ("file not found", "substrate"),
        ("no such file", "substrate"),
        ("missing target", "substrate"),
        ("missing_target", "substrate"),
        ("something else", "runtime"),
        ("", "runtime"),
        ("missing", "runtime"),
        ("argument", "runtime"),
        ("mutating binding denied by sandbox", "policy"),
        ("policy denied", "sandbox"),
        ("not a plan", "runtime"),
    ];
    for (message, kind) in cases {
        assert_eq!(classify_error_kind(message), kind, "{message}");
        let result = CodeModeResult::error(message, 0);
        assert_eq!(
            result.error.as_ref().map(|error| error.kind.as_str()),
            Some(kind),
            "envelope {message}"
        );
    }
}
