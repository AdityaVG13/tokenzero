//! CodeMode bindings over typed dispatcher (tokenzero-irx9.6).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tokenzero_core::operation_abi::{MigrationStatus, all_operations};

fn exec_rs() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-mcp/src/codemode/exec.rs");
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn codemode_domain_bindings_use_dispatcher() {
    let src = exec_rs();
    assert!(
        src.contains("dispatch_codemode_method") || src.contains("domain_via_dispatcher"),
        "CodeMode must call shared dispatcher"
    );
    // Registry domain ops must not call engine domain methods directly.
    let forbidden = [
        "engine.glob(",
        "engine.tree(",
        "engine.edit(",
        "engine.shell(",
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
    let production: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut hits = Vec::new();
    for pat in forbidden {
        if production.contains(pat) {
            // shell_background is transport composition, not domain.
            if pat == "engine.shell(" && production.contains("shell_background") {
                // still flag engine.shell( domain calls
            }
            hits.push(pat);
        }
    }
    // Allow shell_background only.
    hits.retain(|p| *p != "engine.shell(" || production.contains("engine.shell(\n"));
    // More precise: scan lines for engine.shell( excluding shell_background
    let mut shell_hits = Vec::new();
    for (i, line) in production.lines().enumerate() {
        if line.contains("engine.shell(") && !line.contains("shell_background") {
            shell_hits.push(format!("{}: {line}", i + 1));
        }
    }
    hits.retain(|p| *p != "engine.shell(");
    assert!(
        hits.is_empty() && shell_hits.is_empty(),
        "CodeMode still calls engine domain methods directly: {hits:?}\n{}",
        shell_hits.join("\n")
    );
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
    assert!(
        !bindings.is_empty(),
        "expected codemode domain bindings in registry"
    );
    // Core domain methods expected in exec routing.
    for required in [
        "zero.read",
        "zero.find",
        "zero.glob",
        "zero.tree",
        "zero.edit",
        "zero.shell",
        "zero.expand",
    ] {
        assert!(
            bindings.iter().any(|b| *b == required)
                || bindings.iter().any(|b| b.ends_with(&required[5..])),
            "missing registry binding for {required}; have {bindings:?}"
        );
    }
}

#[test]
fn no_nested_codemode_planner_in_bindings() {
    let src = exec_rs();
    // Bindings invoke dispatcher; they must not re-enter a plan executor for domain ops.
    assert!(
        !src.contains("run_codemode_plan(") || src.contains("fn run_codemode_plan"),
        "domain bindings must not call nested planner"
    );
}
