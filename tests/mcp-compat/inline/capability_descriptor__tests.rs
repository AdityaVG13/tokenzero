use super::*;

#[test]
fn portable_ref_schema_addition_advances_descriptor_revision() {
    let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
    let payload = descriptor.to_json();

    assert_eq!(payload["descriptorVersion"], "PR18.4");
    assert_eq!(payload["zeroref_v1"]["portable_ref_kinds"], json!(["blob"]));
    assert_eq!(payload["zeroref_v1"]["cross_engine"], false);
    assert!(payload["zeroref_v1"]["unsupported_portable_ref_kinds"].is_array());
    assert!(payload["zeroref_v1"]["limitations"].is_array());
    assert!(payload["zeroref_v1"].get("clamp_policy").is_none());
    assert!(payload["zeroref_v1"].get("selection_policy").is_none());
    assert!(
        payload["zeroref_v1"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item
                .as_str()
                .is_some_and(|text| text.contains("clamped line ends")))
    );
    assert!(
        payload["zeroref_v1"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().is_some_and(|text| {
                text.contains("not multi-OS proof") || text.contains("ZEROREF_REQUIRE_ALL_OS")
            })),
        "limitations must gate portable claims until multi-OS evidence"
    );
    assert!(
        payload["zeroref_v1"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item.as_str() != Some("cross-engine-blob-expand"))
    );
}
