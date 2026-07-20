//! Cross-surface catalog parity against the tokenzero-irx9.1 operation ABI.
//!
//! These tests live in the transport crate so they can see live FastMCP and
//! CodeMode catalogs. The registry itself is tested in tokenzero-core.

#[cfg(test)]
mod tests {
    use crate::catalog::{canonical_tool_specs, resource_specs, TOOL_ALIASES};
    use crate::codemode::catalog::method_paths;
    use serde_json::json;
    use std::collections::BTreeSet;
    use tokenzero_core::operation_abi::{
        all_operations, contract_digest_hex, fastmcp_tool_names, operation_by_name,
        resolve_operation, resource_uris, schema_diff, schemas_structurally_equal,
    };

    #[test]
    fn mcp_tool_seed_names_match_registry_surface_set() {
        let catalog: BTreeSet<_> = canonical_tool_specs().iter().map(|s| s.name).collect();
        let registry: BTreeSet<_> = all_operations()
            .iter()
            .filter(|op| op.exposure.fastmcp_tool || op.exposure.codemode_mcp_tool)
            .map(|op| op.name)
            .collect();
        assert_eq!(
            catalog, registry,
            "catalog tool seeds must equal ABI exposure for mcp tools"
        );
    }

    #[test]
    fn every_catalog_input_schema_matches_registry_structurally() {
        for seed in canonical_tool_specs() {
            let op = operation_by_name(seed.name)
                .unwrap_or_else(|| panic!("registry missing {}", seed.name));
            assert!(
                schemas_structurally_equal(&seed.input_schema, &op.args.schema),
                "input schema drift for {}: {:?}",
                seed.name,
                schema_diff(&seed.input_schema, &op.args.schema)
            );
        }
    }

    #[test]
    fn tool_aliases_resolve_via_abi() {
        for &(alias, target) in TOOL_ALIASES {
            let resolved = resolve_operation(alias).unwrap_or_else(|| {
                panic!("alias {alias} does not resolve");
            });
            assert_eq!(resolved.name, target, "alias {alias}");
        }
    }

    #[test]
    fn resource_uris_match_registry() {
        let catalog: BTreeSet<_> = resource_specs()
            .into_iter()
            .map(|r| r.uri)
            .collect();
        let registry: BTreeSet<_> = resource_uris()
            .into_iter()
            .map(|u| u.to_string())
            .collect();
        assert_eq!(catalog, registry);
    }

    #[test]
    fn every_codemode_method_path_resolves_in_abi() {
        for path in method_paths() {
            assert!(
                resolve_operation(path).is_some(),
                "CodeMode catalog path {path} not in operation ABI"
            );
        }
    }

    #[test]
    fn kill_missing_tool_from_catalog_set() {
        let mut names: BTreeSet<_> = fastmcp_tool_names().into_iter().collect();
        assert!(names.remove("tz_read"));
        let full: BTreeSet<_> = fastmcp_tool_names().into_iter().collect();
        assert_ne!(names, full, "set equality must fail when a tool is missing");
    }

    #[test]
    fn kill_extra_tool_in_catalog_set() {
        let mut names: BTreeSet<_> = fastmcp_tool_names().into_iter().collect();
        names.insert("tz_not_a_real_tool");
        let full: BTreeSet<_> = fastmcp_tool_names().into_iter().collect();
        assert_ne!(names, full);
    }

    #[test]
    fn kill_schema_type_change_against_live_catalog() {
        let seed = canonical_tool_specs()
            .iter()
            .find(|s| s.name == "tz_read")
            .expect("tz_read");
        let op = operation_by_name("tz_read").expect("registry");
        let mut mutated = seed.input_schema.clone();
        mutated["properties"]["path"]["type"] = json!("integer");
        assert!(!schemas_structurally_equal(&mutated, &op.args.schema));
    }

    #[test]
    fn digest_is_stable_hex() {
        let d = contract_digest_hex();
        assert_eq!(d.len(), 64);
        assert_eq!(d, contract_digest_hex());
    }
}
