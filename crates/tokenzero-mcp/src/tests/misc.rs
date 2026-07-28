use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

#[test]
fn engine_construction_reclaims_orphan_tmp_and_aged_spills() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let set_age = |path: &Path, secs: u64| {
        fs::OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(secs))
            .unwrap();
    };
    let orphan_tmp = dir.path().join("recovery-cache.json.999.dead.tmp");
    fs::write(&orphan_tmp, "orphan").unwrap();
    set_age(&orphan_tmp, 2 * 60 * 60);
    let spill_dir = shell_spill_dir(&cache);
    fs::create_dir_all(&spill_dir).unwrap();
    let aged_spill = spill_dir.join("tokenzero-1-1-stdout.log");
    fs::write(&aged_spill, "spill").unwrap();
    set_age(&aged_spill, 48 * 60 * 60);
    let fresh_spill = spill_dir.join("tokenzero-2-2-stdout.log");
    fs::write(&fresh_spill, "spill").unwrap();

    let _engine = TokenZeroEngine::new(EngineConfig {
        cache_path: cache,
        ..EngineConfig::for_root(dir.path())
    });

    assert!(!orphan_tmp.exists(), "orphan tmp must be swept on startup");
    assert!(!aged_spill.exists(), "aged spill must be pruned on startup");
    assert!(fresh_spill.exists(), "fresh spill must survive startup");
}

#[test]
fn cache_pack_is_daemonless_and_deterministic() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("AGENTS.md"), "stable instructions\n").unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let first = engine.cache_pack("agent");
    let second = engine.cache_pack("agent");

    assert_eq!(first.status, "ok");
    assert_eq!(second.status, "ok");
    assert_eq!(first.telemetry.as_ref().unwrap()["daemon_required"], false);
    assert_eq!(
        first.telemetry.as_ref().unwrap()["content_digest"],
        second.telemetry.as_ref().unwrap()["content_digest"]
    );
    assert_eq!(
        second.telemetry.as_ref().unwrap()["invalidation_reason"],
        "unchanged"
    );
    assert!(first.refs.iter().any(|row| row.kind == "stable_prefix"));
    assert!(first.refs.iter().any(|row| row.kind == "volatile_tail"));
    let expected_ref_tokens = exact_ref_token_count(&first.refs);
    assert_eq!(
        first.accounting.as_ref().unwrap().exact_ref_tokens,
        Some(expected_ref_tokens)
    );
}

#[test]
fn response_never_advertises_a_ref_evicted_during_its_own_persist() {
    // A payload larger than the cache budget is evicted by the persist that
    // the serving call itself runs; the advertised refs must disappear with
    // it rather than dangle.
    let config = tokenzero_recovery::RecoveryConfig {
        max_bytes: 64,
        ..tokenzero_recovery::RecoveryConfig::default()
    };
    let mut store = RecoveryStore::with_config(None, config);
    let oversized = "x".repeat(4096);
    let stored = store.store_payload_deferred(&oversized, ContentType::Unknown, None, None, None);
    store.persist_pending().unwrap();

    let mut refs = vec![
        ref_record("blob", stored.blob_ref.clone(), oversized.len()),
        ref_record("file", stored.file_ref.clone(), oversized.len()),
    ];
    assert!(!prune_dead_refs(&store, &mut refs));
    assert!(
        refs.is_empty(),
        "refs evicted by the persist must not be advertised"
    );
}

#[test]
fn tool_metrics_persist_across_engine_instances() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "hello metrics\n").unwrap();

    {
        let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
        call_tool(
            &engine,
            "read",
            &json!({ "path": file.display().to_string() }),
            None,
        )
        .unwrap();
    }

    // A fresh engine on the same root rehydrates cumulative counters from the sidecar.
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let snap = engine.tool_metrics_snapshot();
    assert!(
        snap["cumulative"]["tools"]["read"]["calls"]
            .as_u64()
            .unwrap()
            >= 1,
        "cumulative counters persist across sessions via the sidecar"
    );
    assert!(
        snap["session"]["tools"].get("read").is_none(),
        "a fresh process starts with empty session counters"
    );
}

#[test]
fn report_tool_issue_accepts_zero_execute_via_mcp_dispatch() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::Classic;
    let engine = TokenZeroEngine::new(config);
    let ok = call_tool(
        &engine,
        "report_tool_issue",
        &json!({
            "tool": "zero_execute",
            "summary": "expand X0 for fz blob under foreign root",
            "detail": "wqw.6 field"
        }),
        None,
    )
    .expect("zero_execute must be reportable");
    let text = ok.to_string();
    assert!(
        text.contains("accepted") || text.contains("zero_execute"),
        "{text}"
    );
    assert!(crate::is_reportable_tool_name("zero_execute"));
    assert!(crate::is_reportable_tool_name("zerostack"));
    assert!(crate::is_reportable_tool_name("tz_execute_code"));
}

#[test]
fn classic_surface_rejects_tz_execute_code() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::Classic;
    let engine = TokenZeroEngine::new(config);
    let err = call_tool(
        &engine,
        "tz_execute_code",
        &json!({ "plan": "return 1" }),
        None,
    )
    .expect_err("Classic must not dispatch CodeMode execute");
    let msg = err.message_text();
    assert!(
        msg.contains("unknown") || msg.to_ascii_lowercase().contains("tz_execute_code"),
        "{msg}"
    );
}

#[test]
#[cfg(feature = "surface-codemode")]
fn codemode_mcp_wire_preserves_canonical_refs() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Write-Output canonical-wire-ref"
    } else {
        "printf 'canonical-wire-ref\\n'"
    };
    let plan = format!(
        "const a = await zero.token.shell({}); return a.stdout_ref;",
        serde_json::to_string(command).unwrap()
    );
    let wire = call_tool(
        &engine,
        "tz_execute_code",
        &json!({ "plan": plan, "root": dir.path().display().to_string() }),
        None,
    )
    .expect("CodeMode shell call");
    let text = wire.to_string();
    assert!(text.contains("tz://blob/"), "{text}");
    assert!(
        !text.contains("tz://s/"),
        "CodeMode MCP wire must not expose replay-unsafe session aliases: {text}"
    );
}

#[test]
fn mcp_execute_root_with_workspace_markers_requires_allowlist() {
    // tokenzero-nbl3: workspace-evidence markers (.git, Cargo.toml, ...,
    // CHANGELOG.md) must not bypass the server allowlist (F-MCP-011 /
    // F-INT-010). A marker-present foreign root is refused just like a
    // markerless one.
    use tokenzero_core::McpToolSurface;
    let server = tempdir().unwrap();
    let foreign = tempdir().unwrap();
    std::fs::write(
        foreign.path().join("CHANGELOG.md"),
        "FOREIGN-WORKSPACE-MARKER\n",
    )
    .unwrap();
    let mut config = EngineConfig::for_root(server.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    let err = call_tool(
        &engine,
        "tz_execute_code",
        &json!({
            "plan": "return zero.read('CHANGELOG.md')",
            "root": foreign.path().display().to_string()
        }),
        None,
    )
    .expect_err("marker-present foreign root must be refused");
    let msg = err.message_text();
    assert!(
        msg.contains("path_not_allowed") || msg.contains("outside"),
        "{msg}"
    );
}

#[test]
#[cfg(feature = "surface-codemode")]
fn mcp_execute_root_rebase_inside_allowed_tree_is_accepted() {
    // Legitimate rebase stays: a root nested inside an allowlisted tree is
    // accepted and unions into the execution workspace.
    use tokenzero_core::McpToolSurface;
    let server = tempdir().unwrap();
    let nested = server.path().join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(nested.join("marker.txt"), "NESTED-ALLOWED\n").unwrap();
    let mut config = EngineConfig::for_root(server.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    let ok = call_tool(
        &engine,
        "tz_execute_code",
        &json!({
            "plan": "return zero.read('marker.txt')",
            "root": nested.display().to_string()
        }),
        None,
    )
    .expect("rebase inside an allowed tree should succeed");
    assert!(
        ok.to_string().contains("NESTED-ALLOWED"),
        "nested read result: {ok}"
    );
}

#[test]
fn codemode_report_tool_issue_is_not_permanently_locked() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    let ok = call_tool(
        &engine,
        "tz_report_tool_issue",
        &json!({
            "tool": "zero_execute",
            "summary": "codemode field report must be accepted"
        }),
        None,
    )
    .expect("report_tool_issue must be NotGated on CodeMode");
    assert!(ok.to_string().contains("accepted") || ok.to_string().contains("zero_execute"));
}

#[test]
fn mcp_execute_root_cannot_escape_server_allowlist() {
    use tokenzero_core::McpToolSurface;
    let server = tempdir().unwrap();
    let foreign = tempdir().unwrap();
    std::fs::write(
        foreign.path().join("secret.txt"),
        "ALLOWLIST-BYPASS-MARKER\n",
    )
    .unwrap();
    let mut config = EngineConfig::for_root(server.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    let err = call_tool(
        &engine,
        "tz_execute_code",
        &json!({
            "plan": "return zero.read('secret.txt')",
            "root": foreign.path().display().to_string()
        }),
        None,
    )
    .expect_err("foreign root must be refused");
    let msg = err.message_text();
    assert!(
        msg.contains("path_not_allowed") || msg.contains("outside"),
        "{msg}"
    );
}

#[test]
fn plan_string_expand_does_not_unlock_without_expand_op() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    assert!(engine.surface_health().is_healthy());
    let _ = call_tool(
        &engine,
        "tz_execute_code",
        &json!({
            "plan": "const note = \"please expand later\"; throw new Error(\"boom\")"
        }),
        None,
    );
    assert!(
        engine.surface_health().is_healthy(),
        "plan text mentioning expand must not unlock recovery"
    );
}

#[test]
#[cfg(feature = "surface-codemode")]
fn shared_health_unlocks_after_plan_expand_miss() {
    use tokenzero_core::McpToolSurface;
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.tool_surface = McpToolSurface::CodeMode;
    let engine = TokenZeroEngine::new(config);
    assert!(engine.surface_health().is_healthy());
    let _ = call_tool(
        &engine,
        "tz_execute_code",
        &json!({
            "plan": "return await zero.expand(\"tz://blob/nonexistent123\")"
        }),
        None,
    );
    assert!(
        !engine.surface_health().is_healthy(),
        "expand miss inside plan must update shared session health"
    );
    assert!(
        engine
            .surface_health()
            .allow_tool_call(McpToolSurface::CodeMode, "tz_expand")
            .is_ok()
    );
}

#[cfg(feature = "surface-codemode")]
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

#[cfg(feature = "surface-codemode")]
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
