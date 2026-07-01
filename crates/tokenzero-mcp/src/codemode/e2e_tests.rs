use super::exec::make_engine_for_root;
use super::{CodeModeOptions, CodeModeStatus, execute_codemode_with_options};
use serde_json::json;

#[test]
fn codemode_records_execution_refs_for_recipe() {
    let work = tempfile::tempdir().unwrap();
    let result = execute_codemode_with_options(
        "compact: durable execution payload",
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
    assert_eq!(result.visible_ack, "C");
    assert!(
        result
            .execution_id
            .as_deref()
            .unwrap()
            .starts_with("cm://exec/")
    );
    let refs = result.execution_refs.as_ref().unwrap();
    assert!(
        refs["execution"]
            .as_str()
            .unwrap()
            .starts_with("codemode/execution/")
    );
    assert!(
        refs["stored"]["code"]
            .as_str()
            .unwrap()
            .starts_with("tz://")
    );
    assert!(
        refs["stored"]["telemetry"]
            .as_str()
            .unwrap()
            .starts_with("tz://")
    );
    assert_eq!(result.telemetry.kind.as_deref(), Some("recipe"));
    assert_eq!(result.telemetry.raw_leak, Some(false));
}

#[test]
fn logical_execution_refs_expand_directly() {
    let work = tempfile::tempdir().unwrap();
    let result = execute_codemode_with_options(
        "compact: logical ref payload",
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
    let engine = make_engine_for_root(work.path().to_path_buf());

    let code = engine.expand(refs["code"].as_str().unwrap(), None, None, None, None, None);
    assert_eq!(code.status, "ok");
    let code_text = code.visible.unwrap().text;
    assert!(
        code_text.contains("logical ref payload"),
        "expanded code ref contained: {code_text:?}"
    );

    let telemetry = engine.expand(
        refs["telemetry"].as_str().unwrap(),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(telemetry.status, "ok");
    assert!(telemetry.visible.unwrap().text.contains("logical_ops"));

    let execution = engine.expand(
        refs["execution"].as_str().unwrap(),
        None,
        None,
        None,
        None,
        None,
    );
    assert_eq!(execution.status, "ok");
    assert!(execution.visible.unwrap().text.contains("cm://exec/"));
}

#[test]
fn json_dag_compacts_then_expands_by_binding() {
    let work = tempfile::tempdir().unwrap();
    let plan = json!({
        "steps": [
            {"id": "c", "method": "zero.token.compact", "args": ["json dag payload"]},
            {"id": "e", "method": "zero.token.expand", "args": ["$c.ref"]}
        ],
        "return": {"roundtrip": "$e.text", "ref": "$c.ref"}
    })
    .to_string();
    let result = execute_codemode_with_options(
        &plan,
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
    assert!(
        value["roundtrip"]
            .as_str()
            .unwrap()
            .contains("json dag payload")
    );
    assert!(value["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(result.telemetry.kind.as_deref(), Some("json"));
    assert_eq!(result.telemetry.steps_run, Some(2));
}

#[test]
fn sandboxed_js_function_runs_against_token_namespace() {
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        export default async function ({ token }) {
            const c = await token.compact("sandbox js payload");
            return { ref: c.ref, text: c.text };
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
    assert_eq!(result.telemetry.kind.as_deref(), Some("code"));
}

#[test]
fn quickjs_executes_real_js_syntax_and_drains_promises() {
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        const payloads = ["a", "b", "c"].map((x, i) => `${i}:${x}`);
        const stored = await zero.token.compactMany(payloads);
        const echoed = await Promise.resolve(stored.items.map((item) => item.text).join("|"));
        return { echoed, count: stored.count };
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
    assert_eq!(value["echoed"].as_str(), Some("0:a|1:b|2:c"));
    assert_eq!(value["count"].as_u64(), Some(3));
    assert_eq!(result.telemetry.steps_run, Some(1));
}

#[test]
fn quickjs_enforces_microtask_cap() {
    let result = execute_codemode_with_options(
        "return await Promise.resolve(1)",
        CodeModeOptions {
            max_microtasks: 0,
            ..Default::default()
        },
    );
    assert_eq!(result.status, CodeModeStatus::Error);
    assert!(
        result.error.as_deref().unwrap().contains("microtask cap"),
        "unexpected error: {:?}",
        result.error
    );
}

#[test]
fn sandbox_denies_network_and_process_capabilities() {
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
            result.error.as_deref().unwrap().contains("sandbox"),
            "unexpected error: {:?}",
            result.error
        );
        assert!(result.execution_refs.is_some());
    }
}

#[test]
fn batch_compact_many_reports_coalesced_telemetry() {
    let work = tempfile::tempdir().unwrap();
    let payloads: Vec<String> = (0..100).map(|i| format!("payload-{i}")).collect();
    let plan = json!({
        "steps": [
            {"id": "many", "method": "zero.token.compactMany", "args": [payloads]}
        ],
        "return": "$many.count"
    })
    .to_string();
    let result = execute_codemode_with_options(
        &plan,
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
    assert_eq!(result.value.as_ref().unwrap().as_u64(), Some(100));
    assert_eq!(result.telemetry.logical_ops, Some(1));
    assert_eq!(result.telemetry.physical_ops, Some(1));
    assert_eq!(result.visible_ack, "C");
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
    assert_eq!(result.telemetry.raw_leak, Some(false));
    assert!(result.execution_refs.is_some());
}
