use super::*;

#[test]
fn handshake_does_not_list_catalog() {
    let cap = build_surface_capability(HandshakeSurface::Mcp);
    let v = serde_json::to_value(&cap).unwrap();
    let s = v.to_string();
    assert!(!s.contains("tz_read") && !s.contains("canonical_tools"));
    assert_eq!(cap.schema, SURFACE_CAPABILITY_SCHEMA);
    assert_eq!(cap.surface, "mcp");
    assert_eq!(cap.semantic_contract_version, SEMANTIC_CONTRACT_VERSION);
    assert_eq!(cap.semantic_contract_digest.len(), 64);
    assert_eq!(cap.raw_worker_version, RAW_WORKER_PROTOCOL_VERSION);
}

#[test]
fn raw_worker_handshake_marks_client_planner() {
    let cap = build_surface_capability(HandshakeSurface::RawWorker);
    assert_eq!(cap.planner_owner, "client");
    assert!(cap.plan_forms.iter().any(|f| f == "raw_frame"));
}

#[test]
fn digest_mismatch_fails_closed() {
    let local = build_surface_capability(HandshakeSurface::RawWorker);
    let err = check_contract_compatibility(&local, Some("deadbeef"), None).unwrap_err();
    assert!(err.contains("digest mismatch"), "{err}");
    assert!(err.contains(&local.semantic_contract_digest), "{err}");
}

#[test]
fn matching_digest_passes() {
    let local = build_surface_capability(HandshakeSurface::Mcp);
    check_contract_compatibility(
        &local,
        Some(&local.semantic_contract_digest),
        Some(&local.semantic_contract_version),
    )
    .unwrap();
}

#[test]
fn composition_trace_has_required_fields() {
    let t = composition_trace(
        HandshakeSurface::RawWorker,
        PlannerOwner::Client,
        CompressionOwner::Engine,
        1,
    );
    for key in [
        "planner_owner",
        "compression_owner",
        "surface",
        "contract_digest",
        "boundary_count",
    ] {
        assert!(t.get(key).is_some(), "missing {key} in {t}");
    }
    assert_eq!(t["boundary_count"], 1);
    assert_eq!(t["planner_owner"], "client");
}
