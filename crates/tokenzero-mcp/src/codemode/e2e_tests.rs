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
        let result = execute_codemode_with_options(
            plan,
            CodeModeOptions {
                // Explicit root: the deny ack must not depend on the
                // root_fallback warning that rides visible_ack otherwise.
                root: Some(std::env::temp_dir()),
                ..CodeModeOptions::default()
            },
        );
        assert_eq!(
            result.status,
            CodeModeStatus::Error,
            "plan should fail: {plan}"
        );
        assert_eq!(result.visible_ack, "2");
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
    assert!(
        text.contains("=7"),
        "scalar should fold into v3 ack: {text}"
    );
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

#[test]
fn promise_all_runs_independent_shells_concurrently() {
    // tokenzero-codemode-parallel-broken-z28: Promise.all must fan out host ops.
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        await Promise.all([
          zero.shell('touch promise-all-a; i=0; while [ ! -f promise-all-b ] || [ ! -f promise-all-c ]; do i=$((i+1)); [ "$i" -lt 100 ] || exit 9; sleep 0.02; done'),
          zero.shell('touch promise-all-b; i=0; while [ ! -f promise-all-a ] || [ ! -f promise-all-c ]; do i=$((i+1)); [ "$i" -lt 100 ] || exit 9; sleep 0.02; done'),
          zero.shell('touch promise-all-c; i=0; while [ ! -f promise-all-a ] || [ ! -f promise-all-b ]; do i=$((i+1)); [ "$i" -lt 100 ] || exit 9; sleep 0.02; done'),
        ]);
        return { ok: true };
    "#;
    let result = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            max_parallel_width: 3,
            max_wall_ms: 5000,
            hard_max_wall_ms: 5000,
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "{:?}",
        result.error
    );
    assert_eq!(
        result.telemetry.physical_ops, 3,
        "expected exactly 3 physical ops"
    );
    assert_eq!(
        result.telemetry.parallel_groups,
        Some(1),
        "expected exactly one parallel group"
    );
}

#[test]
fn expand_object_arg_coerces_ref_form() {
    // tokenzero-expand-arg-coercion-inh: expand({ref}) must not throw opaque QuickJS.
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        const compacted = await zero.token.compact("object-arg expand payload");
        const expanded = await zero.token.expand({ ref: compacted.ref, raw: true });
        return { text: expanded.text, status: expanded.status };
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
        "object-arg expand must complete with typed path, not opaque QuickJS: {:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert_eq!(
        value["text"].as_str(),
        Some("object-arg expand payload"),
        "{value:?}"
    );
}

#[test]
fn expand_array_arg_routes_to_expand_many() {
    // tokenzero-expand-arg-coercion-inh: expand([ref, ...]) → expandMany.
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        const a = await zero.token.compact("alpha-coercion");
        const b = await zero.token.compact("beta-coercion");
        const batch = await zero.token.expand([a.ref, { ref: b.ref }]);
        return { count: batch.count, texts: batch.items.map((item) => item.text) };
    "#;
    let result = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            // This proves argument routing, not the production five-second wall.
            // Leave headroom for shared machine-permit contention in the full suite.
            max_wall_ms: 60_000,
            hard_max_wall_ms: 60_000,
            ..Default::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "array-arg expand must route to expandMany: {:?}",
        result.error
    );
    let value = result.value.as_ref().unwrap();
    assert_eq!(value["count"].as_u64(), Some(2));
    let texts = value["texts"].as_array().expect("texts");
    assert_eq!(texts[0].as_str(), Some("alpha-coercion"));
    assert_eq!(texts[1].as_str(), Some("beta-coercion"));
}

#[test]
fn expand_bad_arg_shape_returns_typed_error() {
    // tokenzero-expand-arg-coercion-inh: bad shapes name the positional signature.
    let result = execute_codemode_with_options(
        "return await zero.token.expand(42)",
        CodeModeOptions::default(),
    );
    assert_eq!(result.status, CodeModeStatus::Error);
    let message = &result.error.as_ref().unwrap().message;
    assert!(
        message.contains("requires a tz:// ref string") || message.contains("got number"),
        "expected typed signature error, got opaque/wrong: {message}"
    );
    assert!(
        !message.contains("Exception generated by QuickJS"),
        "must not surface opaque QuickJS: {message}"
    );
}

#[test]
fn concurrent_direct_compact_expand_uses_requested_store() {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().to_path_buf();
    let cache_path = root.join("intended-store.json");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let mut workers = Vec::new();

    for payload in ["concurrent-alpha", "concurrent-beta"] {
        let root = root.clone();
        let cache_path = cache_path.clone();
        let barrier = std::sync::Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let plan = format!(
                "const compacted = zero.token.compact({}); const expanded = zero.token.expand(compacted.ref); return {{ ref: compacted.ref, text: expanded.text }};",
                serde_json::to_string(payload).unwrap()
            );
            let result = execute_codemode_with_options(
                &plan,
                CodeModeOptions {
                    root: Some(root),
                    cache_path: Some(cache_path),
                    max_wall_ms: 30_000,
                    hard_max_wall_ms: 30_000,
                    ..Default::default()
                },
            );
            assert_eq!(result.status, CodeModeStatus::Completed, "{:?}", result.error);
            let value = result.value.unwrap();
            (
                payload.to_string(),
                value["ref"].as_str().unwrap().to_string(),
                value["text"].as_str().unwrap().to_string(),
            )
        }));
    }

    barrier.wait();
    let outcomes: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    let mut store = tokenzero_recovery::RecoveryStore::new(Some(cache_path));
    for (payload, ref_id, direct_text) in outcomes {
        assert_eq!(
            direct_text, payload,
            "direct expand returned the wrong session payload"
        );
        let expanded = store.expand(&ref_id, None, None, None, None, None);
        assert!(
            expanded.found,
            "ref was not persisted to requested store: {}",
            expanded.reason
        );
        assert_eq!(expanded.content, payload);
    }
}

#[test]
fn shell_ref_is_canonical_and_expandable_in_subsequent_execution() {
    let work = tempfile::tempdir().unwrap();
    let cache_path = work.path().join("recovery-cache.json");
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Write-Output codemode-durable-shell-ref"
    } else {
        "printf 'codemode-durable-shell-ref\\n'"
    };
    let mint = execute_codemode_with_options(
        &format!(
            "const a = await zero.token.shell({}); return a.stdout_ref;",
            serde_json::to_string(command).unwrap()
        ),
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            cache_path: Some(cache_path.clone()),
            ..Default::default()
        },
    );
    assert_eq!(mint.status, CodeModeStatus::Completed, "{:?}", mint.error);
    let shell_ref = mint.value.as_ref().unwrap().as_str().unwrap();
    assert!(
        shell_ref.starts_with("tz://blob/"),
        "CodeMode must not return session aliases that replay caches can outlive: {shell_ref}"
    );

    let expanded = execute_codemode_with_options(
        &format!(
            "return await zero.token.expand({})",
            serde_json::to_string(shell_ref).unwrap()
        ),
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            cache_path: Some(cache_path),
            ..Default::default()
        },
    );
    assert_eq!(
        expanded.status,
        CodeModeStatus::Completed,
        "{:?}",
        expanded.error
    );
    let value = expanded.value.as_ref().unwrap();
    let text = value
        .as_str()
        .or_else(|| value.get("text").and_then(serde_json::Value::as_str));
    assert_eq!(
        text,
        Some("codemode-durable-shell-ref\n"),
        "subsequent CodeMode execution did not recover exact stdout: {value:?}"
    );
}

#[test]
fn full_artifact_durability_matrix_includes_same_call_shell_stdout_expand() {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("matrix.txt"), "matrix-file-bytes\n").unwrap();
    let cache_path = work.path().join("recovery-cache.json");
    let plan = r#"
        const read = zero.read("matrix.txt");
        const tree = zero.tree(".", { depth: 1 });
        const shell = zero.shell("printf 'matrix-shell-bytes\n'");
        const compact = zero.token.compact("matrix-plan-bytes");
        const shellNow = zero.token.expand(shell.stdout_ref);
        return { refs: [read.ref, tree.ref, shell.stdout_ref, compact.ref], shell_now: shellNow.text || shellNow };
    "#;
    let run = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            cache_path: Some(cache_path.clone()),
            max_wall_ms: 30_000,
            hard_max_wall_ms: 30_000,
            ..Default::default()
        },
    );
    assert_eq!(run.status, CodeModeStatus::Completed, "{:?}", run.error);
    let value = run.value.unwrap();
    assert_eq!(value["shell_now"].as_str(), Some("matrix-shell-bytes\n"));
    let refs = value["refs"].as_array().unwrap();
    assert_eq!(refs.len(), 4);
    let mut restarted = tokenzero_recovery::RecoveryStore::new(Some(cache_path));
    for ref_id in refs {
        let ref_id = ref_id.as_str().expect("artifact ref");
        let expanded = restarted.expand(ref_id, None, None, None, None, None);
        assert!(
            expanded.found,
            "durability matrix lost {ref_id}: {}",
            expanded.reason
        );
        assert!(
            !expanded.content.is_empty(),
            "durability matrix stored empty {ref_id}"
        );
    }
}

#[test]
fn builtin_recipe_registry_is_discoverable_and_all_ten_fit_envelopes() {
    let work = tempfile::tempdir().unwrap();
    let file = work.path().join("sample.txt");
    std::fs::write(&file, "needle\nsecond line\n").unwrap();
    let path = serde_json::to_string(work.path()).unwrap();
    let file_path = serde_json::to_string(&file).unwrap();
    let plans = [
        ("read_head", format!(r#"return zero.run("read_head", {{path: {file_path}}})"#)),
        ("find_bounded", format!(r#"return zero.run("find_bounded", {{pattern: "needle", path: {path}}})"#)),
        ("grep_bounded", format!(r#"return zero.run("grep_bounded", {{pattern: "needle", path: {path}}})"#)),
        ("expand_head", r#"return zero.ingest("recipe payload").then(stored => zero.run("expand_head", {ref: stored.ref}))"#.to_string()),
        ("tree_shallow", format!(r#"return zero.run("tree_shallow", {{path: {path}}})"#)),
        ("glob_bounded", format!(r#"return zero.run("glob_bounded", {{pattern: "*.txt", path: {path}}})"#)),
        ("shell_quiet", r#"return zero.run("shell_quiet", {command: "printf recipe"})"#.to_string()),
        ("ingest_text", r#"return zero.run("ingest_text", {text: "recipe payload"})"#.to_string()),
        ("recall_top", r#"return zero.run("recall_top", {query: "recipe payload"})"#.to_string()),
        ("repo_snapshot", format!(r#"return zero.run("repo_snapshot", {{path: {path}, file: {file_path}}})"#)),
    ];
    assert_eq!(plans.len(), 10);
    for (name, plan) in plans {
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
            "{name}: {:?}",
            result.error
        );
        let definition = super::recipe_registry::get(name).unwrap();
        assert!(
            result.telemetry.visible_tokens <= definition.envelope_tokens(),
            "{name}: measured {} > envelope {}",
            result.telemetry.visible_tokens,
            definition.envelope_tokens(),
        );
    }

    let listed = execute_codemode_with_options("return zero.list()", Default::default());
    assert_eq!(
        listed.status,
        CodeModeStatus::Completed,
        "{:?}",
        listed.error
    );
    assert_eq!(listed.value.as_ref().unwrap().as_array().unwrap().len(), 10);
    let described = execute_codemode_with_options(
        r#"return zero.describeRecipe("read_head")"#,
        Default::default(),
    );
    assert_eq!(
        described.status,
        CodeModeStatus::Completed,
        "{:?}",
        described.error
    );
    assert_eq!(
        described.value.as_ref().unwrap()["registry_version"],
        "1.0.0"
    );
}

#[test]
fn builtin_recipe_rejects_envelope_above_declared_budget() {
    let result = execute_codemode_with_options(
        r#"return zero.run("read_head", {path: "Cargo.toml"})"#,
        CodeModeOptions {
            max_visible_tokens: 511,
            ..Default::default()
        },
    );
    assert_eq!(result.status, CodeModeStatus::Error);
    let message = &result.error.as_ref().unwrap().message;
    assert!(
        message.contains("recipe_budget_exceeded") || message.contains("envelope 512"),
        "{message}"
    );
}
