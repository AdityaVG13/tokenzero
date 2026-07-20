//! Canonical TokenZero operation ABI and semantic contract (tokenzero-irx9.1).
//!
//! One versioned registry is the source of truth for operation names, aliases,
//! input/output schemas, mutability, capability, cost class, ref ownership,
//! error taxonomy, and cancellation. FastMCP tools and CodeMode bindings must
//! agree with this registry (name set equality **and** full structural
//! input/output schema parity — types, requiredness, nested constraints).
//!
//! Dispatch wiring is tokenzero-irx9.2; this module defines the contract only.

mod catalog;
mod digest;
mod registry;
mod schema;
mod schemas;
mod types;
mod vectors;

pub use catalog::{
    codemode_binding_names, codemode_mcp_tool_names, fastmcp_tool_names, input_schema_for,
    output_schema_for, resolve_operation, resource_uris,
};
pub use digest::{contract_digest, contract_digest_hex, contract_manifest};
pub use registry::{all_operations, operation_by_name};
pub use schema::{
    assert_tool_schema_parity, canonical_json, canonical_schema_json, normalize_schema,
    schema_diff, schema_fingerprint_hex, schema_property_keys, schema_required_keys,
    schemas_structurally_equal,
};
pub use schemas::{
    batch_schema, cache_pack_schema, codemode_describe_schema, codemode_search_schema, edit_schema,
    execute_code_schema, expand_schema, fetch_schema, glob_schema, no_args_schema, read_schema,
    recall_schema, report_tool_issue_schema, rewrite_schema, search_schema, shell_schema,
    text_schema, tree_schema,
};
pub use types::{
    ABI_DEFAULT_SHELL_TIMEOUT_SECS, ABI_HARD_MAX_WALL_MS, CancellationSemantics,
    CapabilityRequirement, CostClass, DomainError, DomainErrorKind, DomainResult, MigrationStatus,
    Mutability, Operation, OperationArgs, OperationId, OperationResults, RefOwnership,
    SurfaceExposure, SEMANTIC_CONTRACT_VERSION,
};
pub use vectors::{GoldenVector, golden_vectors};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;

    #[test]
    fn every_operation_name_is_unique() {
        let mut seen = BTreeSet::new();
        for op in all_operations() {
            assert!(
                seen.insert(op.name),
                "duplicate canonical operation name: {}",
                op.name
            );
        }
    }

    #[test]
    fn every_alias_resolves_to_exactly_one_canonical() {
        let mut alias_to_op: std::collections::BTreeMap<&str, &str> =
            std::collections::BTreeMap::new();
        for op in all_operations() {
            for alias in op.aliases {
                if let Some(prev) = alias_to_op.insert(*alias, op.name) {
                    panic!(
                        "alias {alias:?} claimed by both {prev:?} and {:?}",
                        op.name
                    );
                }
            }
        }
        for (alias, name) in alias_to_op {
            let resolved = resolve_operation(alias).expect("alias resolves");
            assert_eq!(resolved.name, name);
        }
    }

    #[test]
    fn contract_digest_is_deterministic() {
        let a = contract_digest_hex();
        let b = contract_digest_hex();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn semantic_contract_version_is_semver_like() {
        let parts: Vec<_> = SEMANTIC_CONTRACT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "expected MAJOR.MINOR.PATCH");
        for p in parts {
            assert!(p.parse::<u32>().is_ok(), "non-numeric segment: {p}");
        }
    }

    #[test]
    fn fastmcp_names_match_exposure() {
        let expected: BTreeSet<_> = all_operations()
            .iter()
            .filter(|op| op.exposure.fastmcp_tool)
            .map(|op| op.name)
            .collect();
        let actual: BTreeSet<_> = fastmcp_tool_names().into_iter().collect();
        assert_eq!(actual, expected);
        assert!(
            actual.contains("tz_read") && actual.contains("tz_shell"),
            "core material/execution tools present"
        );
    }

    #[test]
    fn codemode_binding_set_matches_exposure() {
        let expected: BTreeSet<_> = all_operations()
            .iter()
            .filter_map(|op| op.exposure.codemode_binding)
            .collect();
        let actual: BTreeSet<_> = codemode_binding_names().into_iter().collect();
        assert_eq!(actual, expected);
        assert!(actual.contains("zero.read"));
        assert!(actual.contains("zero.token.expand"));
    }

    #[test]
    fn resource_uris_are_unique_and_prefixed() {
        let uris = resource_uris();
        let set: BTreeSet<_> = uris.iter().copied().collect();
        assert_eq!(set.len(), uris.len());
        for u in uris {
            assert!(u.starts_with("resource://tokenzero/"), "{u}");
        }
    }

    #[test]
    fn schema_structural_equality_rejects_type_change() {
        let base = read_schema();
        let mut mutated = base.clone();
        mutated["properties"]["path"]["type"] = json!("integer");
        assert!(!schemas_structurally_equal(&base, &mutated));
        assert!(schema_diff(&base, &mutated).is_some());
    }

    #[test]
    fn schema_structural_equality_rejects_requiredness_drift() {
        let base = read_schema();
        let mut mutated = base.clone();
        mutated["required"] = json!([]);
        assert!(!schemas_structurally_equal(&base, &mutated));
    }

    #[test]
    fn schema_structural_equality_rejects_extra_or_missing_props() {
        let base = read_schema();
        let mut extra = base.clone();
        extra["properties"]["unexpected"] = json!({"type": "string"});
        assert!(!schemas_structurally_equal(&base, &extra));
        let mut missing = base.clone();
        missing["properties"]
            .as_object_mut()
            .unwrap()
            .remove("path");
        assert!(!schemas_structurally_equal(&base, &missing));
    }

    #[test]
    fn schema_structural_equality_rejects_nested_constraint_drift() {
        let base = expand_schema();
        let mut mutated = base.clone();
        mutated["properties"]["ref"]["pattern"] = json!("^tz://");
        assert!(!schemas_structurally_equal(&base, &mutated));
    }

    #[test]
    fn schema_structural_equality_rejects_output_shape_drift() {
        let op = operation_by_name("tz_read").expect("tz_read");
        let mut mutated = op.results.schema.clone();
        mutated["oneOf"][0]["required"] = json!(["value"]); // drop refs/op requirements
        assert!(!schemas_structurally_equal(&op.results.schema, &mutated));
    }

    #[test]
    fn schema_doc_keys_do_not_break_parity() {
        let base = read_schema();
        let mut with_desc = base.clone();
        with_desc["description"] = json!("prose only");
        with_desc["properties"]["path"]["title"] = json!("Path");
        assert!(schemas_structurally_equal(&base, &with_desc));
    }

    #[test]
    fn digest_changes_when_input_schema_type_changes() {
        // Fingerprint of read_schema must differ from a type-mutated copy.
        let a = schema_fingerprint_hex(&read_schema());
        let mut m = read_schema();
        m["properties"]["raw"]["type"] = json!("string");
        let b = schema_fingerprint_hex(&m);
        assert_ne!(a, b);
    }

    #[test]
    fn golden_vectors_cover_required_tags() {
        let vectors = golden_vectors();
        let mut tags: BTreeSet<&str> = BTreeSet::new();
        for v in &vectors {
            for t in v.tags {
                tags.insert(*t);
            }
            assert!(
                operation_by_name(v.op).is_some() || resolve_operation(v.op).is_some(),
                "vector op must exist in registry: {}",
                v.op
            );
            assert!(
                v.expected_ok.is_some() || v.expected_err.is_some(),
                "vector {} needs expected_ok or expected_err",
                v.id
            );
        }
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
    fn every_public_fastmcp_tool_has_complete_io_schemas() {
        for op in all_operations().iter().filter(|o| o.exposure.fastmcp_tool) {
            assert_eq!(
                op.args.schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "{} input must be object schema",
                op.name
            );
            assert!(
                op.results.schema.get("oneOf").is_some()
                    || op.results.schema.get("type").is_some(),
                "{} must own an output schema",
                op.name
            );
        }
    }

    #[test]
    fn report_tool_issue_appears_on_both_surfaces() {
        let op = operation_by_name("tz_report_tool_issue").expect("report tool");
        assert!(op.exposure.fastmcp_tool);
        assert!(op.exposure.codemode_mcp_tool);
    }
}
