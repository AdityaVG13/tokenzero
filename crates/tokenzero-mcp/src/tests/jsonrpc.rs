use super::*;
use super::support::*;
use std::time::Duration;

#[test]
fn malformed_json_returns_error_and_does_not_panic() {
    let (_dir, engine) = setup_default();
    let parsed = rpc(&engine, "{bad");
    assert_structured_error(&parsed, -32700, None);
    assert!(
        parsed["error"]["message"].as_str().unwrap().contains("Parse error"),
        "{parsed:#}"
    );
}

#[test]
fn tools_list_includes_aliases_with_stub_schema() {
    let specs = tool_specs();
    let by_name = |name: &str| -> &ToolSpec { specs.iter().find(|s| s.name == name).unwrap() };
    for (canonical, alias) in [("tz_read", "read"), ("tz_grep", "grep"), ("tz_glob", "glob")] {
        let c = by_name(canonical);
        let a = by_name(alias);
        assert!(
            c.input_schema["properties"].is_object(),
            "{canonical} must have real schema properties"
        );
        assert_eq!(
            a.input_schema,
            json!({"type": "object"}),
            "{alias} must advertise a stub schema"
        );
    }
}

#[test]
fn tools_call_rejects_mixed_type_arrays() {
    let (_dir, engine) = setup_default();
    let cases: &[(&str, &str, Value)] = &[
        ("rewrite", "argv", json!(["printf", 7, "ignored"])),
        ("rewrite", "argv", json!(["printf", null, "ignored"])),
        ("rewrite", "argv", json!(["printf", {"bad": true}, "ignored"])),
        ("rewrite", "argv", json!(["printf", false, "ignored"])),
        ("read", "path", json!(["missing.txt", 1])),
        ("read", "path", json!(["missing.txt", null])),
        ("read", "path", json!(["missing.txt", {"bad": true}])),
        ("read", "path", json!(["missing.txt", false])),
    ];
    for (tool, field, invalid) in cases {
        let parsed = tools_call(&engine, json!("bad"), tool, json!({ *field: invalid }));
        assert_eq!(parsed["error"]["code"], -32602, "{parsed:#}");
        assert_eq!(parsed["error"]["data"]["kind"], "invalid_params");
        assert!(
            parsed["error"]["data"]["reason"]
                .as_str()
                .unwrap()
                .contains("array of strings"),
            "{parsed:#}"
        );
    }
}

#[test]
fn mcp_idle_timeout_zero_disables_and_large_values_clamp() {
    assert_eq!(mcp_idle_timeout_from_secs(Some(0)), None);
    assert_eq!(
        mcp_idle_timeout_from_secs(Some(1)).unwrap(),
        Duration::from_secs(1)
    );
    assert_eq!(
        mcp_idle_timeout_from_secs(Some(u64::MAX)).unwrap(),
        Duration::from_secs(MAX_MCP_IDLE_TIMEOUT_SECS)
    );
    assert_eq!(mcp_idle_timeout_from_secs(None), None);
    assert_eq!(DEFAULT_MCP_IDLE_TIMEOUT_SECS, 0);
}

#[test]
fn mcp_lists_and_calls_cache_pack_tool() {
    let (dir, engine) = setup_default();
    fs::write(dir.path().join("AGENTS.md"), "stable\n").unwrap();
    let listed = tools_list(&engine, 10);
    let names: Vec<_> = listed["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"tz_cache_pack"));
    assert!(names.contains(&"cache_pack"));

    let read_tool = find_tool(&listed, "tz_read");
    assert!(read_tool["inputSchema"].get("$schema").is_none());
    assert_eq!(read_tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(read_tool["inputSchema"]["required"][0], "path");
    let description = read_tool["description"].as_str().unwrap();
    assert!(!description.is_empty() && description.len() < 300, "{description}");
    assert!(description.contains("tz://"), "{description}");

    let docs = resource_read(&engine, 12, "resource://tokenzero/tools");
    let docs_payload: Value =
        serde_json::from_str(docs["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    let read_doc = find_tool_by_name(docs_payload["tools"].as_array().unwrap(), "tz_read");
    let read_doc_description = read_doc["description"].as_str().unwrap();
    for required_section in [
        "Discovery",
        "When to use",
        "Do / Don't",
        "Examples",
        "Common mistakes",
        "Idempotency",
    ] {
        assert!(
            read_doc_description.contains(required_section),
            "missing {required_section} in {read_doc_description}"
        );
    }

    let alias_tool = find_tool(&listed, "read");
    assert_eq!(alias_tool["inputSchema"], json!({"type": "object"}));
    let alias_doc = find_tool_by_name(docs_payload["tools"].as_array().unwrap(), "read");
    assert_eq!(alias_doc["inputSchema"], read_tool["inputSchema"]);
    assert_eq!(find_tool(&listed, "tz_shell")["inputSchema"]["additionalProperties"], false);
    assert_no_schema_combinators(&listed);

    let called = tools_call(&engine, json!(11), "cache_pack", json!({"scope": "agent"}));
    assert!(called["result"].get("structuredContent").is_none(), "{called}");
    assert!(called["result"]["content"][0]["text"].as_str().unwrap().contains("tz://"));
    let pack = engine.cache_pack("agent");
    assert_eq!(pack.tool, "cache-pack");
    assert_eq!(pack.telemetry.as_ref().unwrap()["daemon_required"], false);
}

#[test]
fn mcp_envelope_is_text_only_by_default() {
    let (dir, engine) = setup_default();
    let response = tools_call(
        &engine,
        json!(1),
        "shell",
        json!({"command": "echo compact-envelope-check"}),
    );
    let result = &response["result"];
    assert!(result.get("structuredContent").is_none(), "{result}");
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("compact-envelope-check"), "{text}");
    assert!(
        text.contains("combined_ref: tz://") || text.contains("refs: tz://"),
        "{text}"
    );

    fs::write(dir.path().join("sample.txt"), "alpha\nbeta\n").unwrap();
    let sample_path =
        serde_json::to_string(&dir.path().join("sample.txt").display().to_string()).unwrap();
    let read = rpc(
        &engine,
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"read","arguments":{{"path":{sample_path}}}}}}}"#
        ),
    );
    let read_text = read["result"]["content"][0]["text"].as_str().unwrap();
    assert!(read_text.contains("alpha"), "{read_text}");
    assert!(read_text.contains("refs: tz://blob/"), "{read_text}");
    assert!(read_text.contains("edit: tz_edit"), "{read_text}");
    assert!(!text.contains("edit: tz_edit"), "{text}");

    let shell = engine.shell(
        "echo compact-envelope-check",
        None, None, Mode::Auto, None, false, None, None, None,
    );
    let cli = tools::compact_cli_envelope(&shell);
    assert_eq!(cli["telemetry"]["command_success"], true);
    assert!(cli["accounting"].is_object(), "{cli}");
    assert!(cli.get("visible").is_none(), "{cli}");
    for pruned in ["argv", "stdout_preview", "stderr_preview", "stdout_capture"] {
        assert!(cli["telemetry"].get(pruned).is_none(), "telemetry.{pruned}: {cli}");
    }
}

#[test]
fn initialize_echoes_supported_stable_protocol() {
    let (_dir, engine) = setup_default();
    let parsed = rpc(
        &engine,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"conformance-client","version":"1.0.0"}}}"#,
    );
    assert_eq!(parsed["result"]["protocolVersion"], "2025-06-18");
    assert!(parsed["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn resource_discovery_and_prompt_lists_are_supported() {
    let (_dir, engine) = setup_default();
    let resources = rpc(
        &engine,
        r#"{"jsonrpc":"2.0","id":2,"method":"resources/list","params":{}}"#,
    );
    let prompts = rpc(
        &engine,
        r#"{"jsonrpc":"2.0","id":3,"method":"prompts/list","params":{}}"#,
    );
    let resource_uris: Vec<_> = resources["result"]["resources"].as_array().unwrap().iter().filter_map(|r| r["uri"].as_str()).collect();
    assert!(resource_uris.contains(&"resource://tokenzero/capabilities"));
    assert!(resource_uris.contains(&"resource://tokenzero/tools"));
    assert_eq!(resources["result"]["resultType"], "complete");
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 0);

    let capabilities = resource_read(&engine, 4, "resource://tokenzero/capabilities");
    let payload: Value = serde_json::from_str(
        capabilities["result"]["contents"][0]["text"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(payload["schema_version"], MCP_SCHEMA_VERSION);
    assert!(payload["tool_clusters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|cluster| cluster["cluster"] == "material"));
    assert!(payload["next_actions"].as_array().unwrap().len() >= 2);
}

#[test]
fn mcp_error_data_guides_unknown_tools_resources_params_methods() {
    let (_dir, engine) = setup_default();
    let unknown_tool = tools_call(&engine, json!(12), "tz_reed", json!({}));
    let tool_data = &unknown_tool["error"]["data"];
    assert_eq!(unknown_tool["error"]["code"], -32602);
    assert_eq!(tool_data["error_type"], "NOT_FOUND");
    assert_eq!(tool_data["recoverable"], true);
    assert_eq!(tool_data["entity_type"], "tool");
    assert_eq!(tool_data["provided"], "tz_reed");
    assert!(tool_data["available_options"].as_array().unwrap().iter().any(|t| t == "tz_read"));
    assert_eq!(tool_data["suggestions"][0]["value"], "tz_read");
    assert!(tool_data["fix_hint"].as_str().unwrap().contains("tools/list"));
    assert_eq!(tool_data["suggested_tool_calls"][0]["method"], "tools/list");

    let unknown_resource = resource_read(&engine, 13, "resource://tokenzero/toolz");
    let resource_data = &unknown_resource["error"]["data"];
    assert_eq!(unknown_resource["error"]["code"], -32602);
    assert_eq!(resource_data["error_type"], "NOT_FOUND");
    assert_eq!(resource_data["recoverable"], true);
    assert_eq!(resource_data["entity_type"], "resource");
    assert!(resource_data["available_options"]
        .as_array()
        .unwrap()
        .iter()
        .any(|uri| uri == "resource://tokenzero/tools"));
    assert_eq!(resource_data["suggestions"][0]["value"], "resource://tokenzero/tools");
    assert!(resource_data["fix_hint"].as_str().unwrap().contains("resources/list"));
    assert_eq!(resource_data["suggested_tool_calls"][0]["method"], "resources/list");

    let missing = rpc(
        &engine,
        r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{}}"#,
    );
    let missing_data = &missing["error"]["data"];
    assert_eq!(missing["error"]["code"], -32602);
    assert_eq!(missing_data["error_type"], "INVALID_ARGUMENT");
    assert_eq!(missing_data["recoverable"], true);
    assert_eq!(missing_data["param"], "name");
    assert!(missing_data["available_options"].as_array().unwrap().iter().any(|t| t == "tz_read"));
    assert!(missing_data["fix_hint"].as_str().unwrap().contains("tools/list"));
    assert_eq!(missing_data["suggested_tool_calls"][0]["method"], "tools/list");

    let unknown_method = rpc(
        &engine,
        r#"{"jsonrpc":"2.0","id":15,"method":"tools/lits","params":{}}"#,
    );
    let method_data = &unknown_method["error"]["data"];
    assert_eq!(unknown_method["error"]["code"], -32601);
    assert_eq!(method_data["error_type"], "NOT_FOUND");
    assert_eq!(method_data["recoverable"], true);
    assert_eq!(method_data["entity_type"], "method");
    assert_eq!(method_data["suggestions"][0]["value"], "tools/list");
    assert!(method_data["available_options"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m == "tools/call"));
    assert_eq!(method_data["suggested_tool_calls"][0]["method"], "server/discover");
}

#[test]
fn mcp_tool_calls_are_pulse_accounted_with_attribution() {
    let (dir, file, engine) = setup_file("sample.txt", "line one\nline two\n");
    let read_response = tools_call(
        &engine,
        json!(7),
        "tz_read",
        json!({"path": file.display().to_string()}),
    );
    let text = read_response["result"]["content"][0]["text"].as_str().unwrap();
    let ref_id = text
        .split_whitespace()
        .find(|word| word.starts_with("tz://blob/"))
        .expect("read response advertises a blob ref")
        .to_string();
    let _ = tools_call(&engine, json!("call-8"), "tz_expand", json!({"ref": ref_id}));
    let _ = tools_call(
        &engine,
        json!("7"),
        "tz_read",
        json!({"path": file.display().to_string(), "fresh": true}),
    );

    let ledger = tokenzero_pulse::default_ledger_path(dir.path());
    let lines: Vec<tokenzero_pulse::PulseEvent> = fs::read_to_string(&ledger)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 3);
    assert_eq!(lines[0].tool, "read");
    assert_eq!(lines[0].session_id.as_deref(), Some(engine.session_id()));
    assert_eq!(lines[0].call_id.as_deref(), Some("7"));
    assert!(lines[0].ref_ids.contains(&ref_id));
    assert!(lines[0].raw_tokens > 0);
    assert_eq!(lines[1].tool, "expand");
    assert_eq!(lines[1].call_id.as_deref(), Some("\"call-8\""));
    assert_eq!(lines[1].session_id.as_deref(), Some(engine.session_id()));
    assert!(lines[1].ref_ids.contains(&ref_id));
    assert_eq!(lines[2].tool, "read");
    assert_eq!(lines[2].call_id.as_deref(), Some("\"7\""));
    assert_ne!(lines[0].call_id, lines[2].call_id);
    assert!(lines[1].recovery_tokens > 0);
}

#[test]
fn session_boot_and_metrics_resources_are_served() {
    let (_dir, engine) = setup_default();
    let boot = resource_read(&engine, 1, "resource://tokenzero/session-boot");
    let boot_payload: Value =
        serde_json::from_str(boot["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(boot_payload["schema"], "tokenzero.session-boot.v1");
    assert!(boot_payload["telemetry"]["total"].as_u64().unwrap() < 100);

    let metrics = resource_read(&engine, 1, "resource://tokenzero/metrics");
    let metrics_payload: Value =
        serde_json::from_str(metrics["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert!(metrics_payload["cumulative"].is_object(), "{metrics_payload:#}");
    assert!(
        metrics_payload["slow_threshold_ms"].as_u64().unwrap() > 0,
        "{metrics_payload:#}"
    );
}
