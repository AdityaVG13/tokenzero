//! CodeMode bindings over typed dispatcher + runtime parity (tokenzero-irx9.6).

use serde_json::json;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use tokenzero_core::operation_abi::{MigrationStatus, all_operations};
use tokenzero_engine::{
    EngineConfig, TokenZeroEngine, dispatch_codemode_method, dispatch_mcp_tool,
};

fn exec_rs() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-mcp/src/codemode/exec.rs");
    fs::read_to_string(&path).unwrap()
}

fn engine_for(root: &std::path::Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

#[test]
fn codemode_domain_bindings_use_dispatcher() {
    let src = exec_rs();
    assert!(src.contains("dispatch_codemode_method") || src.contains("domain_via_dispatcher"));
    let production: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let forbidden = [
        "engine.glob(",
        "engine.tree(",
        "engine.edit(",
        "engine.expand(",
        "engine.expand_with_params(",
        "engine.read(",
        "engine.find(",
        "engine.grep(",
        "engine.ingest(",
        "engine.mem(",
        "engine.recall(",
        "engine.fetch(",
        "engine.cache_pack(",
    ];
    for pat in forbidden {
        assert!(
            !production.contains(pat),
            "CodeMode still calls {pat} directly"
        );
    }
    for (i, line) in production.lines().enumerate() {
        if line.contains("engine.shell(") && !line.contains("shell_background") {
            panic!("direct engine.shell at {}: {line}", i + 1);
        }
    }
}

#[test]
fn every_codemode_domain_binding_is_registry_backed() {
    let bindings: BTreeSet<&str> = all_operations()
        .iter()
        .filter(|op| {
            op.exposure.codemode_binding.is_some()
                && matches!(
                    op.migration,
                    MigrationStatus::Canonical | MigrationStatus::LegacyAlias
                )
                && op.exposure.resource_uri.is_none()
        })
        .filter_map(|op| op.exposure.codemode_binding)
        .collect();
    for required in [
        "zero.read",
        "zero.find",
        "zero.glob",
        "zero.tree",
        "zero.edit",
        "zero.shell",
        "zero.token.expand",
    ] {
        assert!(bindings.contains(required), "missing {required} in {bindings:?}");
    }
}

/// Runtime: one-op CodeMode result normalizes to FastMCP for bound domain ops.
#[test]
fn one_op_codemode_normalizes_to_fastmcp() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"codemode-parity").unwrap();
    let root = dir.path();
    let note = root.join("note.txt").display().to_string();

    let cases: Vec<(&str, &str, serde_json::Value)> = vec![
        ("tz_read", "zero.read", json!({"path": note})),
        (
            "tz_glob",
            "zero.glob",
            json!({"pattern": "*.txt", "path": root.display().to_string()}),
        ),
        (
            "tz_tree",
            "zero.tree",
            json!({"path": root.display().to_string(), "depth": 1}),
        ),
        ("tz_mem", "zero.mem", json!({})),
        (
            "tz_shell",
            "zero.shell",
            json!({"command": "true", "cwd": root.display().to_string()}),
        ),
        (
            "tz_edit",
            "zero.edit",
            json!({
                "path": note,
                "edits": [{"find": "codemode-parity", "replace": "codemode-parity"}],
                "dry_run": true
            }),
        ),
    ];

    for (mcp_name, cm_name, args) in cases {
        let mcp = dispatch_mcp_tool(&engine_for(root), mcp_name, &args).expect("mcp");
        let cm = dispatch_codemode_method(&engine_for(root), cm_name, &args).expect("cm");
        let n = |o: &tokenzero_engine::DispatchOutcome| {
            let r = o.tool_response.as_ref().expect("resp");
            (
                r.status.clone(),
                r.error.as_ref().map(|e| e.code.clone()),
                r.visible.as_ref().map(|v| v.text.clone()),
                r.refs.iter().map(|x| x.ref_id.clone()).collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            n(&mcp),
            n(&cm),
            "CodeMode {cm_name} must normalize to FastMCP {mcp_name}"
        );
    }
}

/// Recipe/JSON form: dispatch_codemode_method is the recipe/JSON path (no JS).
#[test]
fn recipe_json_path_is_direct_dispatcher() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), b"x").unwrap();
    let eng = engine_for(dir.path());
    let out = dispatch_codemode_method(
        &eng,
        "zero.read",
        &json!({"path": dir.path().join("note.txt").display().to_string()}),
    )
    .expect("recipe/json path");
    assert!(out.is_ok());
    // Source-level: recipe path does not allocate QuickJS for domain-only dispatch.
    let src = exec_rs();
    // domain_via_dispatcher / dispatch_codemode_method must not call rquickjs.
    let domain_fn = src
        .split("fn domain_via_dispatcher")
        .nth(1)
        .unwrap_or("");
    let domain_body = domain_fn.split("fn ").next().unwrap_or("");
    assert!(
        !domain_body.contains("rquickjs") && !domain_body.contains("Runtime::new"),
        "domain_via_dispatcher must not start a JS runtime"
    );
}

#[test]
fn no_nested_codemode_planner_in_bindings() {
    let src = exec_rs();
    // Nested planner call pattern must not appear in domain exec helpers.
    assert!(!src.contains("run_codemode_plan(engine"));
}
