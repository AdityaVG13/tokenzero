//! Deterministic semantic contract digest.

use serde_json::{Value, json};

use super::registry::all_operations;
use super::schema::{normalize_schema, schema_fingerprint_hex};
use super::types::SEMANTIC_CONTRACT_VERSION;

/// Full contract manifest used as digest input.
///
/// Embeds **complete** normalized input and output schemas (not property-name
/// sets alone) so type/required/nested-constraint drift changes the digest.
pub fn contract_manifest() -> Value {
    let mut ops = Vec::new();
    for op in all_operations() {
        let mut aliases: Vec<&str> = op.aliases.to_vec();
        aliases.sort_unstable();
        let mut error_kinds: Vec<&str> = op.error_kinds.iter().map(|k| k.as_str()).collect();
        error_kinds.sort_unstable();
        let mut capabilities: Vec<&str> = op.capabilities.to_vec();
        capabilities.sort_unstable();
        let input_schema = normalize_schema(&op.args.schema);
        let output_schema = normalize_schema(&op.results.schema);
        ops.push(json!({
            "name": op.name,
            "aliases": aliases,
            "cluster": op.cluster,
            "capabilities": capabilities,
            "mutability": op.mutability.as_str(),
            "capability": op.capability.as_str(),
            "cost_class": op.cost_class.as_str(),
            "ref_ownership": op.ref_ownership.as_str(),
            "cancellation": op.cancellation.as_str(),
            "migration": op.migration.as_str(),
            "fastmcp_tool": op.exposure.fastmcp_tool,
            "codemode_mcp_tool": op.exposure.codemode_mcp_tool,
            "codemode_binding": op.exposure.codemode_binding,
            "resource_uri": op.exposure.resource_uri,
            "input_schema": input_schema,
            "output_schema": output_schema,
            "input_schema_fingerprint": schema_fingerprint_hex(&op.args.schema),
            "output_schema_fingerprint": schema_fingerprint_hex(&op.results.schema),
            "error_kinds": error_kinds,
            "arg_aliases": &op.arg_aliases,
        }));
    }
    json!({
        "semantic_contract_version": SEMANTIC_CONTRACT_VERSION,
        "engine": "tokenzero",
        "schema_parity": "structural_io_v1",
        "operations": ops,
    })
}

/// Raw digest bytes (SHA-256 over canonical JSON).
///
/// Memoized process-wide: the registry is static, so recomputing the full
/// manifest hash on every handshake/SBOM/doctor call was pure overhead
/// (tokenzero-irx9.9 hot-path).
pub fn contract_digest() -> [u8; 32] {
    static DIGEST: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    *DIGEST.get_or_init(|| zero_abi::contract_digest(&contract_manifest()))
}

/// Lowercase hex digest (64 chars). Deterministic across builds for the same registry.
pub fn contract_digest_hex() -> String {
    static HEX: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HEX.get_or_init(|| zero_abi::contract_digest_hex(&contract_manifest()))
        .clone()
}
