use super::{CodeModeOptions, CodeModeStatus, execute_codemode_with_options};

// Serializes tests that mutate TOKENZERO_CHANNEL_SEPARATION (vz89.11).
static CHANNEL_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_channel_gate<R>(value: Option<&str>, f: impl FnOnce() -> R) -> R {
    let _guard = CHANNEL_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: gated by CHANNEL_ENV_LOCK; no other test reads this var.
    unsafe {
        match value {
            Some(v) => std::env::set_var(tokenzero_core::CHANNEL_SEPARATION_ENV, v),
            None => std::env::remove_var(tokenzero_core::CHANNEL_SEPARATION_ENV),
        }
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    // SAFETY: same lock still held; restores the unset default.
    unsafe { std::env::remove_var(tokenzero_core::CHANNEL_SEPARATION_ENV) };
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

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
        "const f = () => __tz_call('zero.edit', ['file.txt', []]); return f();",
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
fn edit_denial_is_canonical_not_lexical() {
    // Alias/computed/obfuscated spellings all resolve to the canonical edit
    // op at the dispatch bridge; every one must be denied (tokenzero-b452).
    for plan in [
        "return __tz_call('zero.edit', ['f.txt', []]);",
        "const c = __tz_call; return c('tz_edit', ['f.txt', []]);",
        "const c = __tz_call; return c('edit', ['f.txt', []]);",
        "return __tz_call('zero.token.edit', ['f.txt', []]);",
    ] {
        let result = execute_codemode_with_options(
            plan,
            CodeModeOptions {
                root: Some(std::env::temp_dir()),
                ..CodeModeOptions::default()
            },
        );
        assert_eq!(
            result.status,
            CodeModeStatus::Error,
            "plan should be denied: {plan}"
        );
        let message = &result.error.as_ref().unwrap().message;
        assert!(
            message.contains("mutating binding denied"),
            "expected canonical dispatch denial, got: {message} (plan: {plan})"
        );
    }
}

#[test]
fn harmless_edit_keywords_in_strings_do_not_fail() {
    // Quoted prose mentioning the edit surface never dispatches it; the plan
    // must complete (tokenzero-b452 false-positive fix).
    let result = execute_codemode_with_options(
        "const s = \"zero.edit.edit( tz_edit .edit( mutating binding denied\"; return s.length;",
        CodeModeOptions {
            root: Some(std::env::temp_dir()),
            ..CodeModeOptions::default()
        },
    );
    assert_eq!(
        result.status,
        CodeModeStatus::Completed,
        "harmless literal plan should complete: {:?}",
        result.error
    );
}

#[test]
fn unknown_edit_shaped_name_fails_closed() {
    // No binding/registry entry exists for this spelling: it must fail closed
    // with an unknown-name error, never reach an edit executor (tokenzero-b452).
    let result = execute_codemode_with_options(
        "return __tz_call('zero.fs.edit', ['f.txt', []]);",
        CodeModeOptions {
            root: Some(std::env::temp_dir()),
            ..CodeModeOptions::default()
        },
    );
    assert_eq!(result.status, CodeModeStatus::Error);
    let message = &result.error.as_ref().unwrap().message;
    assert!(
        !message.contains("mutating binding denied"),
        "unknown names are not the edit family: {message}"
    );
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
fn pn93_sub_threshold_strings_inline_fully_with_ref_attached() {
    // pn93: a ~2KB plan value is under the ref-first inline budget and must
    // come back as the full string, not {ref, 32-char preview}; the ref is
    // still minted into result.refs for exact recovery.
    let work = tempfile::tempdir().unwrap();
    let result = execute_codemode_with_options(
        "return 'abcdefghijklmnopqrstuvwxyz0123456789'.repeat(40)",
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
    let text = value
        .as_str()
        .unwrap_or_else(|| panic!("2KB value must inline as a string, got: {value}"));
    assert_eq!(text.len(), 36 * 40);
    assert!(
        result.refs.iter().any(|r| r.starts_with("tz://")),
        "inlined value must still attach an exact-recovery ref: {:?}",
        result.refs
    );
}

#[test]
fn pn93_over_budget_values_stay_ref_first() {
    // pn93: past the inline budget the ref-first contract is unchanged:
    // {ref, preview, chars, expand} plus the ref in result.refs.
    let work = tempfile::tempdir().unwrap();
    let result = execute_codemode_with_options(
        // varied tokens; a repeated pattern BPE-compresses under the budget
        "return Array.from({length: 3000}, (_, i) => i.toString(36)).join('|')",
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
        .unwrap_or_else(|| panic!("over-budget value must be ref-first, got: {value}"));
    assert!(ref_id.starts_with("tz://"));
    assert!(value["preview"].as_str().is_some());
    assert!(result.refs.iter().any(|r| r == ref_id));
}

// vz89.10 session exposure ledger: deterministic mid-entropy payload between
// the 64-token ref floor and the explicit 2048-token inline budget.
const VZ89_10_PLAN: &str = "return Array.from({length: 600}, (_, i) => i.toString(36)).join('|')";

fn vz89_10_options(root: &std::path::Path) -> CodeModeOptions {
    CodeModeOptions {
        root: Some(root.to_path_buf()),
        cache_path: Some(root.join("recovery-cache.json")),
        ref_first_budget: 2048,
        ..Default::default()
    }
}

#[test]
fn vz89_10_second_reference_in_scope_sends_ref_not_bytes() {
    let work = tempfile::tempdir().unwrap();
    let first = execute_codemode_with_options(VZ89_10_PLAN, vz89_10_options(work.path()));
    assert_eq!(first.status, CodeModeStatus::Completed, "{:?}", first.error);
    let text = first
        .value
        .as_ref()
        .unwrap()
        .as_str()
        .expect("first reference must inline the full string")
        .to_string();
    let ref_id = first
        .refs
        .iter()
        .find(|r| r.starts_with("tz://blob/"))
        .expect("inline value must still mint its ref")
        .clone();

    let second = execute_codemode_with_options(VZ89_10_PLAN, vz89_10_options(work.path()));
    assert_eq!(
        second.status,
        CodeModeStatus::Completed,
        "{:?}",
        second.error
    );
    let value = second.value.as_ref().unwrap();
    assert!(
        value.as_str().is_none(),
        "held bytes must not re-inline, got: {value}"
    );
    assert_eq!(value["session_exposure"].as_str(), Some("held"), "{value}");
    assert_eq!(value["ref"].as_str(), Some(ref_id.as_str()));
    assert!(value["expand"].as_str().unwrap().contains(&ref_id));
    assert!(
        serde_json::to_string(value).unwrap().len() < text.len(),
        "ref-only envelope must be smaller than the payload"
    );
}

#[test]
fn vz89_10_different_scope_inlines_freshly() {
    let work_a = tempfile::tempdir().unwrap();
    let work_b = tempfile::tempdir().unwrap();
    let first = execute_codemode_with_options(VZ89_10_PLAN, vz89_10_options(work_a.path()));
    assert!(first.value.as_ref().unwrap().as_str().is_some());
    let second = execute_codemode_with_options(VZ89_10_PLAN, vz89_10_options(work_b.path()));
    assert_eq!(
        second.status,
        CodeModeStatus::Completed,
        "{:?}",
        second.error
    );
    assert!(
        second.value.as_ref().unwrap().as_str().is_some(),
        "a new session scope must receive the full bytes, got: {:?}",
        second.value
    );
}

#[test]
fn vz89_10_held_ref_expand_succeeds_and_marks_session_replay() {
    let work = tempfile::tempdir().unwrap();
    let first = execute_codemode_with_options(VZ89_10_PLAN, vz89_10_options(work.path()));
    let ref_id = first
        .refs
        .iter()
        .find(|r| r.starts_with("tz://blob/"))
        .expect("ref minted")
        .clone();
    let cache_path = work.path().join("recovery-cache.json");
    // Fresh engine on the same session scope: expand of a session-known ref
    // is always available and tagged as a replay (recovery accounting class).
    let engine = tokenzero_engine::TokenZeroEngine::new(tokenzero_engine::EngineConfig {
        cache_path,
        ..tokenzero_engine::EngineConfig::for_root(work.path())
    });
    let response = engine.expand_with_params(tokenzero_engine::expand_params::ExpandParams {
        ref_id: ref_id.clone(),
        raw: true,
        ..Default::default()
    });
    assert!(
        response.error.is_none(),
        "held ref must expand: {:?}",
        response.error
    );
    assert_eq!(
        response
            .telemetry
            .as_ref()
            .and_then(|t| t.get("session_exposure_replay"))
            .and_then(serde_json::Value::as_u64),
        Some(1),
        "first replay of a held ref must be tagged: {:?}",
        response.telemetry
    );
    // A ref the session never saw is ordinary recovery, not a replay.
    let foreign = engine.expand_with_params(tokenzero_engine::expand_params::ExpandParams {
        ref_id: "tz://blob/0000000000000000000000000000000000000000000000000000000000000000"
            .to_string(),
        raw: true,
        ..Default::default()
    });
    assert!(foreign.error.is_some(), "foreign digest must not resolve");
}

#[test]
fn vz89_11_channels_absent_by_default() {
    with_channel_gate(None, || {
        let work = tempfile::tempdir().unwrap();
        let result = execute_codemode_with_options(
            "return { ok: true }",
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
        let serialized = serde_json::to_string(&result).unwrap();
        assert!(
            !serialized.contains("channels"),
            "gate off must be byte-identical to the pre-gate envelope: {serialized}"
        );
    });
}

#[test]
fn vz89_11_channels_present_when_gated() {
    with_channel_gate(Some("1"), || {
        let work = tempfile::tempdir().unwrap();
        let completed = execute_codemode_with_options(
            "return { ok: true }",
            CodeModeOptions {
                root: Some(work.path().to_path_buf()),
                ..Default::default()
            },
        );
        assert_eq!(
            completed.status,
            CodeModeStatus::Completed,
            "{:?}",
            completed.error
        );
        let value = serde_json::to_value(&completed).unwrap();
        let channels = value
            .get("channels")
            .unwrap_or_else(|| panic!("gated response must carry channels: {value}"));
        assert_eq!(channels["action"].as_str(), Some("codemode.code"));
        assert!(
            channels["status_line"]
                .as_str()
                .unwrap_or_default()
                .starts_with("Executed code plan"),
            "deterministic status line from the receipt: {channels}"
        );
        assert!(
            channels.get("user_message").is_some(),
            "nullable user_message key must be present"
        );
        assert!(channels["user_message"].is_null());

        // Error receipts get a deterministic failure status line too.
        let failed = execute_codemode_with_options(
            "return 1",
            CodeModeOptions {
                root: Some(work.path().to_path_buf()),
                max_code_bytes: 1,
                ..Default::default()
            },
        );
        assert_eq!(failed.status, CodeModeStatus::Error);
        let failed_value = serde_json::to_value(&failed).unwrap();
        let failed_channels = failed_value
            .get("channels")
            .expect("error envelope carries channels when gated");
        assert_eq!(
            failed_channels["status_line"].as_str(),
            Some("Plan failed (validation)")
        );
        assert!(failed_channels["user_message"].is_null());
    });
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
fn promise_all_runs_independent_shells_concurrently() {
    // tokenzero-codemode-parallel-broken-z28: Promise.all must fan out host ops.
    let work = tempfile::tempdir().unwrap();
    let plan = r#"
        await Promise.all([
          zero.shell('touch promise-all-a; i=0; while [ ! -f promise-all-b ] || [ ! -f promise-all-c ]; do i=$((i+1)); [ "$i" -lt 10000 ] || exit 9; sleep 0.02; done'),
          zero.shell('touch promise-all-b; i=0; while [ ! -f promise-all-a ] || [ ! -f promise-all-c ]; do i=$((i+1)); [ "$i" -lt 10000 ] || exit 9; sleep 0.02; done'),
          zero.shell('touch promise-all-c; i=0; while [ ! -f promise-all-a ] || [ ! -f promise-all-b ]; do i=$((i+1)); [ "$i" -lt 10000 ] || exit 9; sleep 0.02; done'),
        ]);
        return { ok: true };
    "#;
    let result = execute_codemode_with_options(
        plan,
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            max_parallel_width: 3,
            max_wall_ms: 15000,
            hard_max_wall_ms: 15000,
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
fn shell_result_exposes_documented_top_level_owner_ref() {
    // yevj: the catalog documents `ref` on every op result; shell results
    // previously exposed only <kind>_ref keys (Grok session 019fa59e hit
    // `undefined property: skill.ref`). The owner ref is the combined blob
    // and must expand to the exact combined bytes.
    let work = tempfile::tempdir().unwrap();
    let cache_path = work.path().join("recovery-cache.json");
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Write-Output owner-ref-probe"
    } else {
        "printf 'owner-ref-probe\\n'"
    };
    let run = execute_codemode_with_options(
        &format!(
            "const r = await zero.token.shell({}); return {{ ref: r.ref, combined: r.combined_ref, stdout: r.stdout_ref }};",
            serde_json::to_string(command).unwrap()
        ),
        CodeModeOptions {
            root: Some(work.path().to_path_buf()),
            cache_path: Some(cache_path.clone()),
            ..Default::default()
        },
    );
    assert_eq!(run.status, CodeModeStatus::Completed, "{:?}", run.error);
    let value = run.value.as_ref().unwrap();
    let owner = value["ref"].as_str().expect("shell result exposes .ref");
    assert!(
        owner.starts_with("tz://blob/"),
        "owner ref is canonical: {owner}"
    );
    assert_eq!(
        Some(owner),
        value["combined"].as_str(),
        "owner ref is the combined-stream blob"
    );

    let expanded = execute_codemode_with_options(
        &format!(
            "return await zero.token.expand({})",
            serde_json::to_string(owner).unwrap()
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
    let text = expanded
        .value
        .as_ref()
        .and_then(|value| {
            value
                .as_str()
                .or_else(|| value.get("text").and_then(serde_json::Value::as_str))
        })
        .unwrap_or_default();
    assert!(
        text.contains("owner-ref-probe"),
        "owner ref expands to the combined shell bytes: {text}"
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
