//! PR18 policy descriptor: the single source of truth for TokenZero tools,
//! capabilities, and ZeroRef v1 features served by `resource://tokenzero/capabilities`.

use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokenzero_core::{MCP_SCHEMA_VERSION, McpToolSurface};

use crate::catalog::{
    ResourceSpec, canonical_tool_names_for_surface, canonical_tool_specs, resource_specs,
    tool_clusters,
};
use crate::codemode::journal::{OperationClass, classify_descriptor_tool};
use crate::jsonrpc::{SUPPORTED_PROTOCOL_VERSIONS, tool_filter_discovery};

/// PR18 policy descriptor revision. Bump whenever the tool or capability
/// contract changes.
pub const PR18_DESCRIPTOR_VERSION: &str = "PR18.2";

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
    pub operation_class: OperationClass,
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
    pub portable_ref_kinds: Vec<String>,
    pub unsupported_portable_ref_kinds: Vec<String>,
    pub limitations: Vec<String>,
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
            protocol_versions: strings(SUPPORTED_PROTOCOL_VERSIONS),
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
            next_actions: strings(&[
                "Call tools/list for JSON Schema 2020-12 input contracts.",
                "Read resource://tokenzero/roots before passing paths or cwd.",
                "Inspect tool text output: shell reports command_success inline and other tools carry a refs: footer; set TOKENZERO_MCP_ENVELOPE=compact|full for structuredContent envelopes.",
            ]),
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
            ref_schemes: strings(&["tz://", "fz://", "gz://"]),
            fragment_selectors: strings(&["#B", "#L"]),
            symbol_aware: true,
            diff_baseline: true,
            // Evidence-backed blob expand across engines under a shared CAS
            // (fixtures/zeroref-conformance-evidence.json). Non-blob portable
            // refs remain unsupported.
            cross_engine: true,
            portable_ref_kinds: strings(&["blob"]),
            unsupported_portable_ref_kinds: strings(&[
                "execution",
                "error",
                "session",
                "file",
                "graph",
                "index",
                "unit",
            ]),
            limitations: strings(&[
                "Cross-engine portability is limited to full-hash ZeroRef v1 blob refs and #B/#L fragments.",
                "Correctness evidence does not establish zero-copy, latency, or performance claims.",
            ]),
            features: strings(&[
                "shared-content-addressable-storage",
                "blob-ref-expand",
                "cross-engine-blob-expand",
                "fragment-selectors",
                "symbol-aware-recovery",
                "diff-baseline",
            ]),
        }
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
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
        .iter()
        .map(|seed| ToolCapability {
            name: seed.name.to_string(),
            cluster: seed.cluster.to_string(),
            summary: seed.summary.to_string(),
            capabilities: std::iter::once(seed.cluster)
                .chain(seed.capabilities.iter().copied())
                .map(str::to_string)
                .collect(),
            operation_class: classify_descriptor_tool(seed.name),
        })
        .collect()
}

/// JSON payload for `resource://tokenzero/capabilities`.
/// This is the only place that owns the capabilities wire shape.
pub(crate) fn build_capability_payload(engine: &crate::TokenZeroEngine) -> Value {
    CapabilityDescriptor::for_engine(engine).to_json()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_ref_schema_addition_advances_descriptor_revision() {
        let descriptor = CapabilityDescriptor::for_surface(McpToolSurface::Classic);
        let payload = descriptor.to_json();

        assert_eq!(payload["descriptorVersion"], "PR18.2");
        assert_eq!(payload["zeroref_v1"]["portable_ref_kinds"], json!(["blob"]));
        assert!(payload["zeroref_v1"]["unsupported_portable_ref_kinds"].is_array());
        assert!(payload["zeroref_v1"]["limitations"].is_array());
    }
}
