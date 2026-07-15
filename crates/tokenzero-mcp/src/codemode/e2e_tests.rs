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
