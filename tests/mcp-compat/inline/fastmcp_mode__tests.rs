use super::*;
use tokenzero_core::operation_abi::operation_by_name;

#[test]
fn projection_keeps_canonical_metadata_and_exact_mcp_aliases() {
    let operation = operation_by_name("tz_read").expect("read operation");
    let seed = canonical_tool_specs()
        .iter()
        .find(|seed| seed.name == operation.name)
        .expect("read catalog seed");
    let projected = canonical_operation(operation, seed.summary);
    assert_eq!(projected.mcp_tool_name.as_deref(), Some("tz_read"));
    assert_eq!(projected.aliases, vec!["read"]);
    assert_eq!(projected.description, seed.summary);
    assert_eq!(projected.args_schema, operation.args.schema);
    assert_eq!(
        projected.output_schema,
        Some(operation.results.schema.clone())
    );
}

#[test]
fn projection_keeps_human_resource_names_and_legacy_instructions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let registration = surface_registration(&engine, McpToolSurface::Classic);
    assert_eq!(
        registration.instructions.as_deref(),
        Some(fastmcp_instructions())
    );
    registration
        .validate()
        .expect("lossless surface registration");

    let projected_resources = registration.adapter.registry.resources;
    let expected_resources = resource_specs();
    assert_eq!(projected_resources.len(), expected_resources.len());
    for (projected, expected) in projected_resources.iter().zip(expected_resources) {
        assert_eq!(projected.uri, expected.uri);
        assert_eq!(projected.name, expected.name);
        assert_eq!(projected.description, expected.description);
        assert_eq!(
            projected.mime_type.as_deref(),
            Some(expected.mime_type.as_str())
        );
    }
}

#[test]
fn projection_keeps_every_classic_tool_and_alias_catalog_entry_exact() {
    let dir = tempfile::tempdir().expect("tempdir");
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let registration = surface_registration(&engine, McpToolSurface::Classic);
    let operations = &registration.adapter.registry.operations;
    let expected_primary = canonical_tool_specs()
        .iter()
        .filter(|seed| surface_includes(McpToolSurface::Classic, seed.name))
        .collect::<Vec<_>>();
    assert_eq!(operations.len(), expected_primary.len());
    for seed in expected_primary {
        let operation = operation_by_name(seed.name).expect("catalog operation");
        let projected = operations
            .iter()
            .find(|candidate| candidate.mcp_tool_name.as_deref() == Some(seed.name))
            .expect("projected canonical tool");
        assert_eq!(projected.description, seed.summary);
        assert_eq!(projected.args_schema, operation.args.schema);
        assert_eq!(
            projected.output_schema,
            Some(operation.results.schema.clone())
        );
        assert_eq!(projected.aliases, mcp_aliases_for(seed.name));
    }

    let projected_aliases = alias_metadata(McpToolSurface::Classic);
    let expected_aliases = TOOL_ALIASES
        .iter()
        .filter(|(_, target)| surface_includes(McpToolSurface::Classic, target))
        .collect::<Vec<_>>();
    assert_eq!(projected_aliases.len(), expected_aliases.len());
    for ((alias, target), projected) in expected_aliases.into_iter().zip(projected_aliases) {
        let operation = operation_by_name(target).expect("alias target operation");
        assert_eq!(
            projected.canonical_id,
            canonical_id(operation.name, operation.cluster)
        );
        assert_eq!(projected.name, *alias);
        assert_eq!(
            projected.description,
            Some(crate::catalog::alias_summary(target))
        );
        assert_eq!(
            projected.input_schema,
            serde_json::json!({"type": "object"})
        );
        assert_eq!(projected.output_schema, None);
    }
}
