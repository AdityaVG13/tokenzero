use super::{CodeModeOptions, CodeModeStatus, execute_codemode_with_options};

#[test]
fn async_function_wrapper_is_lowered_and_size_limited() {
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        async function run({ token }) {
            const compacted = await token.compact("async wrapper payload");
            return { ref: compacted.ref, text: compacted.text };
        }
    "#;
    let result = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert!(value["ref"].as_str().unwrap().starts_with("tz://"));

    let oversized = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            max_code_bytes: 1,
            ..Default::default()
        },
    );
    assert_eq!(oversized.status, CodeModeStatus::Error);
    assert!(
        oversized
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("max_code_bytes"),
        "unexpected error: {:?}",
        oversized.error
    );
}

#[test]
fn sandbox_denies_host_capabilities() {
    for plan in [
        "await fetch('https://example.com')",
        "process.env",
        "require('fs')",
        "setTimeout(() => 1, 1)",
        "const f = () => zero.edit('file.txt', []); return f();",
        "store.put('x')",
        "db.query('select 1')",
        "indexedDB.open('x')",
    ] {
        let result = execute_codemode_with_options(plan, CodeModeOptions::default());
        assert_eq!(
            result.status,
            CodeModeStatus::Error,
            "plan should fail: {plan}"
        );
        assert_eq!(result.visible_ack, "X0");
        assert!(
            result.error.as_ref().unwrap().message.contains("sandbox"),
            "unexpected error: {:?}",
            result.error
        );
        assert!(result.execution_refs.is_some());
    }
}

#[test]
fn output_guard_keeps_large_result_behind_refs() {
    let result = execute_codemode_with_options(
        "return \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"",
        CodeModeOptions {
            max_output_bytes: 8,
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert_eq!(value["truncated"].as_bool(), Some(true));
    let continuation = value["continuation_ref"]
        .as_str()
        .expect("autopage must emit continuation_ref");
    assert!(
        continuation.starts_with("tz://"),
        "continuation must be a tz ref: {continuation}"
    );
    assert!(
        !continuation.contains("envelope"),
        "continuation must point at terminal payload, not envelope: {continuation}"
    );
    assert!(
        result
            .execution_refs
            .as_ref()
            .and_then(|refs| refs.pointer("/stored/result"))
            .and_then(|v| v.as_str())
            == Some(continuation),
        "stored.result must equal continuation_ref: {:?}",
        result.execution_refs
    );
    assert_eq!(
        result
            .telemetry
            .extra
            .as_ref()
            .and_then(|extra| extra.get("raw_leak")),
        None
    );
    assert!(result.execution_refs.is_some());
}

#[test]
fn output_guard_autopage_emits_head_within_budget() {
    // tokenzero-result-cap-autopage-be8: oversized results must surface a head
    // slice in-budget plus one continuation ref to the terminal payload.
    // 400 ASCII chars → JSON string > 256-byte visible budget.
    let result = execute_codemode_with_options(
        "return \"abcdefghijklmnopqrstuvwxyz0123456789\".repeat(10)",
        CodeModeOptions {
            max_output_bytes: 256,
            ref_first: false,
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert_eq!(value["truncated"].as_bool(), Some(true));
    let head = value["head"].as_str().expect("head slice required");
    assert!(!head.is_empty(), "head must be non-empty within budget");
    let continuation = value["continuation_ref"]
        .as_str()
        .expect("continuation_ref required");
    assert!(continuation.starts_with("tz://"));
    assert!(!continuation.contains("envelope"));
    let visible_bytes = serde_json::to_vec(value).unwrap().len();
    assert!(
        visible_bytes <= 256,
        "autopage value must fit budget: {visible_bytes}"
    );
}

#[test]
fn envelope_v3_collapses_execution_refs_and_hides_store_block() {
    let work = tempfile::tempdir().unwrap();
    let result = execute_codemode_with_options(
        "return { answer: 42 }",
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    let refs = result.execution_refs.as_ref().unwrap();
    assert!(refs.get("execution").and_then(|v| v.as_str()).is_some());
    assert!(refs.get("envelope").and_then(|v| v.as_str()).is_some());
    assert!(
        refs.get("code").is_none(),
        "code must be derivable, not spelled: {refs}"
    );
    assert!(refs.get("steps").is_none(), "{refs}");
    assert!(refs.get("telemetry").is_none(), "{refs}");
    assert!(refs.get("result").is_none(), "{refs}");
    assert!(refs.get("error").is_none(), "{refs}");
    assert!(
        refs.pointer("/stored/code").is_none(),
        "store block must stay hidden: {refs}"
    );
    assert!(
        refs.pointer("/stored/envelope")
            .and_then(|v| v.as_str())
            .is_some_and(|r| r.starts_with("tz://")),
        "{refs}"
    );
    assert!(
        result
            .telemetry
            .extra
            .as_ref()
            .and_then(|extra| extra.get("plan_journals"))
            .is_none(),
        "empty plan_journals must not leak into telemetry"
    );
    assert!(
        result
            .execution_id
            .as_deref()
            .is_some_and(|id| id.starts_with("cm://exec/")),
        "{:?}",
        result.execution_id
    );
}

#[test]
fn envelope_v3_ack_uses_execution_id() {
    use crate::tools;
    use serde_json::json;

    let work = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(work.path()));
    let response = tools::dispatch_tool(
        &engine,
        "execute_code",
        "tz_execute_code",
        &json!({
            "plan": "return 7",
            "envelope": "v3",
            "root": work.path().to_string_lossy(),
        }),
    )
    .expect("dispatch");
    assert_eq!(response.status, "ok");
    let text = response
        .visible
        .as_ref()
        .map(|visible| visible.text.as_str())
        .unwrap_or("");
    assert!(
        text.contains("cm://exec/"),
        "v3 ack must include execution id: {text}"
    );
    assert!(!text.contains("execution_refs"), "{text}");
}

#[test]
fn envelope_v3_scalar_fold_keeps_structured_value() {
    // tokenzero-codemode-result-not-surfaced-jhh
    use crate::tools;
    use serde_json::json;

    let work = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig::for_root(work.path()));
    let response = tools::dispatch_tool(
        &engine,
        "execute_code",
        "tz_execute_code",
        &json!({
            "plan": "return 7",
            "envelope": "v3",
            "root": work.path().to_string_lossy(),
        }),
    )
    .expect("dispatch");
    assert_eq!(response.status, "ok");
    let text = response
        .visible
        .as_ref()
        .map(|visible| visible.text.as_str())
        .unwrap_or("");
    assert!(text.contains("=7"), "scalar should fold into v3 ack: {text}");
    let telemetry = response.telemetry.as_ref().expect("telemetry");
    assert_eq!(telemetry.get("result_surfaced"), Some(&json!(true)));
    assert_eq!(
        telemetry.pointer("/structuredContent/value"),
        Some(&json!(7)),
        "structuredContent.value must survive scalar fold"
    );
    let mcp = tools::mcp_tool_response(response);
    assert_eq!(
        mcp.pointer("/structuredContent/value"),
        Some(&json!(7)),
        "MCP wire must expose value for hub extractJsonPayload: {mcp}"
    );
}
