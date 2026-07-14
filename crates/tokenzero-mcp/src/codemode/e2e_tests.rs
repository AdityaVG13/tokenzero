use super::exec::make_engine_for_root;
use super::{CodeModeOptions, CodeModeResult, CodeModeStatus, execute_codemode_with_options};
use serde_json::{Value, json};
use std::path::Path;

fn run(root: &Path, plan: &str) -> CodeModeResult {
    execute_codemode_with_options(plan, CodeModeOptions { root: Some(root.into()), ..Default::default() })
}
fn completed(result: &CodeModeResult) -> &Value {
    assert_eq!(result.status, CodeModeStatus::Completed, "{:?}", result.error);
    assert_eq!(result.telemetry.kind, "codemode.execute");
    result.value.as_ref().unwrap()
}

#[test]
fn codemode_records_execution_refs_for_recipe() {
    let work = tempfile::tempdir().unwrap();
    let result = run(work.path(), "compact: durable execution payload");
    completed(&result);
    assert_eq!(result.visible_ack, "C");
    assert!(result.execution_id.as_deref().unwrap().starts_with("cm://exec/"));
    let refs = result.execution_refs.as_ref().unwrap();
    for path in [["execution", ""], ["stored", "code"], ["stored", "telemetry"]] {
        let value = if path[1].is_empty() { &refs[path[0]] } else { &refs[path[0]][path[1]] };
        let prefix = if path[0] == "execution" { "tz://codemode/execution/" } else { "tz://" };
        assert!(value.as_str().unwrap().starts_with(prefix));
    }
    assert_eq!(result.telemetry.extra.as_ref().and_then(|x| x.get("raw_leak")), None);
}

#[test]
fn logical_execution_refs_expand_directly() {
    let work = tempfile::tempdir().unwrap();
    let result = run(work.path(), "compact: logical ref payload");
    completed(&result);
    let refs = result.execution_refs.as_ref().unwrap();
    let engine = make_engine_for_root(work.path().to_path_buf());
    for (key, needle) in [("code", "logical ref payload"), ("telemetry", "logical_ops"), ("execution", "cm://exec/")] {
        let expanded = engine.expand(refs[key].as_str().unwrap(), None, None, None, None, None);
        assert_eq!(expanded.status, "ok");
        let text = expanded.visible.unwrap().text;
        assert!(text.contains(needle), "expanded {key} ref contained: {text:?}");
    }
}

#[test]
fn json_dag_compacts_then_expands_by_binding() {
    let work = tempfile::tempdir().unwrap();
    let plan = json!({"steps": [
        {"id": "c", "method": "zero.token.compact", "args": ["json dag payload"]},
        {"id": "e", "method": "zero.token.expand", "args": ["$c.ref"]}
    ], "return": {"roundtrip": "$e.text", "ref": "$c.ref"}}).to_string();
    let result = run(work.path(), &plan);
    let value = completed(&result);
    assert!(value["roundtrip"].as_str().unwrap().contains("json dag payload"));
    assert!(value["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(result.telemetry.steps_run, Some(2));
}

#[test]
fn sandboxed_js_function_runs_against_token_namespace() {
    let work = tempfile::tempdir().unwrap();
    let result = run(work.path(), r#"
        export default function ({ token }) {
            const c = await token.compact("sandbox js payload");
            return { ref: c.ref, text: c.text };
        }
    "#);
    assert!(completed(&result)["ref"].as_str().unwrap().starts_with("tz://"));
}

#[test]
fn quickjs_executes_real_js_syntax_and_drains_promises() {
    let work = tempfile::tempdir().unwrap();
    let result = run(work.path(), r#"
        const payloads = ["a", "b", "c"].map((x, i) => String(i) + ":" + x);
        const stored = await zero.token.compactMany(payloads);
        const echoed = await Promise.resolve(stored.items.map((item) => item.text).join("|"));
        return { echoed, count: stored.count };
    "#);
    let value = completed(&result);
    assert_eq!(value["echoed"].as_str(), Some("0:a|1:b|2:c"));
    assert_eq!(value["count"].as_u64(), Some(3));
    assert_eq!(result.telemetry.steps_run, Some(1));
}

#[test]
fn quickjs_enforces_microtask_cap() {
    let result = execute_codemode_with_options("return await Promise.resolve(1)", CodeModeOptions { max_microtasks: 0, ..Default::default() });
    assert_eq!(result.status, CodeModeStatus::Error);
    assert!(result.error.as_ref().unwrap().message.contains("microtask cap"), "unexpected error: {:?}", result.error);
}

#[test]
fn sandbox_denies_network_and_process_capabilities() {
    for plan in ["await fetch('https://example.com')", "process.env", "require('fs')", "setTimeout(() => 1, 1)", "const f = () => zero.edit('file.txt', []); return f();", "store.put('x')", "db.query('select 1')", "indexedDB.open('x')"] {
        let result = execute_codemode_with_options(plan, CodeModeOptions::default());
        assert_eq!(result.status, CodeModeStatus::Error, "plan should fail: {plan}");
        assert_eq!(result.visible_ack, "X0");
        assert!(result.error.as_ref().unwrap().message.contains("sandbox"), "unexpected error: {:?}", result.error);
        assert!(result.execution_refs.is_some());
    }
}

#[test]
fn batch_compact_many_reports_coalesced_telemetry() {
    let work = tempfile::tempdir().unwrap();
    let payloads: Vec<String> = (0..100).map(|i| format!("payload-{i}")).collect();
    let plan = json!({"steps": [{"id": "many", "method": "zero.token.compactMANY", "args": [payloads]}], "return": "$many.count"}).to_string();
    let result = run(work.path(), &plan);
    assert_eq!(completed(&result).as_u64(), Some(100));
    assert_eq!((result.telemetry.logical_ops, result.telemetry.physical_ops), (1, 1));
    assert_eq!(result.visible_ack, "C");
}

#[test]
fn output_guard_keeps_large_result_behind_refs() {
    let result = execute_codemode_with_options("return \"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\"", CodeModeOptions { max_output_bytes: 8, ..Default::default() });
    assert_eq!(completed(&result)["truncated"].as_bool(), Some(true));
    assert_eq!(result.telemetry.extra.as_ref().and_then(|x| x.get("raw_leak")), None);
    assert!(result.execution_refs.is_some());
}
