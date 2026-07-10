//! PR18 policy descriptor: the single source of truth for TokenZero tools,
//! capabilities, and ZeroRef v1 features served by `resource://tokenzero/capabilities`.

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokenzero_core::{McpToolSurface, MCP_SCHEMA_VERSION};

use crate::catalog::{
    canonical_tool_names_for_surface, canonical_tool_specs, resource_specs, tool_clusters,
    ResourceSpec,
};
use crate::jsonrpc::{tool_filter_discovery, SUPPORTED_PROTOCOL_VERSIONS};

/// PR18 policy descriptor revision. Bump whenever the tool or capability
/// contract changes.
pub const PR18_DESCRIPTOR_VERSION: &str = "PR18.1";

/// Machine-readable policy descriptor enumerating every TokenZero tool,
/// capability tag, and ZeroRef v1 feature.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CapabilityDescriptor {
    #[serde(rename = "descriptorVersion")]
    pub descriptor_version: String,
    pub schema_version: String,
    pub status: String,
    pub server: String,
    pub version: String,
    #[serde(rename = "protocolVersions")]
    pub protocol_versions: Vec<String>,
    pub tool_surface: String,
    pub canonical_tools: Vec<String>,
    pub aliases: BTreeMap<String, String>,
    pub tool_clusters: Value,
    #[serde(rename = "toolFiltering")]
    pub tool_filtering: Value,
    pub tools: Vec<ToolCapability>,
    pub zeroref_v1: ZeroRefCapabilities,
    pub codemode: Value,
    pub resources: Vec<ResourceSpec>,
    pub next_actions: Vec<String>,
}

/// Capability record for one exposed tool.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCapability {
    pub name: String,
    pub cluster: String,
    pub summary: String,
    pub capabilities: Vec<String>,
}

/// ZeroRef v1 capability contract.
#[derive(Debug, Clone, Serialize)]
pub struct ZeroRefCapabilities {
    pub version: String,
    pub enabled: bool,
    pub shared_cas: bool,
    pub blob_ref_expand: bool,
    pub ref_schemes: Vec<String>,
    pub fragment_selectors: Vec<String>,
    pub symbol_aware: bool,
    pub diff_baseline: bool,
    pub cross_engine: bool,
    pub features: Vec<String>,
}

impl CapabilityDescriptor {
    /// Build the descriptor for a given MCP tool surface.
    pub fn for_surface(surface: McpToolSurface) -> Self {
        let canonical_tools = canonical_tool_names_for_surface(surface);
        let aliases = alias_map_for_surface(surface);
        let tools = build_all_tool_capabilities();
        Self {
            descriptor_version: PR18_DESCRIPTOR_VERSION.to_string(),
            schema_version: MCP_SCHEMA_VERSION.to_string(),
            status: "ok".to_string(),
            server: "tokenzero".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_versions: SUPPORTED_PROTOCOL_VERSIONS.iter().map(|s| s.to_string()).collect(),
            tool_surface: surface.as_str().to_string(),
            canonical_tools,
            aliases,
            tool_clusters: tool_clusters(),
            tool_filtering: tool_filter_discovery(surface),
            tools,
            zeroref_v1: ZeroRefCapabilities::default(),
            codemode: json!({
                "schema": "tokenzero.codemode.v1",
                "cli": "tokenzero codemode --json --plan '<plan>'",
                "note": "CodeMode is a separate plan-based execution layer on the same base tools/engine (Cloudflare-style, fewer round-trips). Use `tokenzero codemode` or resource://tokenzero/codemode for discovery."
            }),
            resources: resource_specs(),
            next_actions: vec![
                "Call tools/list for JSON Schema 2020-12 input contracts.".to_string(),
                "Read resource://tokenzero/roots before passing paths or cwd.".to_string(),
                "Inspect tool text output: shell reports command_success inline and other tools carry a refs: footer; set TOKENZERO_MCP_ENVELOPE=compact|full for structuredContent envelopes.".to_string(),
            ],
        }
    }

    /// Build the descriptor from an engine instance (its configured surface).
    pub fn for_engine(engine: &crate::TokenZeroEngine) -> Self {
        Self::for_surface(engine.config.tool_surface)
    }

    /// Serialize the descriptor to a JSON value.
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }
}

impl Default for ZeroRefCapabilities {
    fn default() -> Self {
        Self {
            version: "v1".to_string(),
            enabled: true,
            shared_cas: true,
            blob_ref_expand: true,
            ref_schemes: vec![
                "tz://".to_string(),
                "fz://".to_string(),
                "gz://".to_string(),
            ],
            fragment_selectors: vec!["#B".to_string(), "#L".to_string()],
            symbol_aware: true,
            diff_baseline: true,
            cross_engine: false,
            features: vec![
                "shared-content-addressable-storage".to_string(),
                "blob-ref-expand".to_string(),
                "fragment-selectors".to_string(),
                "symbol-aware-recovery".to_string(),
                "diff-baseline".to_string(),
            ],
        }
    }
}

fn alias_map_for_surface(surface: McpToolSurface) -> BTreeMap<String, String> {
    use crate::catalog::TOOL_ALIASES;
    use crate::surface_health::surface_includes;
    let mut map = BTreeMap::new();
    for &(alias, target) in TOOL_ALIASES {
        if surface_includes(surface, target) {
            map.insert(alias.to_string(), target.to_string());
        }
    }
    map
}

fn build_all_tool_capabilities() -> Vec<ToolCapability> {
    canonical_tool_specs()
        .into_iter()
        .map(|seed| ToolCapability {
            name: seed.name.to_string(),
            cluster: seed.cluster.to_string(),
            summary: seed.summary.to_string(),
            capabilities: capabilities_for_tool(seed.name, seed.cluster),
        })
        .collect()
}

fn capabilities_for_tool(name: &str, cluster: &str) -> Vec<String> {
    let mut caps = Vec::new();
    caps.push(cluster.to_string());
    let extra: &[&str] = match name {
        "tz_read" => &["read", "exact-refs", "line-range", "shared-cas"],
        "tz_find" => &["search", "literal", "exact-refs", "shared-cas"],
        "tz_grep" => &["search", "regex", "exact-refs", "shared-cas"],
        "tz_recall" => &["search", "cache", "exact-refs", "shared-cas"],
        "tz_glob" => &["discover", "glob", "shared-cas"],
        "tz_tree" => &["discover", "tree", "shared-cas"],
        "tz_expand" => &[
            "expand",
            "exact-refs",
            "fragment-selectors",
            "symbol-anchors",
            "diff-baseline",
            "shared-cas",
        ],
        "tz_edit" => &["write", "atomic", "exact-refs"],
        "tz_shell" => &["shell", "exact-refs", "command-success"],
        "tz_fetch" => &["fetch", "web", "cache", "exact-refs"],
        "tz_ingest" => &["ingest", "exact-refs"],
        "tz_batch" => &["batch", "exact-refs"],
        "tz_mem" => &["diagnostic", "cache"],
        "tz_cache_pack" => &["cache", "prompt-cache"],
        "tz_rewrite" => &["diagnostic", "rewrite"],
        "tz_discover" => &["diagnostic", "discovery"],
        "tz_report_tool_issue" => &["diagnostic", "report"],
        "tz_execute_code" => &["codemode", "plan-execution", "sandboxed"],
        "tz_codemode_search" => &["codemode", "catalog-search", "read-only"],
        "tz_codemode_describe" => &["codemode", "catalog-describe", "read-only"],
        _ => &[],
    };
    caps.extend(extra.iter().map(|s| s.to_string()));
    caps
}

/// JSON payload for `resource://tokenzero/capabilities`.
/// This is the only place that owns the capabilities wire shape.
pub(crate) fn build_capability_payload(engine: &crate::TokenZeroEngine) -> Value {
    CapabilityDescriptor::for_engine(engine).to_json()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokenzero_core::McpToolSurface;

    #[test]
    fn descriptor_lists_all_expected_canonical_tools() {
        let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
        let expected = [
            "tz_execute_code",
            "tz_codemode_search",
            "tz_codemode_describe",
            "tz_read",
            "tz_find",
            "tz_grep",
            "tz_recall",
            "tz_batch",
            "tz_fetch",
            "tz_glob",
            "tz_tree",
            "tz_edit",
            "tz_shell",
            "tz_ingest",
            "tz_expand",
            "tz_mem",
            "tz_cache_pack",
            "tz_rewrite",
            "tz_discover",
            "tz_report_tool_issue",
        ];
        let names: std::collections::HashSet<String> =
            descriptor.tools.iter().map(|t| t.name.clone()).collect();
        assert_eq!(names.len(), expected.len(), "tool count mismatch");
        for name in expected {
            assert!(
                names.contains(name),
                "expected tool {name} missing from descriptor"
            );
        }
    }

    #[test]
    fn descriptor_exposes_zeroref_v1_capabilities() {
        let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
        assert!(descriptor.zeroref_v1.enabled);
        assert!(descriptor.zeroref_v1.shared_cas);
        assert!(descriptor.zeroref_v1.blob_ref_expand);
        assert_eq!(descriptor.zeroref_v1.version, "v1");
        assert!(
            descriptor
                .zeroref_v1
                .features
                .contains(&"shared-content-addressable-storage".to_string()),
            "expected shared-cas feature"
        );
        assert!(
            descriptor
                .zeroref_v1
                .features
                .contains(&"blob-ref-expand".to_string()),
            "expected blob-ref-expand feature"
        );
    }

    #[test]
    fn descriptor_to_json_is_object() {
        let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
        let json = descriptor.to_json();
        assert!(json.is_object());
        assert_eq!(json["descriptorVersion"], PR18_DESCRIPTOR_VERSION);
        assert_eq!(json["schema_version"], MCP_SCHEMA_VERSION);
        assert!(json["tools"].as_array().is_some_and(|a| !a.is_empty()));
    }
}
