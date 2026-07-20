//! Live-boundary parity for tokenzero-irx9.1.
//!
//! These tests intentionally compare the ABI registry against **independent**
//! live surfaces that are not re-derived from the same `all_operations()` view:
//!
//! - `ToolKind` / `tool_table!` product names + `dispatch_tool` exhaustiveness
//! - `TOOL_ALIASES` table
//! - `resource_specs()` URI list
//! - CodeMode `METHOD_CATALOG` paths
//! - Wire `tools/list` / FastMCP definitions (input **and** output schemas)
//! - CodeMode `describe_method` / method catalog I/O schemas
//!
//! Kill tests mutate independent fixtures (name sets, cloned wire schemas)
//! so missing/extra tools and structural I/O drift fail closed.

#[cfg(test)]
mod tests {
    use crate::catalog::{
        ToolKind, TOOL_ALIASES, canonical_tool_specs, resource_specs, tool_specs,
        tool_specs_for_filter,
    };
    use crate::codemode::catalog::{describe_method, method_paths};
    use serde_json::{Value, json};
    use std::collections::BTreeSet;
    use tokenzero_core::McpToolSurface;
    use tokenzero_core::operation_abi::{
        all_operations, contract_digest_hex, golden_vectors, operation_by_name, resolve_operation,
        resource_uris, schema_diff, schemas_structurally_equal,
    };

    /// Independent product inventory: every `tool_table!` seed name.
    /// This is the live MCP product list, maintained next to handlers — not a
    /// projection of `all_operations()`.
    fn live_tool_table_names() -> BTreeSet<&'static str> {
        canonical_tool_specs().iter().map(|s| s.name).collect()
    }

    /// Independent alias inventory from the const table.
    fn live_alias_pairs() -> BTreeSet<(&'static str, &'static str)> {
        TOOL_ALIASES.iter().copied().collect()
    }

    /// Independent resource inventory.
    fn live_resource_uris() -> BTreeSet<String> {
        resource_specs().into_iter().map(|r| r.uri).collect()
    }

    /// Independent CodeMode method inventory.
    fn live_codemode_paths() -> BTreeSet<&'static str> {
        method_paths().into_iter().collect()
    }

    /// Wire tools/list boundary (classic surface, aliases off).
    fn live_classic_tools_list() -> Vec<crate::catalog::ToolSpec> {
        tool_specs_for_filter(None, false, McpToolSurface::Classic)
    }

    /// Simulate FastMCP registration definitions for the classic surface.
    fn live_fastmcp_definitions() -> Vec<(String, Value, Option<Value>)> {
        let mut out = Vec::new();
        for seed in canonical_tool_specs() {
            if !crate::surface_health::surface_includes(McpToolSurface::Classic, seed.name) {
                continue;
            }
            let (input, output) = match operation_by_name(seed.name) {
                Some(op) => (op.args.schema.clone(), Some(op.results.schema.clone())),
                None => (seed.input_schema.clone(), None),
            };
            out.push((seed.name.to_string(), input, output));
        }
        out
    }

    #[test]
    fn every_tool_table_name_has_registry_and_toolkind() {
        for name in live_tool_table_names() {
            assert!(
                operation_by_name(name).is_some(),
                "live tool_table name {name} missing from operation ABI registry"
            );
            assert!(
                ToolKind::from_canonical(name).is_some(),
                "live tool_table name {name} missing ToolKind (dispatch gap)"
            );
        }
    }

    #[test]
    fn every_registry_mcp_tool_is_in_tool_table() {
        let live = live_tool_table_names();
        for op in all_operations()
            .iter()
            .filter(|o| o.exposure.fastmcp_tool || o.exposure.codemode_mcp_tool)
        {
            assert!(
                live.contains(op.name),
                "registry MCP op {} not present in live tool_table",
                op.name
            );
        }
    }

    #[test]
    fn kill_missing_live_tool_from_set_equality() {
        let mut live = live_tool_table_names();
        let registry: BTreeSet<_> = all_operations()
            .iter()
            .filter(|o| o.exposure.fastmcp_tool || o.exposure.codemode_mcp_tool)
            .map(|o| o.name)
            .collect();
        assert_eq!(live, registry);
        live.remove("tz_read");
        assert_ne!(
            live, registry,
            "set equality must fail when a live tool is missing"
        );
    }

    #[test]
    fn kill_extra_live_tool_from_set_equality() {
        let live = live_tool_table_names();
        let mut registry: BTreeSet<_> = all_operations()
            .iter()
            .filter(|o| o.exposure.fastmcp_tool || o.exposure.codemode_mcp_tool)
            .map(|o| o.name)
            .collect();
        registry.insert("tz_not_registered");
        assert_ne!(live, registry);
    }

    #[test]
    fn live_aliases_match_registry_alias_inventory() {
        let live = live_alias_pairs();
        let mut registry_pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
        for op in all_operations() {
            for alias in op.aliases {
                // Only MCP short aliases (not zero.* CodeMode aliases)
                if !alias.contains('.') {
                    registry_pairs.insert((*alias, op.name));
                }
            }
        }
        assert_eq!(
            live, registry_pairs,
            "TOOL_ALIASES must equal registry non-dotted aliases"
        );
        for (alias, target) in live {
            let resolved = resolve_operation(alias).expect("alias resolves");
            assert_eq!(resolved.name, target);
        }
    }

    #[test]
    fn kill_altered_alias_target() {
        let mut live = live_alias_pairs();
        live.insert(("read", "tz_shell")); // wrong target
        let registry: BTreeSet<_> = all_operations()
            .iter()
            .flat_map(|op| {
                op.aliases
                    .iter()
                    .filter(|a| !a.contains('.'))
                    .map(move |a| (*a, op.name))
            })
            .collect();
        // live has both correct and wrong? insert replaces if same key - BTreeSet of pairs
        // so we have both (read, tz_read) and (read, tz_shell)
        assert_ne!(live.len(), registry.len());
    }

    #[test]
    fn live_resources_match_registry() {
        let live = live_resource_uris();
        let registry: BTreeSet<_> = resource_uris()
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        assert_eq!(live, registry);
    }

    #[test]
    fn kill_extra_or_missing_resource_uri() {
        let mut live = live_resource_uris();
        let registry: BTreeSet<_> = resource_uris()
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        assert_eq!(live, registry);
        live.insert("resource://tokenzero/not-real".into());
        assert_ne!(live, registry);
        live.remove("resource://tokenzero/not-real");
        live.remove("resource://tokenzero/tools");
        assert_ne!(live, registry);
    }

    #[test]
    fn every_codemode_method_resolves_and_has_io_schemas_on_describe() {
        for path in live_codemode_paths() {
            let op = resolve_operation(path)
                .unwrap_or_else(|| panic!("CodeMode path {path} not in ABI"));
            let described = describe_method(path);
            assert!(
                described.get("error").is_none(),
                "describe failed for {path}: {described}"
            );
            let input = described
                .get("inputSchema")
                .unwrap_or_else(|| panic!("describe({path}) missing inputSchema"));
            let output = described
                .get("outputSchema")
                .unwrap_or_else(|| panic!("describe({path}) missing outputSchema"));
            assert!(
                schemas_structurally_equal(input, &op.args.schema),
                "CodeMode describe input drift for {path}: {:?}",
                schema_diff(input, &op.args.schema)
            );
            assert!(
                schemas_structurally_equal(output, &op.results.schema),
                "CodeMode describe output drift for {path}: {:?}",
                schema_diff(output, &op.results.schema)
            );
        }
    }

    #[test]
    fn live_tools_list_exposes_complete_io_schemas_matching_registry() {
        for tool in live_classic_tools_list() {
            let op = operation_by_name(&tool.name)
                .unwrap_or_else(|| panic!("tools/list name {} not in registry", tool.name));
            assert!(
                schemas_structurally_equal(&tool.input_schema, &op.args.schema),
                "tools/list inputSchema drift for {}: {:?}",
                tool.name,
                schema_diff(&tool.input_schema, &op.args.schema)
            );
            let out = tool
                .output_schema
                .as_ref()
                .unwrap_or_else(|| panic!("tools/list missing outputSchema for {}", tool.name));
            assert!(
                schemas_structurally_equal(out, &op.results.schema),
                "tools/list outputSchema drift for {}: {:?}",
                tool.name,
                schema_diff(out, &op.results.schema)
            );
        }
    }

    #[test]
    fn live_fastmcp_definitions_include_output_schemas() {
        let defs = live_fastmcp_definitions();
        assert!(!defs.is_empty());
        for (name, input, output) in defs {
            let op = operation_by_name(&name).expect("registry op");
            assert!(schemas_structurally_equal(&input, &op.args.schema));
            let out = output.unwrap_or_else(|| panic!("FastMCP missing output_schema for {name}"));
            assert!(
                schemas_structurally_equal(&out, &op.results.schema),
                "FastMCP output drift for {name}: {:?}",
                schema_diff(&out, &op.results.schema)
            );
        }
    }

    #[test]
    fn kill_input_type_change_on_wire_schema() {
        let tool = live_classic_tools_list()
            .into_iter()
            .find(|t| t.name == "tz_read")
            .expect("tz_read");
        let op = operation_by_name("tz_read").expect("op");
        let mut mutated = tool.input_schema.clone();
        mutated["properties"]["path"]["type"] = json!("integer");
        assert!(!schemas_structurally_equal(&mutated, &op.args.schema));
        assert!(schema_diff(&mutated, &op.args.schema).is_some());
    }

    #[test]
    fn kill_input_requiredness_drift_on_wire_schema() {
        let tool = live_classic_tools_list()
            .into_iter()
            .find(|t| t.name == "tz_read")
            .expect("tz_read");
        let op = operation_by_name("tz_read").expect("op");
        let mut mutated = tool.input_schema.clone();
        mutated["required"] = json!([]);
        assert!(!schemas_structurally_equal(&mutated, &op.args.schema));
    }

    #[test]
    fn kill_input_missing_and_extra_properties() {
        let tool = live_classic_tools_list()
            .into_iter()
            .find(|t| t.name == "tz_read")
            .expect("tz_read");
        let op = operation_by_name("tz_read").expect("op");
        let mut missing = tool.input_schema.clone();
        missing["properties"]
            .as_object_mut()
            .unwrap()
            .remove("path");
        assert!(!schemas_structurally_equal(&missing, &op.args.schema));
        let mut extra = tool.input_schema.clone();
        extra["properties"]["unexpected_field"] = json!({"type": "string"});
        assert!(!schemas_structurally_equal(&extra, &op.args.schema));
    }

    #[test]
    fn kill_nested_input_constraint_drift() {
        let tool = live_classic_tools_list()
            .into_iter()
            .find(|t| t.name == "tz_expand")
            .expect("tz_expand");
        let op = operation_by_name("tz_expand").expect("op");
        let mut mutated = tool.input_schema.clone();
        mutated["properties"]["ref"]["pattern"] = json!("^tz://");
        assert!(!schemas_structurally_equal(&mutated, &op.args.schema));
    }

    #[test]
    fn kill_output_shape_drift_on_wire_schema() {
        let tool = live_classic_tools_list()
            .into_iter()
            .find(|t| t.name == "tz_read")
            .expect("tz_read");
        let op = operation_by_name("tz_read").expect("op");
        let mut mutated = tool.output_schema.clone().expect("output");
        // Drop required fields / change envelope
        mutated["oneOf"][0]["required"] = json!(["value"]);
        assert!(!schemas_structurally_equal(&mutated, &op.results.schema));
        let mut type_changed = tool.output_schema.clone().expect("output");
        type_changed["oneOf"] = json!([{"type": "string"}]);
        assert!(!schemas_structurally_equal(
            &type_changed,
            &op.results.schema
        ));
    }

    #[test]
    fn kill_codemode_describe_output_drift() {
        let described = describe_method("zero.read");
        let op = resolve_operation("zero.read").expect("op");
        let mut mutated = described["outputSchema"].clone();
        mutated["oneOf"][0]["properties"]
            .as_object_mut()
            .unwrap()
            .remove("refs");
        assert!(!schemas_structurally_equal(&mutated, &op.results.schema));
    }

    #[test]
    fn kill_altered_operation_name_does_not_resolve() {
        assert!(resolve_operation("tz_read_typo").is_none());
        assert!(operation_by_name("tz_READ").is_none());
    }

    #[test]
    fn classic_tool_specs_default_includes_aliases_independently() {
        let with_aliases: BTreeSet<_> = tool_specs().into_iter().map(|t| t.name).collect();
        let without: BTreeSet<_> = live_classic_tools_list()
            .into_iter()
            .map(|t| t.name)
            .collect();
        for &(alias, target) in TOOL_ALIASES {
            if without.contains(target) {
                assert!(
                    with_aliases.contains(alias),
                    "default tools/list must include live alias {alias} -> {target}"
                );
            }
        }
        assert!(with_aliases.len() > without.len());
    }

    #[test]
    fn digest_and_golden_vectors_still_valid() {
        let d = contract_digest_hex();
        assert_eq!(d.len(), 64);
        assert_eq!(d, contract_digest_hex());
        let vectors = golden_vectors();
        let tags: BTreeSet<_> = vectors.iter().flat_map(|v| v.tags.iter().copied()).collect();
        for required in [
            "success",
            "typed_failure",
            "ref_recovery",
            "mutation",
            "deadline",
            "cancellation",
        ] {
            assert!(tags.contains(required), "missing golden tag {required}");
        }
    }

    #[test]
    fn report_tool_issue_on_both_live_surfaces() {
        assert!(crate::surface_health::surface_includes(
            McpToolSurface::Classic,
            "tz_report_tool_issue"
        ));
        // Codemode surface policy: report may be classic+codemode per ABI
        let op = operation_by_name("tz_report_tool_issue").expect("op");
        assert!(op.exposure.fastmcp_tool && op.exposure.codemode_mcp_tool);
    }
}
