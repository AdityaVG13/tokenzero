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
