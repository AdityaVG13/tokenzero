use super::exec::{
    exec_edit, execute_codemode, execute_codemode_with_options, make_engine_for_root,
};
use super::parser::{Statement, parse_expr, parse_plan, resolve_expr};
use super::result::{CodeModeOptions, CodeModeResult, CodeModeStatus};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokenzero_core::Mode;

fn execute_plan_in_token(plan: &str) -> String {
    execute_codemode(plan).to_line()
}

#[test]
fn undefined_variable_in_return_is_plan_error() {
    let result = execute_codemode("return missing_binding");
    assert_eq!(result.status, CodeModeStatus::Error);
    assert!(
        result
            .error
            .as_deref()
            .unwrap()
            .contains("undefined variable: missing_binding")
    );
}

#[test]
fn read_honors_start_and_end_line_options() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("lines.txt");
    let content: String = (1..=20).map(|line| format!("LINE_{line}\n")).collect();
    fs::write(&path, content).unwrap();

    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let plan = format!("await zero.read({quoted}, {{ start_line: 2, end_line: 3 }})");
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
    let text = result
        .value
        .as_ref()
        .and_then(|value| value.get("text"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(text.contains("LINE_2"), "expected bounded read: {text}");
    assert!(text.contains("LINE_3"), "expected bounded read: {text}");
    assert!(
        !text.contains("LINE_1"),
        "start_line should exclude earlier lines: {text}"
    );
    assert!(
        !text.contains("LINE_4"),
        "end_line should exclude later lines: {text}"
    );
}

#[test]
fn pathless_tree_uses_configured_root_not_cwd() {
    let work = tempfile::tempdir().unwrap();
    fs::write(work.path().join("marker.txt"), "present\n").unwrap();
    let cwd = std::env::current_dir().unwrap();
    assert_ne!(work.path(), cwd.as_path());

    let result = execute_codemode_with_options(
        "await zero.tree()",
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
    let text = result
        .value
        .as_ref()
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        text.contains("marker.txt"),
        "tree should search configured --root, not process cwd: {text}"
    );
}

#[test]
fn parser_rejects_lone_quote_without_panicking() {
    let err = parse_expr("\"").unwrap_err();
    assert!(err.contains("unterminated string literal"));
    assert!(parse_plan("return \"").is_err());
}

#[test]
fn parser_object_numeric_options_resolve_as_u64() {
    let scope = HashMap::new();
    let expr = parse_expr("{ start_line: 1, end_line: 10, max_files: 5 }").unwrap();
    let value = resolve_expr(&expr, &scope).unwrap();
    let obj = value.as_object().unwrap();
    assert_eq!(obj.get("start_line").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(obj.get("end_line").and_then(|v| v.as_u64()), Some(10));
    assert_eq!(obj.get("max_files").and_then(|v| v.as_u64()), Some(5));
}

#[test]
fn parser_rejects_non_finite_number_literals() {
    assert!(parse_expr("inf").is_err());
    assert!(parse_expr("nan").is_err());
}

#[test]
fn empty_plan_returns_error() {
    let r = execute_codemode("");
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.as_ref().unwrap().contains("empty"));
}

#[test]
fn search_returns_ranked_methods() {
    let r = execute_codemode("search:read");
    assert_eq!(r.status, CodeModeStatus::Completed);
    let results = r.value.unwrap();
    let hits = results["results"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["path"].as_str().unwrap().contains("read"));
}

#[test]
fn describe_returns_signature() {
    let r = execute_codemode("describe:zero.read");
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert!(val["types"].as_str().unwrap().contains("Promise"));
}

#[test]
fn describe_unknown_returns_available_list() {
    let r = execute_codemode("describe:zero.nonexistent");
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert!(val["error"].is_string());
    assert!(val["available"].as_array().unwrap().len() > 5);
}

#[test]
fn unknown_method_gives_helpful_error() {
    let r = execute_codemode("await zero.banana()");
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.as_ref().unwrap().contains("unknown method"));
    assert!(r.error.as_ref().unwrap().contains("codemode.search"));
}

#[test]
fn parser_handles_binding_and_return() {
    let stmts = parse_plan(r#"const x = await zero.compact("hello"); return x.ref"#).unwrap();
    assert_eq!(stmts.len(), 2);
    assert!(matches!(&stmts[0], Statement::Binding { name, .. } if name == "x"));
    assert!(matches!(&stmts[1], Statement::Return(..)));
}

#[test]
fn parser_splits_multiline_plan() {
    let plan = "const a = await zero.shell(\"ls\");\nconst b = await zero.shell(\"pwd\");\nreturn { a, b }";
    let stmts = parse_plan(plan).unwrap();
    assert_eq!(stmts.len(), 3);
}

#[test]
fn parser_handles_object_args() {
    let stmts =
        parse_plan(r#"zero.read("src/main.rs", { mode: "auto", start_line: 1, end_line: 10 })"#)
            .unwrap();
    assert_eq!(stmts.len(), 1);
    if let Statement::Call(call) = &stmts[0] {
        assert_eq!(call.method, "zero.read");
        assert_eq!(call.args.len(), 2);
    } else {
        panic!("expected Call");
    }
}

#[test]
fn compact_roundtrip_through_codemode() {
    let r = execute_codemode(r#"await zero.compact("test payload for codemode")"#);
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.as_ref().unwrap();
    let ref_id = val["ref"].as_str().unwrap();
    assert!(ref_id.starts_with("tz://"));

    let expand_plan = format!(r#"await zero.expand("{ref_id}")"#);
    let r2 = execute_codemode(&expand_plan);
    assert_eq!(r2.status, CodeModeStatus::Completed);
    let val2 = r2.value.as_ref().unwrap();
    let text = val2["text"].as_str().unwrap_or("");
    assert!(text.contains("test payload for codemode"));
}

#[test]
fn token_namespace_compact_roundtrip_through_codemode() {
    let r = execute_codemode(r#"await zero.token.compact("token namespace payload")"#);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.as_ref().unwrap();
    let ref_id = val["ref"].as_str().unwrap();
    assert!(ref_id.starts_with("tz://"));

    let expand_plan = format!(r#"await zero.token.expand("{ref_id}")"#);
    let r2 = execute_codemode(&expand_plan);
    assert_eq!(r2.status, CodeModeStatus::Completed, "{:?}", r2.error);
    let val2 = r2.value.as_ref().unwrap();
    let text = val2["text"].as_str().unwrap_or("");
    assert!(text.contains("token namespace payload"));
}

#[test]
fn describe_token_namespace_returns_signature() {
    let r = execute_codemode("describe:zero.token.compact");
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert_eq!(val["path"], "zero.token.compact");
    assert!(
        val["types"]
            .as_str()
            .unwrap()
            .contains("zero.token.compact")
    );
}

#[test]
fn codemode_engine_uses_codemode_recovery_cache_and_repo_scope() {
    let root = PathBuf::from("/tmp/tokenzero-codemode-root");
    let engine = make_engine_for_root(root.clone());
    assert_eq!(engine.config.allowed_roots, vec![root.clone()]);
    assert_eq!(
        engine.config.cache_path,
        crate::workspace::default_codemode_recovery_cache_path(&root)
    );
    assert!(
        engine
            .config
            .cache_path
            .to_string_lossy()
            .contains("codemode-recovery.json")
    );
}

#[test]
fn edit_rejects_partially_invalid_hunks_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.txt");
    fs::write(&path, "hello\n").unwrap();
    let engine = make_engine_for_root(dir.path().to_path_buf());
    let args = vec![
        serde_json::json!(path.to_string_lossy().to_string()),
        serde_json::json!([
            {"find": "hello", "replace": "bye"},
            {"find": "hello"}
        ]),
    ];

    let err = exec_edit(&engine, &args).unwrap_err();
    assert!(
        err.error
            .as_deref()
            .unwrap()
            .contains("invalid hunk at index 1"),
        "{:?}",
        err.error
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
}

#[test]
fn edit_reports_zero_hunks_applied_on_engine_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.txt");
    fs::write(&path, "hello\n").unwrap();
    let engine = make_engine_for_root(dir.path().to_path_buf());
    let args = vec![
        serde_json::json!(path.to_string_lossy().to_string()),
        serde_json::json!([{ "find": "missing", "replace": "bye" }]),
    ];

    let outcome = exec_edit(&engine, &args).unwrap();
    assert_eq!(outcome.as_value()["status"], "error");
    assert_eq!(outcome.as_value()["hunks_applied"], 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
}

#[test]
fn shell_plan_captures_exit_code() {
    let r = execute_codemode(r#"await zero.shell("echo hello")"#);
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert!(
        val["status"].is_string(),
        "should complete without panic: {:?}",
        val
    );
}

#[test]
fn multi_statement_composition() {
    let plan = r#"
        const data = await zero.compact("composed payload");
        const expanded = await zero.expand(data.ref);
        return { ref: data.ref, found: expanded.text }
    "#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
}

#[test]
fn telemetry_line_format_is_stable() {
    let r = execute_codemode(r#"await zero.compact("line test")"#);
    let line = r.to_line();
    assert!(line.starts_with("codemode:ok"));
    assert!(line.contains("ops="));
    assert!(line.contains("visible_tokens="));
    assert!(line.contains("raw_tokens="));
}

#[test]
fn expand_invalid_ref_returns_error_not_panic() {
    let r = execute_codemode(r#"await zero.expand("tz://blob/nonexistent123")"#);
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert!(
        val["status"].is_string(),
        "should complete without panic: {:?}",
        val
    );
}

#[test]
fn expand_without_tz_prefix_is_rejected() {
    let r = execute_codemode(r#"await zero.expand("not-a-ref")"#);
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.as_ref().unwrap().contains("tz://"));
}

#[test]
fn recall_method_is_discoverable_and_dispatchable() {
    let r = execute_codemode("describe:zero.recall");
    assert_eq!(r.status, CodeModeStatus::Completed);
    assert!(
        r.value.as_ref().unwrap()["types"]
            .as_str()
            .unwrap()
            .contains("zero.recall")
    );
    let search = execute_codemode("search:recall");
    assert!(
        search.value.as_ref().unwrap()["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hit| hit["path"] == "zero.recall")
    );
}

#[test]
fn codemode_method_catalog_resource_shape() {
    let catalog = crate::codemode::catalog::codemode_method_catalog();
    assert_eq!(catalog["schema_version"], "tokenzero.codemode.catalog.v1");
    assert!(
        catalog["methods"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["path"] == "zero.recall")
    );
}

#[test]
fn search_all_methods_discoverable() {
    let r = execute_codemode("search:zero");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    assert!(results.len() >= 10, "catalog should expose all ops");
}

#[test]
fn legacy_line_api_still_works() {
    let line = execute_plan_in_token(r#"await zero.compact("legacy")"#);
    assert!(line.starts_with("codemode:ok"));
}
