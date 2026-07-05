use super::exec::{
    exec_edit, execute_codemode, execute_codemode_with_options, make_engine_for_root,
};
use super::parser::{Statement, parse_expr, parse_plan, resolve_expr};
use super::result::{CodeModeOptions, CodeModeResult, CodeModeStatus};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokenzero_core::Mode;

#[test]
fn codemode_v2_one_token_answer_stays_tiny_on_wire() {
    let work = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig {
        allowed_roots: vec![work.path().to_path_buf()],
        cache_path: super::exec::make_engine_for_root(work.path().to_path_buf())
            .config
            .cache_path,
        ..crate::EngineConfig::for_root(work.path())
    });
    let response = crate::call_tool_fastmcp(
        &engine,
        "execute_code",
        &serde_json::json!({"plan": "return await Promise.resolve(1)"}),
        None,
    )
    .unwrap();
    let text = response
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap();
    assert!(text.starts_with("ok tz0 "), "v2 ack was {text:?}");
    assert!(response.get("structuredContent").is_none());
    assert!(text.contains(" =1 t:"), "v2 ack was {text:?}");
    let visible_tokens = tokenzero_core::count_tokens(text);
    assert!(
        visible_tokens <= 14,
        "ack should fit the v2 visible budget, got {visible_tokens}: {text}"
    );
}

#[test]
fn ref_first_large_final_string_recovers_byte_exactly() {
    let work = tempfile::tempdir().unwrap();
    let payload = "x ".repeat(2500);
    let plan = format!("return {}", serde_json::to_string(&payload).unwrap());
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
    let ref_id = value["ref"]
        .as_str()
        .expect("large string should be ref-first");
    assert!(ref_id.starts_with("tz://"));
    assert_eq!(
        value["preview"].as_str(),
        Some("x x x x x x x x x x x x x x x x ")
    );

    let engine = make_engine_for_root(work.path().to_path_buf());
    let expanded = engine.expand(ref_id, None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.unwrap().text, payload);
}

#[test]
fn v1_envelope_escape_hatch_keeps_legacy_payload_shape() {
    let work = tempfile::tempdir().unwrap();
    let engine = crate::TokenZeroEngine::new(crate::EngineConfig {
        allowed_roots: vec![work.path().to_path_buf()],
        cache_path: super::exec::make_engine_for_root(work.path().to_path_buf())
            .config
            .cache_path,
        ..crate::EngineConfig::for_root(work.path())
    });
    let response = crate::call_tool_fastmcp(
        &engine,
        "execute_code",
        &serde_json::json!({"plan": "return await Promise.resolve(1)", "envelope": "v1"}),
        None,
    )
    .unwrap();
    assert!(response.get("structuredContent").is_none());
    let text = response
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap();
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["ack"], "C");
    assert_eq!(payload["value"], 1);
    assert!(payload.get("telemetry").is_some());
}

#[test]
fn quiet_combinators_handle_edge_cases_compactly() {
    let result = execute_codemode_with_options(
        r#"
        const empty_count = zero.count("");
        const array_count = zero.count([1, 2, 3]);
        const missing_first = zero.first([]);
        const first_two = zero.first("a\nb\nc", 2);
        const verdict = zero.verdict(false, "bad\nverbose");
        return { empty_count, array_count, missing_first, first_two, verdict };
        "#,
        CodeModeOptions::default(),
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert_eq!(value["empty_count"].as_u64(), Some(0));
    assert_eq!(value["array_count"].as_u64(), Some(3));
    assert!(value["missing_first"].is_null());
    assert_eq!(value["first_two"].as_str(), Some("a\nb"));
    assert_eq!(value["verdict"]["ok"].as_bool(), Some(false));
    assert_eq!(value["verdict"]["detail"].as_str(), Some("bad"));
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
fn partial_limits_objects_deserialize_with_defaults() {
    // Tool callers send PARTIAL limits (the documented contract:
    // {"max_output_bytes": 1024}). Plain derive made every field required,
    // so tools.rs's `if let Ok` silently DROPPED the caller's limits — a
    // silent-failure regression caught in PR 16 review. serde(default)
    // restores the contract: given fields apply, missing fields default.
    let limits: crate::CodeModeLimits =
        serde_json::from_value(serde_json::json!({ "max_output_bytes": 1024 }))
            .expect("partial limits object MUST deserialize");
    assert_eq!(limits.max_output_bytes, 1024);
    assert_eq!(
        limits.max_logical_ops,
        crate::CodeModeLimits::default().max_logical_ops,
        "missing fields take defaults"
    );
    // Empty object = all defaults (the degenerate partial).
    let empty: crate::CodeModeLimits = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(
        empty.max_code_bytes,
        crate::CodeModeLimits::default().max_code_bytes
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
    assert!(val["signature"].as_str().unwrap().contains("Promise"));
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
        val["signature"]
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
        r.value.as_ref().unwrap()["signature"]
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

// ─── Composition engine tests ───────────────────────────────────────────────

#[test]
fn pipe_sequential_composition() {
    let plan = r#"await zero.pipe([{"method": "zero.compact", "args": ["step one"]}, {"method": "zero.compact", "args": ["step two"]}], {"raw": true})"#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert_eq!(val["steps"], 2);
    let results = val["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0]["ref"].as_str().unwrap().starts_with("tz://"));
    assert!(results[1]["ref"].as_str().unwrap().starts_with("tz://"));
}

#[test]
fn pipe_empty_steps_rejected() {
    let r = execute_codemode(r#"await zero.pipe([])"#);
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.as_ref().unwrap().contains("at least one step"));
}

#[test]
fn pick_extracts_keys_from_result() {
    let plan = r#"
        const data = await zero.compact("payload for pick test");
        const picked = await zero.pick(data, ["ref", "status"]);
        return picked
    "#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(val["status"], "ok");
    assert!(val.get("text").is_none(), "text should be excluded by pick");
}

#[test]
fn filter_lines_narrows_text() {
    let plan = r#"
        const data = await zero.compact("alpha line\nbeta match\ngamma line\ndelta match");
        const expanded = await zero.expand(data.ref);
        const filtered = await zero.filter_lines(expanded, "match");
        return filtered
    "#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert_eq!(val["lines"], 2);
    let text = val["text"].as_str().unwrap();
    assert!(text.contains("beta match"));
    assert!(text.contains("delta match"));
    assert!(!text.contains("alpha"));
}

#[test]
fn telemetry_reports_equivalent_calls() {
    let plan = r#"
        const a = await zero.compact("first");
        const b = await zero.compact("second");
        const c = await zero.expand(a.ref);
        return { a_ref: a.ref, b_ref: b.ref, c_text: c.text }
    "#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    assert_eq!(r.telemetry.operations, 3);
    assert_eq!(r.telemetry.equivalent_calls, Some(4));
}

#[test]
fn multi_step_dataflow_with_intermediate_binding() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("data.txt");
    fs::write(&path, "line1 important\nline2 noise\nline3 important\n").unwrap();

    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let plan = format!(
        r#"const content = await zero.read({quoted});
        const filtered = await zero.filter_lines(content, "important");
        return {{ lines: filtered.lines, text: filtered.text }}"#
    );
    let r = execute_codemode_with_options(
        &plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            ..Default::default()
        },
    );
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert_eq!(val["lines"], 2);
    assert!(val["text"].as_str().unwrap().contains("important"));
    assert!(!val["text"].as_str().unwrap().contains("noise"));
    assert_eq!(r.telemetry.operations, 2);
    assert_eq!(r.telemetry.equivalent_calls, Some(3));
}

#[test]
fn pipe_and_pick_composition() {
    let plan = r#"
        const piped = await zero.pipe([{"method": "zero.compact", "args": ["piped data"]}], {"raw": true});
        const picked = await zero.pick(piped, ["steps", "last"]);
        return picked
    "#;
    let r = execute_codemode(plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert_eq!(val["steps"], 1);
    assert!(val["last"].is_object());
}

#[test]
fn new_composition_methods_discoverable() {
    let r = execute_codemode("search:pipe");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    assert!(results.iter().any(|hit| hit["path"] == "zero.pipe"));

    let r = execute_codemode("search:pick");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    assert!(results.iter().any(|hit| hit["path"] == "zero.pick"));

    let r = execute_codemode("search:filter");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    assert!(results.iter().any(|hit| hit["path"] == "zero.filter_lines"));
}

// --- Recovery-aware compression tests ---

#[test]
fn compact_content_aware_produces_ref_and_savings() {
    let large_code = (0..200)
        .map(|i| format!("pub fn handler_{i}(ctx: &Context, request: Request<Body>) -> Result<Response<Body>, Error> {{ log::info!(\"handling request {i}\"); Ok(Response::new(Body::empty())) }}"))
        .collect::<Vec<_>>()
        .join("\n");
    let plan = format!(
        r#"await zero.compact({})"#,
        serde_json::to_string(&large_code).unwrap()
    );
    let r = execute_codemode(&plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(val["compression_strategy"], "content_aware");
    assert!(val["visible_tokens"].as_u64().unwrap() < val["raw_tokens"].as_u64().unwrap());
}

#[test]
fn compact_max_aggressive_compression_with_recovery() {
    let large_logs = (0..200)
        .map(|i| {
            if i % 20 == 0 {
                format!("ERROR: something failed at step {i}")
            } else {
                format!("INFO: processing item {i} successfully")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let plan = format!(
        r#"await zero.compact_max({})"#,
        serde_json::to_string(&large_logs).unwrap()
    );
    let r = execute_codemode(&plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    let ref_id = val["ref"].as_str().unwrap();
    assert!(ref_id.starts_with("tz://"));
    assert_eq!(val["compression_strategy"], "content_aware_max");
    // Aggressive should compress significantly more
    let vis = val["visible_tokens"].as_u64().unwrap();
    let raw = val["raw_tokens"].as_u64().unwrap();
    assert!(
        vis < raw / 2,
        "aggressive should save >50%: vis={vis} raw={raw}"
    );
}

#[test]
fn compact_max_roundtrip_recovery_is_byte_exact() {
    let payload = "exact recovery test: special chars !@#$%^&*()\nnewlines\ttabs\n";
    let plan = format!(
        r#"const c = await zero.compact_max({}); const e = await zero.expand(c.ref); return {{ original_ref: c.ref, recovered: e.text }}"#,
        serde_json::to_string(payload).unwrap()
    );
    let r = execute_codemode(&plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    let recovered = val["recovered"].as_str().unwrap();
    assert_eq!(recovered, payload, "recovery must be byte-exact");
}

#[test]
fn content_aware_logs_prioritizes_errors() {
    let logs = (0..500)
        .map(|i| {
            if i == 42 {
                "FATAL: database connection lost at 2024-01-15T10:30:00Z host=prod-db-1 stack=main".to_string()
            } else if i == 77 {
                "ERROR: timeout exceeded after 30s waiting for upstream response from gateway".to_string()
            } else {
                format!("DEBUG: routine operation {i} completed successfully in 2ms status=200 bytes=1024")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let plan = format!(
        r#"await zero.compact_max({})"#,
        serde_json::to_string(&logs).unwrap()
    );
    let r = execute_codemode(&plan);
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
    let val = r.value.unwrap();
    let text = val["text"].as_str().unwrap();
    // Content-aware should surface errors in visible output
    assert!(
        text.contains("FATAL") || text.contains("ERROR"),
        "content-aware compression should surface errors: {text}"
    );
}

#[test]
fn compact_max_discoverable() {
    let r = execute_codemode("search:compact_max");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    assert!(results.iter().any(|hit| hit["path"] == "zero.compact_max"));
}

// --- Plan vs direct execution parity tests ---

#[test]
fn parity_read_plan_vs_direct_identical_output() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("test.txt");
    fs::write(&path, "line one\nline two\nline three\n").unwrap();
    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let opts = CodeModeOptions {
        root: Some(work.path().to_path_buf()),
        ..Default::default()
    };

    // Direct single-call
    let direct =
        execute_codemode_with_options(&format!(r#"await zero.read({quoted})"#), opts.clone());
    // Plan with binding
    let plan = execute_codemode_with_options(
        &format!(r#"const r = await zero.read({quoted}); return r"#),
        opts.clone(),
    );

    assert_eq!(direct.status, CodeModeStatus::Completed);
    assert_eq!(plan.status, CodeModeStatus::Completed);
    let d_val = direct.value.unwrap();
    let p_val = plan.value.unwrap();
    // Same text content
    assert_eq!(d_val["text"], p_val["text"], "read text must be identical");
    // Same ref
    assert_eq!(d_val["ref"], p_val["ref"], "recovery ref must be identical");
    // Same token accounting
    assert_eq!(d_val["visible_tokens"], p_val["visible_tokens"]);
    assert_eq!(d_val["raw_tokens"], p_val["raw_tokens"]);
}

#[test]
fn parity_grep_plan_vs_direct_identical_matches() {
    // Parity is about MARSHALING (statement form vs plan form), not session
    // economics: running both against one shared store makes the second
    // serve a legitimate seen-set dedup and the comparison meaningless.
    // Isolate each form in its own root with identical content and compare
    // modulo the root path.
    let run = |plan_tpl: &str| {
        let work = tempfile::tempdir().unwrap();
        fs::write(
            work.path().join("code.rs"),
            "fn main() {}\nfn helper() {}\nstruct Foo;\n",
        )
        .unwrap();
        let dir = serde_json::to_string(work.path().to_str().unwrap()).unwrap();
        let opts = CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            ..Default::default()
        };
        let result = execute_codemode_with_options(&plan_tpl.replace("{dir}", &dir), opts);
        assert_eq!(
            result.status,
            CodeModeStatus::Completed,
            "{:?}",
            result.error
        );
        let val = result.value.unwrap();
        let root = work.path().to_str().unwrap().to_string();
        (val, root)
    };

    let (d_val, d_root) = run(r#"await zero.grep("fn", {dir})"#);
    let (p_val, p_root) = run(r#"const g = await zero.grep("fn", {dir}); return g"#);

    let normalize = |val: &serde_json::Value, root: &str| {
        val["text"]
            .as_str()
            .unwrap_or_default()
            .replace(root, "<ROOT>")
    };
    assert_eq!(
        normalize(&d_val, &d_root),
        normalize(&p_val, &p_root),
        "grep results must be identical modulo root path"
    );
}

#[test]
fn parity_shell_plan_vs_direct_identical_capture() {
    let opts = CodeModeOptions::default();

    let direct =
        execute_codemode_with_options(r#"await zero.shell("echo hello world")"#, opts.clone());
    let plan = execute_codemode_with_options(
        r#"const s = await zero.shell("echo hello world"); return s"#,
        opts.clone(),
    );

    assert_eq!(direct.status, CodeModeStatus::Completed);
    assert_eq!(plan.status, CodeModeStatus::Completed);
    let d_val = direct.value.unwrap();
    let p_val = plan.value.unwrap();
    assert_eq!(
        d_val["text"], p_val["text"],
        "shell output must be identical"
    );
    assert_eq!(d_val["exit_code"], p_val["exit_code"]);
    assert_eq!(d_val["success"], p_val["success"]);
}

#[test]
fn parity_edit_plan_vs_direct_identical_result() {
    let work = tempfile::tempdir().unwrap();
    let path1 = work.path().join("a.txt");
    let path2 = work.path().join("b.txt");
    fs::write(&path1, "hello world").unwrap();
    fs::write(&path2, "hello world").unwrap();
    let opts = CodeModeOptions {
        root: Some(work.path().to_path_buf()),
        ..Default::default()
    };

    let q1 = serde_json::to_string(path1.to_str().unwrap()).unwrap();
    let q2 = serde_json::to_string(path2.to_str().unwrap()).unwrap();

    let direct = execute_codemode_with_options(
        &format!(r#"await zero.edit({q1}, [{{ "find": "hello", "replace": "goodbye" }}])"#),
        opts.clone(),
    );
    let plan = execute_codemode_with_options(
        &format!(
            r#"const e = await zero.edit({q2}, [{{ "find": "hello", "replace": "goodbye" }}]); return e"#
        ),
        opts.clone(),
    );

    assert_eq!(direct.status, CodeModeStatus::Error);
    assert_eq!(plan.status, CodeModeStatus::Error);
    assert_eq!(direct.error.as_ref().unwrap().kind, "policy");
    assert_eq!(plan.error.as_ref().unwrap().kind, "policy");
    assert_eq!(fs::read_to_string(&path1).unwrap(), "hello world");
    assert_eq!(fs::read_to_string(&path2).unwrap(), "hello world");
}

// --- New helper tests ---

#[test]
fn count_tokens_returns_metrics() {
    let r = execute_codemode(r#"await zero.count_tokens("hello world this is a test")"#);
    assert_eq!(r.status, CodeModeStatus::Completed);
    let val = r.value.unwrap();
    assert!(val["tokens"].as_u64().unwrap() > 0);
    assert_eq!(val["bytes"].as_u64().unwrap(), 26);
    assert_eq!(val["lines"].as_u64().unwrap(), 1);
}

#[test]
fn assert_passes_on_truthy() {
    let r = execute_codemode(r#"await zero.assert(true, "should pass")"#);
    assert_eq!(r.status, CodeModeStatus::Completed);
    assert_eq!(r.value.unwrap()["ok"], true);
}

#[test]
fn assert_fails_on_falsy_with_message() {
    let r = execute_codemode(r#"await zero.assert(false, "expected failure")"#);
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.unwrap().contains("expected failure"));
}

#[test]
fn search_includes_signatures_and_examples() {
    let r = execute_codemode("search:read");
    let val = r.value.unwrap();
    let results = val["results"].as_array().unwrap();
    let hit = results.iter().find(|h| h["path"] == "zero.read").unwrap();
    assert!(hit["signature"].as_str().unwrap().contains("path: string"));
    assert!(hit["example"].as_str().unwrap().contains("await"));
}

#[test]
fn describe_includes_related_methods() {
    let r = execute_codemode("describe:zero.read");
    let val = r.value.unwrap();
    let related = val["related"].as_array().unwrap();
    assert!(!related.is_empty());
    assert!(related.iter().any(|r| r.as_str() == Some("zero.expand")));
}

#[test]
fn denied_token_guard_requires_identifier_boundary() {
    use super::sandbox::lower_code_plan;
    use super::store::CodeModeLimits;
    let limits = CodeModeLimits::default();

    for plan in [
        "const f = await zero.read(\"a.txt\"); return f.refs.length",
        "const r = await zero.shell(\"ls\"); return r.refs.stdout",
        "const subprocess_count = 1; return subprocess_count",
        "const respawned = true; return respawned",
        "const restored = await zero.expand(\"tz://blob/x\"); return restored",
    ] {
        assert!(
            lower_code_plan(plan, &limits).is_ok(),
            "false positive for plan: {plan}"
        );
    }

    for (plan, token) in [
        ("return fs.readFileSync(\"/etc/passwd\")", "fs."),
        ("const x = a.fs.read()", "fs."),
        ("return process.env.HOME", "process"),
        ("spawn(\"sh\")", "spawn"),
        ("return db.query(\"x\")", "db."),
    ] {
        let err = lower_code_plan(plan, &limits).expect_err(plan);
        assert!(err.contains(token), "expected {token} denial, got: {err}");
    }
}

#[test]
fn alias_rewrites_skip_string_literals_and_identifier_tails() {
    use super::sandbox::lower_code_plan;
    use super::store::CodeModeLimits;
    let limits = CodeModeLimits::default();

    let lowered = lower_code_plan(
        "const p = \"/tmp/fab-api.txt\"; const q = 'ctx.ref in a string'; return p",
        &limits,
    )
    .unwrap();
    assert!(
        lowered.contains("/tmp/fab-api.txt"),
        "string literal corrupted: {lowered}"
    );
    assert!(
        lowered.contains("ctx.ref in a string"),
        "string literal corrupted: {lowered}"
    );

    let lowered = lower_code_plan("const myapi = 1; return myapi.foo", &limits).unwrap();
    assert!(
        lowered.contains("myapi.foo"),
        "identifier tail corrupted: {lowered}"
    );

    let lowered = lower_code_plan("return api.read(\"a.txt\")", &limits).unwrap();
    assert!(
        lowered.contains("zero.read("),
        "api alias not rewritten: {lowered}"
    );

    let lowered = lower_code_plan("return token.compact(x)", &limits).unwrap();
    assert!(
        lowered.contains("zero.token.compact(x)"),
        "token alias not rewritten: {lowered}"
    );

    let lowered = lower_code_plan("return zero.token.compact(x)", &limits).unwrap();
    assert!(
        lowered.contains("zero.token.compact(x)") && !lowered.contains("zero.zero."),
        "double prefix: {lowered}"
    );
}
