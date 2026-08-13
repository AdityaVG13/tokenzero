use super::*;

#[test]
fn channels_gate_off_leaves_response_byte_identical() {
    let response = ToolResponse::default();
    let before = serde_json::to_string(&response).unwrap();
    let after = attach_channels_gated(response, "read", &json!({"path": ["src/main.rs"]}), false);
    assert!(after.channels.is_none());
    assert_eq!(serde_json::to_string(&after).unwrap(), before);
}

#[test]
fn channels_gate_on_attaches_action_status_and_null_user_message() {
    let response = attach_channels_gated(
        ToolResponse::default(),
        "read",
        &json!({"path": ["src/main.rs"]}),
        true,
    );
    let channels = response.channels.as_ref().expect("channels attached");
    assert_eq!(channels.action, "read");
    assert_eq!(channels.status_line, "Reading src/main.rs");
    assert_eq!(channels.user_message, None);
    let serialized = serde_json::to_value(&response).unwrap();
    let user_message = serialized
        .get("channels")
        .and_then(|c| c.get("user_message"));
    assert!(
        user_message.is_some(),
        "nullable user_message key must serialize, not be skipped"
    );
    assert_eq!(user_message, Some(&Value::Null));
}

#[test]
fn status_lines_are_deterministic_per_op() {
    let shell = attach_channels_gated(
        ToolResponse::default(),
        "shell",
        &json!({"command": "cargo test -p foo"}),
        true,
    );
    assert_eq!(
        shell.channels.unwrap().status_line,
        "Running cargo test -p foo"
    );
    let expand = attach_channels_gated(
        ToolResponse::default(),
        "expand",
        &json!({"ref": "tz://blob/ab12"}),
        true,
    );
    assert_eq!(
        expand.channels.unwrap().status_line,
        "Expanding tz://blob/ab12"
    );
    let glob = attach_channels_gated(
        ToolResponse::default(),
        "glob",
        &json!({"pattern": "**/*.rs"}),
        true,
    );
    assert_eq!(glob.channels.unwrap().status_line, "Globbing **/*.rs");
}
