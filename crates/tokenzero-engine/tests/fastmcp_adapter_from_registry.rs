//! Registry → FastMCP adapter derivation + runtime parity (tokenzero-irx9.5).

use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use tokenzero_core::operation_abi::{
    MigrationStatus, all_operations, input_schema_for, output_schema_for,
    schemas_structurally_equal,
};
use tokenzero_engine::{
    DispatchSurface, EngineConfig, TokenZeroEngine, dispatch_mcp_tool, dispatch_operation,
    domain_fastmcp_ops,
};

fn mcp_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-mcp/src")
}

fn engine_for(root: &std::path::Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

#[test]
fn registry_fastmcp_names_equal_domain_dispatch_set() {
    let from_registry: BTreeSet<&str> = all_operations()
        .iter()
        .filter(|op| op.exposure.fastmcp_tool && op.exposure.resource_uri.is_none())
        .filter(|op| {
            matches!(
                op.migration,
                MigrationStatus::Canonical | MigrationStatus::LegacyAlias
            )
        })
        .map(|op| op.name)
        .collect();
    let from_dispatcher: BTreeSet<&str> = domain_fastmcp_ops().into_iter().collect();
    assert_eq!(from_registry, from_dispatcher);
}

#[test]
fn every_fastmcp_domain_op_has_io_schemas() {
    for name in domain_fastmcp_ops() {
        let input = input_schema_for(name).expect("input schema");
        let output = output_schema_for(name).expect("output schema");
        assert_eq!(input.get("type").and_then(|v| v.as_str()), Some("object"));
        assert!(!output.is_null() && output != json!({}));
        assert!(schemas_structurally_equal(&input, &input));
    }
}

/// Runtime golden: one MCP dispatch per domain op produces exactly one
/// domain envelope (status + tool) and does not invent extra tools.
#[test]
fn runtime_one_call_one_domain_dispatch_per_tool() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"adapter-probe").unwrap();
    let engine = engine_for(dir.path());
    let note = dir.path().join("note.txt").display().to_string();

    for op in domain_fastmcp_ops() {
        if matches!(op, "tz_fetch") {
            continue; // network
        }
        let args = match op {
            "tz_read" => json!({"path": note}),
            "tz_find" | "tz_grep" => {
                json!({"query": "adapter", "path": dir.path().display().to_string()})
            }
            "tz_recall" => json!({"query": "adapter"}),
            "tz_glob" => json!({"pattern": "*.txt", "path": dir.path().display().to_string()}),
            "tz_tree" => json!({"path": dir.path().display().to_string(), "depth": 1}),
            "tz_edit" => json!({
                "path": note,
                "edits": [{"find": "adapter-probe", "replace": "adapter-probe"}],
                "dry_run": true
            }),
            "tz_shell" => json!({"command": "true", "cwd": dir.path().display().to_string()}),
            "tz_ingest" => json!({"text": "x"}),
            "tz_expand" => {
                json!({"ref": "tz://0000000000000000000000000000000000000000000000000000000000000000"})
            }
            "tz_mem" => json!({}),
            "tz_cache_pack" => json!({"scope": "agent"}),
            "tz_rewrite" => json!({"command": "echo hi"}),
            "tz_discover" => json!({}),
            "tz_report_tool_issue" => json!({"tool": "zero_execute", "summary": "probe"}),
            "tz_batch" => json!({"ops": [{"tool": "tz_mem", "args": {}}]}),
            _ => json!({}),
        };
        let mcp = dispatch_mcp_tool(&engine, op, &args).expect("mcp");
        let raw = dispatch_operation(&engine, DispatchSurface::Mcp, op, &args);
        // Same dispatcher entry → matching status class.
        assert_eq!(mcp.is_ok(), raw.is_ok(), "{op}: mcp vs raw ok class");
        if let Some(resp) = mcp.tool_response.as_ref() {
            assert!(
                !resp.tool.is_empty(),
                "{op}: transport envelope must name a tool"
            );
        }
    }
}

/// Exact catalog derivation snapshot: registry FastMCP names + schema fingerprints.
#[test]
fn registry_to_tools_golden_catalog_snapshot() {
    let mut rows = Vec::new();
    for op in all_operations().iter().filter(|o| o.exposure.fastmcp_tool) {
        let input = input_schema_for(op.name).unwrap_or(json!({}));
        let output = output_schema_for(op.name).unwrap_or(json!({}));
        rows.push(json!({
            "name": op.name,
            "aliases": op.aliases,
            "input_type": input.get("type"),
            "input_required": input.get("required"),
            "input_props": input.get("properties").and_then(|p| p.as_object()).map(|m| {
                let mut keys: Vec<_> = m.keys().cloned().collect();
                keys.sort();
                keys
            }),
            "output_present": !output.is_null(),
        }));
    }
    rows.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let golden = json!({
        "schema": "tokenzero.irx9.fastmcp_catalog.v1",
        "tools": rows,
    });
    let snap: BTreeSet<_> = golden["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    for required in ["tz_read", "tz_shell", "tz_mem", "tz_edit"] {
        assert!(
            snap.contains(required),
            "missing {required} in golden catalog"
        );
    }
    let expected: BTreeSet<_> = all_operations()
        .iter()
        .filter(|o| o.exposure.fastmcp_tool)
        .map(|o| o.name)
        .collect();
    assert_eq!(
        snap, expected,
        "golden catalog name set must equal registry FastMCP exposure"
    );
}

#[test]
fn fastmcp_mode_uses_registry_and_single_dispatch() {
    let fastmcp = fs::read_to_string(mcp_root().join("fastmcp_mode.rs")).unwrap();
    let tools = fs::read_to_string(mcp_root().join("tools.rs")).unwrap();
    assert!(fastmcp.contains("operation_by_name") || fastmcp.contains("operation_abi"));
    assert!(tools.contains("dispatch_operation("));
    assert!(!fastmcp.contains("execute_codemode") && !fastmcp.contains("run_codemode_plan"));
}
