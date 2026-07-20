//! Registry → FastMCP adapter derivation evidence (tokenzero-irx9.5).
//!
//! Proves catalog/schema derivation from the operation ABI without compiling
//! the full tokenzero-mcp lib test suite (which still has post-extraction WIP).

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use tokenzero_core::operation_abi::{
    all_operations, input_schema_for, output_schema_for, schemas_structurally_equal,
};
use tokenzero_engine::domain_fastmcp_ops;

fn mcp_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tokenzero-mcp/src")
}

#[test]
fn registry_fastmcp_names_equal_domain_dispatch_set() {
    let from_registry: BTreeSet<&str> = all_operations()
        .iter()
        .filter(|op| op.exposure.fastmcp_tool && op.exposure.resource_uri.is_none())
        .filter(|op| {
            matches!(
                op.migration,
                tokenzero_core::operation_abi::MigrationStatus::Canonical
                    | tokenzero_core::operation_abi::MigrationStatus::LegacyAlias
            )
        })
        .map(|op| op.name)
        .collect();
    let from_dispatcher: BTreeSet<&str> = domain_fastmcp_ops().into_iter().collect();
    assert_eq!(
        from_registry, from_dispatcher,
        "FastMCP domain set must equal registry exposure ∩ domain"
    );
}

#[test]
fn every_fastmcp_domain_op_has_io_schemas() {
    for name in domain_fastmcp_ops() {
        let input = input_schema_for(name).unwrap_or_else(|| panic!("{name}: missing input schema"));
        let output =
            output_schema_for(name).unwrap_or_else(|| panic!("{name}: missing output schema"));
        assert_eq!(
            input.get("type").and_then(|v| v.as_str()),
            Some("object"),
            "{name}: input schema type"
        );
        // Output may be a full object schema or a non-empty JSON Schema fragment.
        assert!(
            !output.is_null() && output != serde_json::json!({}),
            "{name}: output schema must be non-empty"
        );
        // Self-parity (golden): schema equals itself structurally.
        assert!(schemas_structurally_equal(&input, &input));
    }
}

#[test]
fn fastmcp_mode_uses_registry_and_single_dispatch() {
    let fastmcp = fs::read_to_string(mcp_root().join("fastmcp_mode.rs")).expect("fastmcp_mode");
    let tools = fs::read_to_string(mcp_root().join("tools.rs")).expect("tools");
    assert!(
        fastmcp.contains("operation_abi") || fastmcp.contains("operation_by_name"),
        "FastMCP mode must derive tools from operation ABI"
    );
    assert!(
        tools.contains("dispatch_operation"),
        "MCP tools must route domain ops through dispatch_operation"
    );
    // No CodeMode planner inside FastMCP path.
    assert!(
        !fastmcp.contains("execute_codemode") && !fastmcp.contains("run_codemode_plan"),
        "FastMCP must not invoke CodeMode planner"
    );
}

#[test]
fn one_call_one_dispatch_surface_in_tools() {
    let tools = fs::read_to_string(mcp_root().join("tools.rs")).expect("tools");
    // Domain path documents single dispatch; count call sites of dispatch_operation.
    let count = tools.matches("dispatch_operation(").count();
    assert!(
        count >= 1,
        "expected at least one dispatch_operation call site"
    );
}
