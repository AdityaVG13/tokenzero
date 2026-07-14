use super::*;
use super::support::*;

#[test]
fn engine_construction_reclaims_orphan_tmp_and_aged_spills() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let set_age = |path: &Path, secs: u64| {
        fs::OpenOptions::new().write(true).open(path).unwrap()
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
    let _engine = TokenZeroEngine::new(EngineConfig { cache_path: cache, ..EngineConfig::for_root(dir.path()) });
    assert!(!orphan_tmp.exists(), "orphan tmp must be swept on startup");
    assert!(!aged_spill.exists(), "aged spill must be pruned on startup");
    assert!(fresh_spill.exists(), "fresh spill must survive startup");
}

#[test]
fn cache_pack_is_daemonless_and_deterministic() {
    let (dir, engine) = setup_default();
    fs::write(dir.path().join("AGENTS.md"), "stable instructions\n").unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    let first = engine.cache_pack("agent");
    let second = engine.cache_pack("agent");
    assert_status_ok(&first);
    assert_status_ok(&second);
    assert_eq!(first.telemetry.as_ref().unwrap()["daemon_required"], false);
    assert_eq!(first.telemetry.as_ref().unwrap()["content_digest"], second.telemetry.as_ref().unwrap()["content_digest"]);
    assert_eq!(second.telemetry.as_ref().unwrap()["invalidation_reason"], "unchanged");
    assert!(first.refs.iter().any(|row| row.kind == "stable_prefix"));
    assert!(first.refs.iter().any(|row| row.kind == "volatile_tail"));
    assert_eq!(first.accounting.as_ref().unwrap().exact_ref_tokens, Some(exact_ref_token_count(&first.refs)));
}

#[test]
fn response_never_advertises_a_ref_evicted_during_its_own_persist() {
    let config = tokenzero_recovery::RecoveryConfig { max_bytes: 64, ..tokenzero_recovery::RecoveryConfig::default() };
    let mut store = RecoveryStore::with_config(None, config);
    let oversized = "x".repeat(4096);
    let stored = store.store_payload_deferred(&oversized, ContentType::Unknown, None, None, None);
    store.persist_pending().unwrap();
    let mut refs = vec![
        ref_record("blob", stored.blob_ref.clone(), oversized.len()),
        ref_record("file", stored.file_ref.clone(), oversized.len()),
    ];
    assert!(!prune_dead_refs(&store, &mut refs));
    assert!(refs.is_empty(), "refs evicted by the persist must not be advertised");
}

#[test]
fn tool_metrics_persist_across_engine_instances() {
    let (dir, file, _) = setup_file("hello.txt", "hello metrics\n");
    call_tool(&default_engine(dir.path()), "read", &json!({ "path": file.display().to_string() }), None).unwrap();
    let snap = default_engine(dir.path()).tool_metrics_snapshot();
    assert!(snap["cumulative"]["tools"]["read"]["calls"].as_u64().unwrap() >= 1);
    assert!(snap["session"]["tools"].get("read").is_none());
}

#[test]
fn classic_surface_report_and_reject_matrix() {
    let (_dir, classic) = setup_engine(|root| {
        let mut c = EngineConfig::for_root(root);
        c.tool_surface = tokenzero_core::McpToolSurface::Classic;
        c
    });
    let ok = call_tool(
        &classic, "report_tool_issue",
        &json!({"tool": "zero_execute", "summary": "expand X0 for fz blob under foreign root", "detail": "wqw.6 field"}),
        None,
    ).expect("zero_execute must be reportable");
    assert!(ok.to_string().contains("accepted") || ok.to_string().contains("zero_execute"));
    for name in ["zero_execute", "zerostack", "tz_execute_code"] {
        assert!(crate::is_reportable_tool_name(name));
    }
    let err = call_tool(&classic, "tz_execute_code", &json!({ "plan": "return 1" }), None)
        .expect_err("Classic must not dispatch CodeMode execute");
    let msg = err.message_text();
    assert!(msg.contains("unknown") || msg.to_ascii_lowercase().contains("tz_execute_code"), "{msg}");
}

#[test]
fn mcp_execute_root_honors_foreign_workspace_and_denies_outside() {
    let server = tempdir().unwrap();
    let foreign = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(foreign.path().join("CHANGELOG.md"), "FOREIGN-WORKSPACE-MARKER\n").unwrap();
    fs::write(outside.path().join("secret.txt"), "SECRET\n").unwrap();
    let engine = codemode_engine(server.path());
    let foreign_root = foreign.path().display().to_string();

    let rel = call_tool(
        &engine,
        "tz_execute_code",
        &json!({"plan": "return zero.token.read(\"CHANGELOG.md\")", "root": foreign_root.clone()}),
        None,
    )
    .expect("relative read should succeed");
    assert!(rel.to_string().contains("FOREIGN-WORKSPACE-MARKER"), "relative read result: {rel}");

    let abs_plan = format!(
        "return zero.token.read({});",
        serde_json::to_string(&foreign.path().join("CHANGELOG.md").display().to_string()).unwrap()
    );
    let abs = call_tool(
        &engine,
        "tz_execute_code",
        &json!({"plan": abs_plan, "root": foreign_root.clone()}),
        None,
    )
    .expect("absolute read should succeed");
    assert!(abs.to_string().contains("FOREIGN-WORKSPACE-MARKER"), "absolute read result: {abs}");

    let secret_plan = format!(
        "return zero.token.read({});",
        serde_json::to_string(&outside.path().join("secret.txt").display().to_string()).unwrap()
    );
    let denied = call_tool(
        &engine,
        "tz_execute_code",
        &json!({"plan": secret_plan, "root": foreign_root}),
        None,
    )
    .expect("outside-root failure must use the structured CodeMode result");
    let denied_text = denied.to_string();
    assert!(
        denied_text.contains("path_not_allowed") || denied_text.contains("outside allowed roots"),
        "{denied_text}"
    );
}

#[test]
fn mcp_execute_root_cannot_escape_server_allowlist() {
    let server = tempdir().unwrap();
    let foreign = tempdir().unwrap();
    fs::write(foreign.path().join("secret.txt"), "ALLOWLIST-BYPASS-MARKER\n").unwrap();
    let engine = codemode_engine(server.path());
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
    assert!(msg.contains("path_not_allowed") || msg.contains("outside"), "{msg}");
}

#[test]
fn codemode_report_and_health_matrix() {
    let (_dir, engine) = setup_engine(|root| {
        let mut c = EngineConfig::for_root(root);
        c.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
        c
    });
    let ok = call_tool(
        &engine, "tz_report_tool_issue",
        &json!({"tool": "zero_execute", "summary": "codemode field report must be accepted"}),
        None,
    ).expect("report_tool_issue must be NotGated on CodeMode");
    assert!(ok.to_string().contains("accepted") || ok.to_string().contains("zero_execute"));

    assert!(engine.surface_health().is_healthy());
    let _ = call_tool(&engine, "tz_execute_code", &json!({"plan": "const note = \"please expand later\"; throw new Error(\"boom\")"}), None);
    assert!(engine.surface_health().is_healthy(), "plan text mentioning expand must not unlock recovery");

    let (_dir2, engine2) = setup_engine(|root| {
        let mut c = EngineConfig::for_root(root);
        c.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
        c
    });
    assert!(engine2.surface_health().is_healthy());
    let _ = call_tool(&engine2, "tz_execute_code", &json!({"plan": "return await zero.expand(\"tz://blob/nonexistent123\")"}), None);
    assert!(!engine2.surface_health().is_healthy(), "expand miss inside plan must update shared session health");
    assert!(engine2.surface_health().allow_tool_call(tokenzero_core::McpToolSurface::CodeMode, "tz_expand").is_ok());
}
