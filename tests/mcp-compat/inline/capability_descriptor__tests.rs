use super::*;

#[test]
fn portable_ref_schema_addition_advances_descriptor_revision() {
    let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
    let payload = descriptor.to_json();

    assert_eq!(payload["descriptorVersion"], "PR18.5");
    assert_eq!(payload["zeroref"]["portable_ref_kinds"], json!(["blob"]));
    assert_eq!(payload["zeroref"]["cross_engine"], false);
    assert!(payload["zeroref"]["unsupported_portable_ref_kinds"].is_array());
    assert!(payload["zeroref"]["limitations"].is_array());
    assert!(payload["zeroref"].get("clamp_policy").is_none());
    assert!(payload["zeroref"].get("selection_policy").is_none());
    assert!(payload["zeroref"]["limitations"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item
            .as_str()
            .is_some_and(|text| text.contains("clamped line ends"))));
    assert!(
        payload["zeroref"]["limitations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item.as_str().is_some_and(|text| {
                text.contains("not multi-OS proof") || text.contains("ZEROREF_REQUIRE_ALL_OS")
            })),
        "limitations must gate portable claims until multi-OS evidence"
    );
    assert!(payload["zeroref"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item.as_str() != Some("cross-engine-blob-expand")));
}

const CODEMODE_EXCLUSIVE: &[&str] = &[
    "tz_execute_code",
    "tz_codemode_search",
    "tz_codemode_describe",
];

fn tool_names(payload: &Value) -> Vec<String> {
    payload["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn classic_descriptor_advertises_only_callable_classic_tools() {
    let payload = CapabilityDescriptor::for_surface(McpToolSurface::Classic).to_json();
    let canonical: Vec<String> = payload["canonical_tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|name| name.as_str().map(str::to_string))
        .collect();
    let tools = tool_names(&payload);
    assert_eq!(
        tools, canonical,
        "descriptor.tools must match canonical_tools for the served surface"
    );
    for exclusive in CODEMODE_EXCLUSIVE {
        assert!(
            !tools.iter().any(|name| name == exclusive),
            "Classic must not advertise {exclusive}: {tools:?}"
        );
        assert!(
            !canonical.iter().any(|name| name == exclusive),
            "Classic canonical_tools must not list {exclusive}"
        );
    }
    if let Some(codemode) = payload["tool_clusters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["cluster"] == "codemode")
    {
        let members: Vec<&str> = codemode["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for exclusive in CODEMODE_EXCLUSIVE {
            assert!(
                !members.contains(exclusive),
                "Classic must not cluster-advertise uncallable {exclusive}: {members:?}"
            );
        }
        assert_eq!(
            members,
            ["tz_report_tool_issue"],
            "Classic 'codemode' cluster is only the shared report tool, not execute/search/describe"
        );
    }
    let accepted = payload["toolFiltering"]["acceptedParams"]["_meta.tokenzero/toolCluster"]
        .as_array()
        .unwrap();
    let accepted: Vec<&str> = accepted.iter().filter_map(Value::as_str).collect();
    assert_eq!(accepted, ["material", "execution"]);
    assert!(
        !accepted.contains(&"codemode")
            && !accepted.contains(&"edit")
            && !accepted.contains(&"web"),
        "accepted toolCluster filters must match tools/list, got {accepted:?}"
    );
}

#[test]
fn classic_mcp_does_not_advertise_undispatched_decision_views() {
    use crate::catalog::{tool_docs_for_surface, tool_specs};
    use crate::fastmcp_mode::{fastmcp_codemode_instructions, fastmcp_instructions};

    const NEEDLES: &[&str] = &[
        "decision view",
        "decisionview",
        "reasoning-state",
        "opaque reasoning",
        "output novelty",
        "outputnovelty",
        "continuation class",
        "continuationkind",
        "decisionviewheadroom",
        "dv headroom",
        "decision_view",
        "decision-view",
        "reasoning_state",
        "output_novelty",
        "continuation_class",
        "headroom",
    ];
    let haystacks = [
        serde_json::to_string(&tool_specs()).expect("tool_specs serializable"),
        serde_json::to_string(&tool_docs_for_surface(McpToolSurface::Classic))
            .expect("classic tool docs serializable"),
        CapabilityDescriptor::for_surface(McpToolSurface::Classic)
            .to_json()
            .to_string(),
        fastmcp_instructions().to_string(),
        fastmcp_codemode_instructions().to_string(),
    ];
    for (i, haystack) in haystacks.iter().enumerate() {
        let lower = haystack.to_lowercase();
        for needle in NEEDLES {
            assert!(
                !lower.contains(needle),
                "Classic MCP catalog haystack {i} advertises undispatched {needle:?}"
            );
        }
    }
}

#[test]
fn classic_mcp_does_not_advertise_missing_strict_mode() {
    use crate::catalog::{tool_docs_for_surface, tool_specs};
    use crate::fastmcp_mode::{fastmcp_codemode_instructions, fastmcp_instructions};

    const NEEDLES: &[&str] = &[
        "strict mode",
        "strict-mode",
        "strict_mode",
        "strictmode",
    ];
    let haystacks = [
        serde_json::to_string(&tool_specs()).expect("tool_specs serializable"),
        serde_json::to_string(&tool_docs_for_surface(McpToolSurface::Classic))
            .expect("classic tool docs serializable"),
        CapabilityDescriptor::for_surface(McpToolSurface::Classic)
            .to_json()
            .to_string(),
        fastmcp_instructions().to_string(),
        fastmcp_codemode_instructions().to_string(),
    ];
    for (i, haystack) in haystacks.iter().enumerate() {
        let lower = haystack.to_lowercase();
        for needle in NEEDLES {
            assert!(
                !lower.contains(needle),
                "Classic MCP catalog haystack {i} advertises missing strict-mode as present ({needle:?})"
            );
        }
    }
}

#[test]
fn classic_tools_resource_omits_codemode_exclusive_tools() {
    use crate::catalog::tool_docs_for_surface;
    let docs = tool_docs_for_surface(McpToolSurface::Classic);
    let names: Vec<&str> = docs.iter().filter_map(|doc| doc["name"].as_str()).collect();
    for exclusive in CODEMODE_EXCLUSIVE {
        assert!(
            !names.contains(exclusive),
            "resource://tokenzero/tools on Classic must not document {exclusive}: {names:?}"
        );
    }
    assert!(
        names.contains(&"tz_read") && names.contains(&"tz_shell"),
        "Classic catalog must still document per-op tools: {names:?}"
    );
}
