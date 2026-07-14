use super::exec::{
    exec_edit, execute_codemode, execute_codemode_with_options, limits_from_options,
    make_engine_for_root, resolve_paths_against_work_root,
};
use super::parser::{Statement, parse_expr, parse_plan, resolve_expr};
use super::result::{CodeModeOptions, CodeModeResult, CodeModeStatus};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tokenzero_core::Mode;

fn engine_for(root: &std::path::Path) -> crate::TokenZeroEngine {
    crate::TokenZeroEngine::new(crate::EngineConfig { allowed_roots: vec![root.to_path_buf()], cache_path: make_engine_for_root(root.to_path_buf()).config.cache_path, ..crate::EngineConfig::for_root(root) })
}

fn opts_for(root: &std::path::Path) -> CodeModeOptions {
    CodeModeOptions { root: Some(root.to_path_buf()), ..Default::default() }
}

fn run_plan(plan: &str) -> CodeModeResult {
    execute_codemode(plan)
}

fn run_at(root: &std::path::Path, plan: &str) -> CodeModeResult {
    execute_codemode_with_options(plan, opts_for(root))
}

fn assert_completed(r: &CodeModeResult) {
    assert_eq!(r.status, CodeModeStatus::Completed, "{:?}", r.error);
}

fn assert_error_contains(r: &CodeModeResult, needle: &str) {
    assert_eq!(r.status, CodeModeStatus::Error);
    assert!(r.error.as_ref().unwrap().contains(needle), "{:?}", r.error);
}

fn assert_search_hit(query: &str, path: &str) {
    let r = run_plan(&format!("search:{query}"));
    assert_completed(&r);
    let hits = r.value.unwrap()["results"].as_array().unwrap().clone();
    assert!(hits.iter().any(|h| h["path"] == path), "{query} -> {path}: {hits:?}");
}

fn content_text(response: &serde_json::Value) -> &str {
    response.pointer("/content/0/text").and_then(serde_json::Value::as_str).unwrap()
}

fn assert_compact_roundtrip(compact: &str, expand_tpl: &str, needle: &str) {
    let r = run_plan(compact);
    assert_completed(&r);
    let ref_id = r.value.as_ref().unwrap()["ref"].as_str().unwrap();
    assert!(ref_id.starts_with("tz://"));
    let r2 = run_plan(&expand_tpl.replace("{ref_id}", ref_id));
    assert_completed(&r2);
    assert!(r2.value.as_ref().unwrap()["text"].as_str().unwrap_or("").contains(needle));
}

#[test]
fn codemode_v2_one_token_answer_stays_tiny_on_wire() {
    let work = tempfile::tempdir().unwrap();
    let engine = engine_for(work.path());
    let response = crate::call_tool_fastmcp(
        &engine,
        "execute_code",
        &serde_json::json!({"plan": "return await Promise.resolve(1)"}),
        None,
    )
    .unwrap();
    let text = content_text(&response);
    assert!(text.starts_with("ok tz0 "), "v2 ack was {text:?}");
    assert!(response.get("structuredContent").is_none());
    assert!(text.contains(" =1 t:"), "v2 ack was {text:?}");
    let visible_tokens = tokenzero_core::count_tokens(text);
    assert!(visible_tokens <= 14, "ack should fit the v2 visible budget, got {visible_tokens}: {text}");
}

#[cfg(not(windows))]
#[test]
fn fastmcp_shell_refs_are_role_labeled_not_anonymous_array() {
    let work = tempfile::tempdir().unwrap();
    fs::write(work.path().join("small.txt"), "tok ".repeat(200)).unwrap();
    let engine = engine_for(work.path());
    let cwd = serde_json::to_string(work.path().to_str().unwrap()).unwrap();
    let plan = format!(r#"return await zero.shell("cat small.txt", {{ cwd: {cwd} }})"#);
    let response = crate::call_tool_fastmcp(
        &engine,
        "execute_code",
        &serde_json::json!({"plan": plan}),
        None,
    )
    .unwrap();
    let value = response
        .pointer("/structuredContent/value")
        .expect("structuredContent.value");
    assert!(value["combined_ref"].as_str().unwrap().starts_with("tz://"));
    assert!(value["capture_ref"].as_str().unwrap().starts_with("tz://"));
    assert!(response.pointer("/structuredContent/refs").is_none(), "shell refs must not be an anonymous refs array: {response}");
}

#[test]
fn ref_first_large_final_string_recovers_byte_exactly() {
    let work = tempfile::tempdir().unwrap();
    let payload = "x ".repeat(2500);
    let result = run_at(work.path(), &format!("return {}", serde_json::to_string(&payload).unwrap()));
    assert_completed(&result);
    let value = result.value.as_ref().unwrap();
    let ref_id = value["ref"].as_str().expect("large string should be ref-first");
    assert!(ref_id.starts_with("tz://"));
    assert_eq!(value["preview"].as_str(), Some("x x x x x x x x x x x x x x x x "));
    let expanded = make_engine_for_root(work.path().to_path_buf()).expand(ref_id, None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    assert_eq!(expanded.visible.unwrap().text, payload);
}

#[test]
fn explicit_expand_always_returns_exact_bytes_dda8627() {
    let work = tempfile::tempdir().unwrap();
    let payload = "deep".repeat(1250);
    assert_eq!(payload.len(), 5000);
    let quoted = serde_json::to_string(&payload).unwrap();
    let result = run_at(
        work.path(),
        &format!("const c = await zero.token.compact({quoted}); return await zero.token.expand(c.ref);"),
    );
    assert_completed(&result);
    assert_eq!(result.value.as_ref().unwrap().as_str(), Some(payload.as_str()), "expand must return exact bytes (dda8627)");
}

#[test]
fn budget_capped_expand_appends_windowing_hint() {
    let work = tempfile::tempdir().unwrap();
    let payload = "line\n".repeat(2000);
    let quoted = serde_json::to_string(&payload).unwrap();
    let plan = format!("const c = await zero.token.compact({quoted}); return await zero.token.expand(c.ref);");
    let result = execute_codemode_with_options(
        &plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            max_output_bytes: 900,
            ..Default::default()
        },
    );
    assert_completed(&result);
        let text = result.value.as_ref().unwrap().as_str().unwrap();
    assert!(text.contains("tokenzero expand truncated"), "missing truncation line: {text:?}");
    assert!(text.contains("start_line/end_line"), "missing windowing opts: {text:?}");
}

#[test]
fn shell_inline_threshold_keeps_refs_and_ref_wraps_large_text() {
    let work = tempfile::tempdir().unwrap();
    let (small, large) = ("tok ".repeat(200), "tok ".repeat(400));
    fs::write(work.path().join("small.txt"), &small).unwrap();
    fs::write(work.path().join("large.txt"), &large).unwrap();
    let cwd = serde_json::to_string(work.path().to_str().unwrap()).unwrap();
    let run = |file: &str| execute_codemode_with_options(&format!(r#"return await zero.shell("cat {file}", {{ cwd: {cwd} }})"#), opts_for(work.path()));
    let tz = |v: &serde_json::Value, key: &str| assert!(v[key].as_str().unwrap().starts_with("tz://"), "{key}: {v}");
    let small_result = run("small.txt");
    assert_completed(&small_result);
    let sv = small_result.value.as_ref().unwrap();
    assert!(sv["text"].as_str().expect("small shell text should inline").contains(small.trim_end()));
    tz(sv, "combined_ref"); tz(sv, "capture_ref");
    assert!(sv.get("refs").is_none(), "shell refs must be role-labeled fields: {sv}");
    let large_result = run("large.txt");
    assert_completed(&large_result);
    let lv = large_result.value.as_ref().unwrap();
    assert!(lv["text"]["ref"].as_str().unwrap().starts_with("tz://"));
    assert!(lv["text"]["preview"].as_str().is_some());
    tz(lv, "combined_ref");
    assert!(lv.get("refs").is_none(), "shell refs must be role-labeled fields: {lv}");
}

#[test]
fn ref_first_recurses_to_leaf_strings_not_whole_objects() {
    let result = run_plan(
        r#"return { status: "ok", text: "tok ".repeat(1500), nested: { keep: "shape" } }"#,
    );
    assert_completed(&result);
    let value = result.value.as_ref().unwrap();
    assert_eq!(value["status"].as_str(), Some("ok"));
    assert_eq!(value["nested"]["keep"].as_str(), Some("shape"));
    assert!(value["text"]["ref"].as_str().unwrap().starts_with("tz://"));
    assert!(value.get("ref").is_none(), "object itself must not be ref-wrapped: {value}");
}

#[test]
fn v1_envelope_escape_hatch_keeps_legacy_payload_shape() {
    let work = tempfile::tempdir().unwrap();
    let engine = engine_for(work.path());
    let response = crate::call_tool_fastmcp(
        &engine,
        "execute_code",
        &serde_json::json!({"plan": "return await Promise.resolve(1)", "envelope": "v1"}),
        None,
    )
    .unwrap();
    assert!(response.get("structuredContent").is_none());
    let text = content_text(&response);
    let payload: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(payload["ack"], "C");
    assert_eq!(payload["value"], 1);
    assert!(payload.get("telemetry").is_some());
}

#[test]
fn quiet_combinators_handle_edge_cases_compactly() {
    let result = execute_codemode_with_options(r#"const empty_count = zero.count(""); const array_count = zero.count([1, 2, 3]); const missing_first = zero.first([]); const first_two = zero.first("a
b
c", 2); const verdict = zero.verdict(false, "bad
verbose"); return { empty_count, array_count, missing_first, first_two, verdict };"#, CodeModeOptions::default());
    assert_completed(&result);
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
    let result = run_plan("return missing_binding");
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
        let limits: crate::CodeModeLimits =
        serde_json::from_value(serde_json::json!({ "max_output_bytes": 1024 }))
            .expect("partial limits object MUST deserialize");
    assert_eq!(limits.max_output_bytes, 1024);
    assert_eq!(limits.max_logical_ops, crate::CodeModeLimits::default().max_logical_ops, "missing fields take defaults");
    let empty: crate::CodeModeLimits = serde_json::from_value(serde_json::json!({})).unwrap();
    assert_eq!(empty.max_code_bytes, crate::CodeModeLimits::default().max_code_bytes);
}

#[test]
fn read_honors_start_and_end_line_options() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("lines.txt");
    let content: String = (1..=20).map(|line| format!("LINE_{line}\n")).collect();
    fs::write(&path, content).unwrap();

    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let plan = format!("await zero.read({quoted}, {{ start_line: 2, end_line: 3 }})");
    let result = run_at(work.path(), &plan);
    assert_completed(&result);
    let text = result.value.as_ref().and_then(|v| v.get("text")).and_then(|v| v.as_str()).unwrap_or("");
    assert!(text.contains("LINE_2"), "expected bounded read: {text}");
    assert!(text.contains("LINE_3"), "expected bounded read: {text}");
    assert!(!text.contains("LINE_1"), "start_line should exclude earlier lines: {text}");
    assert!(!text.contains("LINE_4"), "end_line should exclude later lines: {text}");
}

#[test]
fn pathless_tree_uses_configured_root_not_cwd() {
    let work = tempfile::tempdir().unwrap();
    fs::write(work.path().join("marker.txt"), "present\n").unwrap();
    assert_ne!(work.path(), std::env::current_dir().unwrap().as_path());
    let result = run_at(work.path(), "await zero.tree()");
    assert_completed(&result);
    let text = result.value.as_ref().and_then(|v| v.get("text")).and_then(|v| v.as_str()).unwrap_or("");
    assert!(text.contains("marker.txt"), "tree should search configured --root, not process cwd: {text}");
}

#[test]
fn parser_contract_matrix() {
    let err = parse_expr("\"").unwrap_err();
    assert!(err.contains("unterminated string literal"));
    assert!(parse_plan("return \"").is_err());

    let scope = HashMap::new();
    let value = resolve_expr(
        &parse_expr("{ start_line: 1, end_line: 10, max_files: 5 }").unwrap(),
        &scope,
    ).unwrap();
    let obj = value.as_object().unwrap();
    for (key, expected) in [("start_line", 1), ("end_line", 10), ("max_files", 5)] {
        assert_eq!(obj.get(key).and_then(serde_json::Value::as_u64), Some(expected));
    }
    for literal in ["inf", "nan"] { assert!(parse_expr(literal).is_err()); }

    let stmts = parse_plan(r#"const x = await zero.compact("hello"); return x.ref"#).unwrap();
    assert_eq!(stmts.len(), 2);
    assert!(matches!(&stmts[0], Statement::Binding { name, .. } if name == "x"));
    assert!(matches!(&stmts[1], Statement::Return(..)));
    let stmts = parse_plan("const a = await zero.shell(\"ls\");\nconst b = await zero.shell(\"pwd\");\nreturn { a, b }").unwrap();
    assert_eq!(stmts.len(), 3);
    let stmts = parse_plan(r#"zero.read("src/main.rs", { mode: "auto", start_line: 1, end_line: 10 })"#).unwrap();
    assert_eq!(stmts.len(), 1);
    assert!(matches!(&stmts[0], Statement::Call(call) if call.method == "zero.read" && call.args.len() == 2));
}

#[test]
fn catalog_and_validation_matrix() {
    let empty = run_plan("");
    assert_error_contains(&empty, "empty");

    let search = run_plan("search:read");
    assert_completed(&search);
    let hits = search.value.unwrap();
    let hits = hits["results"].as_array().unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0]["path"].as_str().unwrap().contains("read"));

    let describe = run_plan("describe:zero.read");
    assert_completed(&describe);
    assert!(describe.value.unwrap()["signature"].as_str().unwrap().contains("Promise"));
    let unknown = run_plan("describe:zero.nonexistent");
    assert_completed(&unknown);
    let value = unknown.value.unwrap();
    assert!(value["error"].is_string());
    assert!(value["available"].as_array().unwrap().len() > 5);
    let unknown = run_plan("await zero.banana()");
    assert_error_contains(&unknown, "unknown method");
    assert!(unknown.error.as_ref().unwrap().contains("codemode.search"));
}

#[test]
fn compact_roundtrip_alias_matrix() {
    for (compact, expand, needle) in [
        (r#"await zero.compact("test payload for codemode")"#, r#"await zero.expand("{ref_id}")"#, "test payload for codemode"),
        (r#"await zero.token.compact("token namespace payload")"#, r#"await zero.token.expand("{ref_id}")"#, "token namespace payload"),
    ] {
        assert_compact_roundtrip(compact, expand, needle);
    }
}

#[test]
fn compact_object_json_serializes_and_roundtrips_exactly() {
        let blob = "x".repeat(9000);
    let plan = format!(
        r#"const obj = {{ nested: {{ blob: "{}", answer: 42 }} }}; const c = await zero.token.compact(obj); const e = await zero.token.expand(c.ref); return {{ blob_len: e.nested.blob.length, answer: e.nested.answer, exact: JSON.stringify(e) === JSON.stringify(obj) }}"#,
        blob
    );
    let r = run_plan(&plan);
    assert_completed(&r);
    let val = r.value.as_ref().unwrap();
    assert_eq!(val["blob_len"].as_u64(), Some(9000), "blob length via property access: {val}");
    assert_eq!(val["answer"].as_u64(), Some(42), "answer via property access: {val}");
    assert_eq!(val["exact"].as_bool(), Some(true), "parsed object must deep-equal the original: {val}");
}

#[test]
fn describe_token_namespace_returns_signature() {
    let r = run_plan("describe:zero.token.compact");
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["path"], "zero.token.compact");
    assert!(val["signature"].as_str().unwrap().contains("zero.token.compact"));
}

#[test]
fn codemode_engine_uses_shared_recovery_cache_and_repo_scope() {
        let root = PathBuf::from("/tmp/tokenzero-codemode-root");
    let engine = make_engine_for_root(root.clone());
    assert_eq!(engine.config.allowed_roots, vec![root.clone()]);
    assert_eq!(engine.config.cache_path, crate::workspace::default_recovery_cache_path(&root));
    assert!(engine.config.cache_path.to_string_lossy().contains("recovery-cache.json"), "{}", engine.config.cache_path.display());
}

#[test]
fn caller_soft_wall_cannot_raise_hard_wall() {
    let limits = limits_from_options(&CodeModeOptions { max_wall_ms: 60_000, hard_max_wall_ms: 5_000, ..Default::default() });
    assert_eq!(limits.max_wall_ms, 5_000);
    assert_eq!(limits.hard_max_wall_ms, 5_000);

    let trusted = limits_from_options(&CodeModeOptions { max_wall_ms: 60_000, hard_max_wall_ms: 60_000, ..Default::default() });
    assert_eq!(trusted.max_wall_ms, 60_000);
    assert_eq!(trusted.hard_max_wall_ms, 60_000);
}

#[test]
fn edit_failure_matrix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.txt");
    fs::write(&path, "hello
").unwrap();
    let engine = make_engine_for_root(dir.path().to_path_buf());
    let bad = exec_edit(&engine, dir.path(), &[
        serde_json::json!(path.to_string_lossy().to_string()),
        serde_json::json!([{"find":"hello","replace":"bye"},{"find":"hello"}]),
    ]).unwrap_err();
    assert!(bad.error.as_deref().unwrap().contains("invalid hunk at index 1"), "{:?}", bad.error);
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello
");

    engine.surface_health().record_substrate_down();
    let err = exec_edit(&engine, dir.path(), &[
        serde_json::json!(path.to_string_lossy().to_string()),
        serde_json::json!([{"find":"missing","replace":"bye"}]),
    ]).unwrap_err();
    let msg = err.error.as_deref().unwrap_or("");
    assert!(msg.contains("Write recovery ladder") || msg.contains("tz_report_tool_issue"), "expected write ladder: {msg}");
    assert!(!msg.contains("write_escape_ack"), "expand/read health must not authorize native writes: {msg}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "hello
");
}

#[test]
fn shell_plan_captures_exit_code() {
    let r = run_plan(r#"await zero.shell("echo hello")"#);
    assert_completed(&r);
    assert!(r.value.unwrap()["status"].is_string());
}

#[cfg(unix)]
#[test]
fn background_shell_returns_before_wall_cap_and_can_be_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("background-prompt-cache.json");
    let options = CodeModeOptions { root: Some(dir.path().to_path_buf()), cache_path: Some(cache), max_wall_ms: 500, hard_max_wall_ms: 500, ..CodeModeOptions::default() };
    let started = std::time::Instant::now();
    let result = execute_codemode_with_options(
        r#"return zero.token.shell("sleep 30", { background: true })"#,
        options,
    );
    assert_completed(&result);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    let value = result.value.unwrap();
    assert!(value["job"].as_str().unwrap().starts_with("tzjob-"));
    assert!(value["log"].as_str().unwrap().ends_with(".log"));
    make_engine_for_root(dir.path().to_path_buf()).shutdown_background_jobs_for_test();
}

#[cfg(unix)]
#[test]
fn background_job_polls_to_exit_with_log_tail() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("background-poll-cache.json");
    let options = || CodeModeOptions { root: Some(dir.path().to_path_buf()), cache_path: Some(cache.clone()), max_wall_ms: 1_000, hard_max_wall_ms: 1_000, ..CodeModeOptions::default() };
    let launched = execute_codemode_with_options(
        r#"return zero.token.shell("printf alpha; sleep 0.2; printf omega", { background: true })"#,
        options(),
    );
    assert_completed(&launched);
    let job = launched.value.unwrap()["job"].as_str().unwrap().to_string();
    let running = execute_codemode_with_options(&format!(r#"return zero.token.job("{job}")"#), options());
    assert_completed(&running);
    assert_eq!(running.value.unwrap()["status"], "running");
    std::thread::sleep(std::time::Duration::from_millis(500));
    let exited = execute_codemode_with_options(&format!(r#"return zero.token.job("{job}")"#), options());
    assert_completed(&exited);
    let value = exited.value.unwrap();
    assert_eq!(value["status"], "exited");
    assert_eq!(value["exitCode"], 0);
    assert!(value["tail"].as_str().unwrap().contains("alpha"));
    assert!(value["tail"].as_str().unwrap().contains("omega"));
    assert!(std::path::Path::new(value["log"].as_str().unwrap()).exists());
}

#[test]
fn multi_statement_composition() {
    let plan = r#"const data = await zero.compact("composed payload"); const expanded = await zero.expand(data.ref); return { ref: data.ref, found: expanded.text }"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
}

#[test]
fn expand_invalid_ref_matrix() {
    let missing = run_plan(r#"await zero.expand("tz://blob/nonexistent123")"#);
    assert_completed(&missing);
    assert!(missing.value.unwrap()["status"].is_string(), "missing ref must complete without panic");
    let bad = run_plan(r#"await zero.expand("not-a-ref")"#);
    assert_eq!(bad.status, CodeModeStatus::Error);
    assert!(bad.error.as_ref().unwrap().contains("tz://"));
}

#[test]
fn expand_same_store_scheme_alias_fz_gz_in_one_plan() {
            let plan = r#"const data = await zero.compact("codemode cross-scheme body"); const id = String(data.ref).replace("tz://blob/", ""); const a = await zero.expand("fz://blob/" + id); const b = await zero.expand("gz://blob/" + id); return { tz: data.ref, fz_text: a, gz_text: b, match: a === b && String(a).includes("cross-scheme") }"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["match"], true, "{val}");
    assert!(val["fz_text"].as_str().unwrap().contains("codemode cross-scheme body"), "{val}");
}

#[test]
fn windowed_expand_same_session_codemode_blob() {
        let plan = r#"const lines = Array.from({length: 200}, (_, i) => "line-" + (i + 1)).join("
") + "
"; const data = await zero.compact(lines); const win = await zero.expand(data.ref, { start_line: 120, end_line: 190 }); const text = typeof win === "string" ? win : (win && win.text) || ""; return { ref: data.ref, starts: String(text).startsWith("line-120"), has190: String(text).includes("line-190"), no119: !String(text).includes("line-119"), no191: !String(text).includes("line-191"), text }"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    for key in ["starts", "has190", "no119", "no191"] {
        assert_eq!(val[key], true, "{key} in {val}");
    }
}

#[test]
fn discovery_catalog_matrix() {
    let describe = run_plan("describe:zero.recall");
    assert_completed(&describe);
    assert!(describe.value.as_ref().unwrap()["signature"].as_str().unwrap().contains("zero.recall"));
    assert!(run_plan("search:recall").value.as_ref().unwrap()["results"].as_array().unwrap().iter().any(|hit| hit["path"] == "zero.recall"));
    let catalog = crate::codemode::catalog::codemode_method_catalog();
    assert_eq!(catalog["schema_version"], "tokenzero.codemode.catalog.v1");
    assert!(catalog["methods"].as_array().unwrap().iter().any(|m| m["path"] == "zero.recall"));
    assert!(run_plan("search:zero").value.unwrap()["results"].as_array().unwrap().len() >= 10);
    assert!(run_plan("search:compact_max").value.unwrap()["results"].as_array().unwrap().iter().any(|hit| hit["path"] == "zero.compact_max"));
}

#[test]
fn pipe_sequential_composition() {
    let plan = r#"await zero.pipe([{"method": "zero.compact", "args": ["step one"]}, {"method": "zero.compact", "args": ["step two"]}], {"raw": true})"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["steps"], 2);
    let results = val["results"].as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert!(results[0]["ref"].as_str().unwrap().starts_with("tz://"));
    assert!(results[1]["ref"].as_str().unwrap().starts_with("tz://"));
    assert_error_contains(&run_plan(r#"await zero.pipe([])"#), "at least one step");
}

#[test]
fn pick_extracts_keys_from_result() {
    let plan = r#"const data = await zero.compact("payload for pick test"); const picked = await zero.pick(data, ["ref", "status"]); return picked"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(val["status"], "ok");
    assert!(val.get("text").is_none(), "text should be excluded by pick");
}

#[test]
fn filter_lines_narrows_text() {
    let plan = r#"const data = await zero.compact("alpha line
beta match
gamma line
delta match"); const expanded = await zero.expand(data.ref); const filtered = await zero.filter_lines(expanded, "match"); return filtered"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["lines"], 2);
    let text = val["text"].as_str().unwrap();
    assert!(text.contains("beta match"));
    assert!(text.contains("delta match"));
    assert!(!text.contains("alpha"));
}

#[test]
fn telemetry_reports_equivalent_calls() {
    let plan = r#"const a = await zero.compact("first"); const b = await zero.compact("second"); const c = await zero.expand(a.ref); return { a_ref: a.ref, b_ref: b.ref, c_text: c.text }"#;
    let r = run_plan(plan);
    assert_completed(&r);
    assert_eq!(r.telemetry.operations, 3);
    assert_eq!(r.telemetry.equivalent_calls, Some(4));
}

#[test]
fn multi_step_dataflow_with_intermediate_binding() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("data.txt");
    fs::write(&path, "line1 important\nline2 noise\nline3 important\n").unwrap();

    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let plan = format!(r#"const content = await zero.read({quoted}); const filtered = await zero.filter_lines(content, "important"); return {{ lines: filtered.lines, text: filtered.text }}"#);
    let r = execute_codemode_with_options(&plan, opts_for(work.path()));
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["lines"], 2);
    assert!(val["text"].as_str().unwrap().contains("important"));
    assert!(!val["text"].as_str().unwrap().contains("noise"));
    assert_eq!(r.telemetry.operations, 2);
    assert_eq!(r.telemetry.equivalent_calls, Some(3));
}

#[test]
fn pipe_and_pick_composition() {
    let plan = r#"const piped = await zero.pipe([{"method": "zero.compact", "args": ["piped data"]}], {"raw": true}); const picked = await zero.pick(piped, ["steps", "last"]); return picked"#;
    let r = run_plan(plan);
    assert_completed(&r);
    let val = r.value.unwrap();
    assert_eq!(val["steps"], 1);
    assert!(val["last"].is_object());
}

#[test]
fn new_composition_methods_discoverable() {
    for (q, p) in [("pipe", "zero.pipe"), ("pick", "zero.pick"), ("filter", "zero.filter_lines")] {
        assert_search_hit(q, p);
    }
}

#[test]
fn compact_recovery_matrix() {
    let large_code = (0..200)
        .map(|i| format!("pub fn handler_{i}(ctx: &Context, request: Request<Body>) -> Result<Response<Body>, Error> {{ log::info!(\"handling request {i}\"); Ok(Response::new(Body::empty())) }}"))
        .collect::<Vec<_>>()
        .join("
");
    let r = run_plan(&format!(r#"await zero.compact({})"#, serde_json::to_string(&large_code).unwrap()));
    assert_completed(&r);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(val["compression_strategy"], "content_aware");
    assert!(val["visible_tokens"].as_u64().unwrap() < val["raw_tokens"].as_u64().unwrap());

    let large_logs = (0..200)
        .map(|i| if i % 20 == 0 { format!("ERROR: something failed at step {i}") } else { format!("INFO: processing item {i} successfully") })
        .collect::<Vec<_>>()
        .join("
");
    let r = run_plan(&format!(r#"await zero.compact_max({})"#, serde_json::to_string(&large_logs).unwrap()));
    assert_completed(&r);
    let val = r.value.unwrap();
    assert!(val["ref"].as_str().unwrap().starts_with("tz://"));
    assert_eq!(val["compression_strategy"], "content_aware_max");
    let (vis, raw) = (val["visible_tokens"].as_u64().unwrap(), val["raw_tokens"].as_u64().unwrap());
    assert!(vis < raw / 2, "aggressive should save >50%: vis={vis} raw={raw}");

    let payload = "exact recovery test: special chars !@#$%^&*()
newlines	tabs
";
    let r = run_plan(&format!(
        r#"const c = await zero.compact_max({}); const e = await zero.expand(c.ref); return {{ original_ref: c.ref, recovered: e.text }}"#,
        serde_json::to_string(payload).unwrap()
    ));
    assert_completed(&r);
    assert_eq!(r.value.unwrap()["recovered"].as_str().unwrap(), payload);

    let logs = (0..500)
        .map(|i| {
            if i == 42 { "FATAL: database connection lost at 2024-01-15T10:30:00Z host=prod-db-1 stack=main".into() }
            else if i == 77 { "ERROR: timeout exceeded after 30s waiting for upstream response from gateway".into() }
            else { format!("DEBUG: routine operation {i} completed successfully in 2ms status=200 bytes=1024") }
        })
        .collect::<Vec<_>>()
        .join("
");
    let r = run_plan(&format!(r#"await zero.compact_max({})"#, serde_json::to_string(&logs).unwrap()));
    assert_completed(&r);
    let value = r.value.unwrap();
        let text = value["text"].as_str().unwrap();
    assert!(text.contains("FATAL") || text.contains("ERROR"), "content-aware should surface errors: {text}");
}

#[test]
fn parity_plan_vs_direct_matrix() {
    let work = tempfile::tempdir().unwrap();
    let path = work.path().join("test.txt");
    fs::write(&path, "line one
line two
line three
").unwrap();
    let quoted = serde_json::to_string(path.to_str().unwrap()).unwrap();
    let opts = opts_for(work.path());
    let direct = execute_codemode_with_options(&format!(r#"await zero.read({quoted})"#), opts.clone());
    let plan = execute_codemode_with_options(&format!(r#"const r = await zero.read({quoted}); return r"#), opts);
    assert_completed(&direct); assert_completed(&plan);
    let (d, pval) = (direct.value.unwrap(), plan.value.unwrap());
    for key in ["text", "ref", "visible_tokens", "raw_tokens"] { assert_eq!(d[key], pval[key], "read {key}"); }

    let run_grep = |plan_tpl: &str| {
        let work = tempfile::tempdir().unwrap();
        fs::write(work.path().join("code.rs"), "fn main() {}
fn helper() {}
struct Foo;
").unwrap();
        let dir = serde_json::to_string(work.path().to_str().unwrap()).unwrap();
        let result = execute_codemode_with_options(&plan_tpl.replace("{dir}", &dir), opts_for(work.path()));
        assert_completed(&result);
        (result.value.unwrap(), work.path().to_str().unwrap().to_string())
    };
    let (d_val, d_root) = run_grep(r#"await zero.grep("fn", {dir})"#);
    let (p_val, p_root) = run_grep(r#"const g = await zero.grep("fn", {dir}); return g"#);
    let norm = |val: &serde_json::Value, root: &str| val["text"].as_str().unwrap_or_default().replace(root, "<ROOT>");
    assert_eq!(norm(&d_val, &d_root), norm(&p_val, &p_root), "grep results must match modulo root");

    let opts = CodeModeOptions::default();
    let direct = execute_codemode_with_options(r#"await zero.shell("echo hello world")"#, opts.clone());
    let plan = execute_codemode_with_options(r#"const s = await zero.shell("echo hello world"); return s"#, opts);
    assert_completed(&direct); assert_completed(&plan);
    let (d, pval) = (direct.value.unwrap(), plan.value.unwrap());
    for key in ["text", "exit_code", "success"] { assert_eq!(d[key], pval[key], "shell {key}"); }

    let work = tempfile::tempdir().unwrap();
    let (path1, path2) = (work.path().join("a.txt"), work.path().join("b.txt"));
    fs::write(&path1, "hello world").unwrap();
    fs::write(&path2, "hello world").unwrap();
    let opts = CodeModeOptions { root: Some(work.path().to_path_buf()), cache_path: Some(work.path().join(".tokenzero/recovery-cache.json")), ..Default::default() };
    let (q1, q2) = (serde_json::to_string(path1.to_str().unwrap()).unwrap(), serde_json::to_string(path2.to_str().unwrap()).unwrap());
    let direct = execute_codemode_with_options(&format!(r#"await zero.edit({q1}, [{{ "find": "hello", "replace": "goodbye" }}])"#), opts.clone());
    let plan = execute_codemode_with_options(&format!(r#"const e = await zero.edit({q2}, [{{ "find": "hello", "replace": "goodbye" }}]); return e"#), opts);
    assert_completed(&direct); assert_completed(&plan);
    assert_eq!(fs::read_to_string(&path1).unwrap(), "goodbye world");
    assert_eq!(fs::read_to_string(&path2).unwrap(), "goodbye world");
}

#[test]
fn quickjs_freeform_edit_denied_includes_write_ladder() {
        let r = run_plan("const f = () => zero.edit('file.txt', []); return f();");
    assert_eq!(r.status, CodeModeStatus::Error);
    let msg = r.error.as_ref().map(|e| e.message.as_str()).unwrap_or("");
    assert!(msg.contains("sandbox"), "{msg}");
    assert!(msg.contains("Write recovery ladder") || msg.contains("tz_report_tool_issue"), "expected ladder: {msg}");
}

#[test]
fn quiet_helper_and_catalog_contract_matrix() {
    let counted = run_plan(r#"await zero.count_tokens("hello world this is a test")"#);
    assert_completed(&counted);
    let value = counted.value.unwrap();
    assert!(value["tokens"].as_u64().unwrap() > 0);
    assert_eq!(value["bytes"], 26);
    assert_eq!(value["lines"], 1);
    let passed = run_plan(r#"await zero.assert(true, "should pass")"#);
    assert_completed(&passed);
    assert_eq!(passed.value.unwrap()["ok"], true);
    assert_error_contains(&run_plan(r#"await zero.assert(false, "expected failure")"#), "expected failure");
    let search = run_plan("search:read").value.unwrap();
    let hit = search["results"].as_array().unwrap().iter().find(|h| h["path"] == "zero.read").unwrap();
    assert!(hit["signature"].as_str().unwrap().contains("path: string"));
    assert!(hit["example"].as_str().unwrap().contains("await"));
    let related = run_plan("describe:zero.read").value.unwrap()["related"].as_array().unwrap().clone();
    assert!(!related.is_empty() && related.iter().any(|item| item.as_str() == Some("zero.expand")));
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
        assert!(lower_code_plan(plan, &limits).is_ok(), "false positive for plan: {plan}");
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
    let lower = |plan: &str| lower_code_plan(plan, &limits).unwrap();
    let lowered = lower("const p = \"/tmp/fab-api.txt\"; const q = 'ctx.ref in a string'; return p");
    assert!(lowered.contains("/tmp/fab-api.txt") && lowered.contains("ctx.ref in a string"), "string literal corrupted: {lowered}");
    assert!(lower("const myapi = 1; return myapi.foo").contains("myapi.foo"), "identifier tail corrupted");
    assert!(lower("return api.read(\"a.txt\")").contains("zero.read("), "api alias not rewritten");
    assert!(lower("return token.compact(x)").contains("zero.token.compact(x)"), "token alias not rewritten");
    let lowered = lower("return zero.token.compact(x)");
    assert!(lowered.contains("zero.token.compact(x)") && !lowered.contains("zero.zero."), "double prefix: {lowered}");
}

#[test]
fn foreign_root_token_read_relative_and_absolute() {
    let foreign = tempfile::tempdir().unwrap();
    let foreign_opts = || CodeModeOptions { root: Some(foreign.path().to_path_buf()), allowed_roots: vec![], cache_path: Some(foreign.path().join(".tokenzero/recovery-cache.json")), ..Default::default() };
    let changelog = foreign.path().join("CHANGELOG.md");
    fs::write(&changelog, "# foreign changelog
wqw5-marker
").unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("secret.txt"), "nope
").unwrap();
    let result = execute_codemode_with_options(r#"return await zero.token.read("CHANGELOG.md")"#, foreign_opts());
    assert_completed(&result);
    let text = serde_json::to_string(&result.value).unwrap_or_default();
    assert!(text.contains("wqw5-marker") || result.to_line().contains("wqw5-marker"), "relative read under foreign root: {result:?}");
    let abs = changelog.display().to_string().replace('\\', "\\\\");
    assert_completed(&execute_codemode_with_options(&format!(r#"return await zero.token.read("{abs}")"#), foreign_opts()));
    let outside_file = outside.path().join("secret.txt");
    let denied = execute_codemode_with_options(&format!(r#"return await zero.token.read("{}")"#, outside_file.display().to_string().replace('\\', "\\\\")), foreign_opts());
    assert_eq!(denied.status, CodeModeStatus::Error, "outside must deny");
    let err = denied.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
    assert!(err.contains("outside allowed roots") || err.to_ascii_lowercase().contains("not allowed"), "deny message: {err}");
}

#[cfg(unix)]
#[test]
fn relative_shell_cwd_is_anchored_to_execute_root() {
    let root = tempfile::tempdir().unwrap();
    let extra = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("sub")).unwrap();
    std::fs::create_dir_all(extra.path().join("sub")).unwrap();

    let result = execute_codemode_with_options(
        r#"return await zero.shell("pwd", { cwd: "sub" })"#,
        CodeModeOptions {
            root: Some(root.path().to_path_buf()),
            allowed_roots: vec![extra.path().to_path_buf()],
            cache_path: Some(root.path().join(".tokenzero/recovery-cache.json")),
            ..Default::default()
        },
    );
    assert_completed(&result);
    let rendered = serde_json::to_string(&result.value).unwrap_or_default();
    let expected = root.path().join("sub").display().to_string();
    assert!(rendered.contains(expected.as_str()), "relative cwd escaped execute root: {rendered}");
}

#[test]
fn default_root_token_read_still_works() {
    let work = tempfile::tempdir().unwrap();
    fs::write(work.path().join("README.md"), "default-root-ok\n").unwrap();
    let result = execute_codemode_with_options(
        r#"return await zero.token.read("README.md")"#,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            cache_path: Some(work.path().join(".tokenzero/recovery-cache.json")),
            ..Default::default()
        },
    );
    assert_completed(&result);
}

#[test]
fn recipe_registry_contracts_and_resolve_paths_against_work_root() {
    let root = PathBuf::from("/tmp/foreign-proj");
    let resolved = resolve_paths_against_work_root(
        vec![PathBuf::from("CHANGELOG.md"), PathBuf::from("/abs/file.txt")],
        &root,
    );
    assert_eq!(resolved[0], root.join("CHANGELOG.md"));
    assert_eq!(resolved[1], PathBuf::from("/abs/file.txt"));

    let work = tempfile::tempdir().unwrap();
    let opts = |cache: PathBuf| CodeModeOptions { root: Some(work.path().to_path_buf()), cache_path: Some(cache), ..Default::default() };
    let run = |plan: &str, cache: PathBuf| execute_codemode_with_options(plan, opts(cache));
    let ok = |plan: &str, cache: PathBuf| { let r = run(plan, cache); assert_completed(&r); r };
    let err_kind = |plan: &str, cache: PathBuf, kind: &str| {
        assert_eq!(run(plan, cache).error.as_ref().map(|e| e.kind.as_str()), Some(kind));
    };

    let cache = work.path().join("recipe-happy.json");
    ok(r#"return zero.register("zeta", "return args.value;")"#, cache.clone());
    ok(r#"return zero.register("alpha", "return { value: args.message, nested: args.nested.value };")"#, cache.clone());
    let ran = ok(r#"return await zero.run("alpha", { message: "hello", nested: { value: 7 } })"#, cache.clone());
    assert_eq!(ran.value, Some(serde_json::json!({"value": "hello", "nested": 7})));
    assert_eq!(ok("return zero.list()", cache).value, Some(serde_json::json!(["alpha", "zeta"])));
    err_kind(r#"return await zero.run("missing")"#, work.path().join("recipe-missing.json"), "recipe_not_found");

    let policy = work.path().join("recipe-policy.json");
    ok(r#"return zero.register("denied", ["return await zero.", "edi", "t(\"blocked.txt\", []);"].join(""))"#, policy.clone());
    err_kind(r#"return await zero.run("denied")"#, policy, "sandbox");

    let args_cache = work.path().join("recipe-args.json");
    ok(r#"return zero.register("echo", "args.nested.value = 'mutated'; return args;")"#, args_cache.clone());
    let injected = r#"return zero.shell("touch should-not-run")"#;
    let injected_run = ok(&format!(r#"return await zero.run("echo", {{ injected: {}, nested: {{ value: "original" }} }})"#, serde_json::to_string(injected).unwrap()), args_cache);
    assert_eq!(injected_run.value.as_ref().unwrap()["injected"], injected);
    assert_eq!(injected_run.value.as_ref().unwrap()["nested"]["value"], "original");
    assert!(!work.path().join("should-not-run").exists());

    let capacity = work.path().join("recipe-capacity.json");
    for index in 0..64 { ok(&format!(r#"return zero.register("r{index:02}", "return {index};")"#), capacity.clone()); }
    err_kind(r#"return zero.register("overflow", "return 65;")"#, capacity, "recipe_registry_full");

    let oversized = execute_codemode_with_options(
        &format!(r#"return zero.register("too-large", {})"#, serde_json::to_string(&"x".repeat(64 * 1024 + 1)).unwrap()),
        CodeModeOptions { max_code_bytes: 128 * 1024, ..opts(work.path().join("recipe-size.json")) },
    );
    assert_eq!(oversized.error.as_ref().map(|e| e.kind.as_str()), Some("recipe_source_too_large"));

    let first = work.path().join("recipe-session-00.json");
    for index in 0..33 {
        ok(r#"return zero.register("only", "return 1;")"#, work.path().join(format!("recipe-session-{index:02}.json")));
    }
    err_kind(r#"return await zero.run("only")"#, first, "recipe_not_found");

    let source = r#"const inventory = await zero.tree("crates", { depth: 4 }); const matches = await zero.find("dispatch_values", "crates/tokenzero-mcp/src"); return { inventory, matches };"#;
    let invocation = r#"return await zero.run("inventory", { depth: 4 })"#;
    assert!(tokenzero_core::count_tokens(invocation) < tokenzero_core::count_tokens(source));
}
