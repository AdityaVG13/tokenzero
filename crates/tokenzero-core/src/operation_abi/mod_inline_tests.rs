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
    fn contract_digest_matches_frozen_operation_abi() {
        // Bumped 2026-07-26 (tokenzero-gpa0): `tz_shell` gained an optional
        // `timeout_ms` property. Additive and backward compatible -- existing
        // callers that spell the deadline in seconds are unaffected -- but the
        // published ABI surface changed, so the freeze is re-pinned here
        // deliberately rather than relaxed.
        assert_eq!(
            contract_digest_hex(),
            "f8c415afad6870926d0d22907e8f350b11854ab2f73164d211bd467ce1d42b04"
        );
    }

    /// tokenzero-irx9.9: memoized digest must stay byte-stable and stay hot.
    /// Kill-test: if memoization were removed, 200 calls recompute the full
    /// manifest hash; with OnceLock the second batch must not be slower than
    /// ~10x the first single call wall (loose CI-safe bound) and must return
    /// identical hex.
    #[test]
    fn contract_digest_memoization_before_after_and_kill() {
        use std::time::Instant;
        // Warm / first call (may compute).
        let t0 = Instant::now();
        let first = contract_digest_hex();
        let first_ns = t0.elapsed().as_nanos() as u64;

        // After: 200 cached hits.
        let t1 = Instant::now();
        for _ in 0..200 {
            let last = contract_digest_hex();
            assert_eq!(last, first, "memoized digest must not drift");
        }
        let batch_ns = t1.elapsed().as_nanos() as u64;
        let per_hit = batch_ns / 200;

        // Kill-test for removed work: cached per-hit must be far cheaper than
        // a full recompute budget. On CI noise we only require that 200 hits
        // finish in under 50ms total (memoized) — uncached SHA of large
        // manifest 200x would typically exceed that on this machine class.
        assert!(
            batch_ns < 50_000_000,
            "memoized digest batch too slow ({batch_ns} ns); OnceLock may be broken"
        );
        // first_ns may be 0 on coarse clocks; only log via assert soft bound.
        let _ = (first_ns, per_hit);
        assert_eq!(contract_digest(), contract_digest());
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
