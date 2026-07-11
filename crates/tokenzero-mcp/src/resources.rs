use serde_json::{json, Value};
use tokenzero_core::MCP_SCHEMA_VERSION;

use crate::catalog::{resource_specs, tool_clusters, tool_docs};
use crate::codemode::catalog::codemode_method_catalog;
use crate::jsonrpc::{tool_filter_discovery, JsonRpcErrorData};
use crate::TokenZeroEngine;

/// Build the JSON payload string for a resource URI. Used by both the
/// hand-rolled resources/read dispatch and the FastMCP ResourceHandler impls.
pub(crate) fn build_resource_payload(
    engine: &TokenZeroEngine,
    uri: &str,
) -> Result<String, JsonRpcErrorData> {
    let _resource = resource_specs()
        .into_iter()
        .find(|resource| resource.uri == uri)
        .ok_or_else(|| JsonRpcErrorData::unknown_resource(uri))?;

    let payload = match uri {
        "resource://tokenzero/capabilities" => {
            crate::capability_descriptor::build_capability_payload(engine)
        }
        "resource://tokenzero/codemode" => codemode_method_catalog(),
        "resource://tokenzero/tools" => json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "tools": tool_docs(),
            "tool_clusters": tool_clusters(),
            "toolFiltering": tool_filter_discovery(engine.config.tool_surface),
            "next_actions": ["Use canonical tz_* names in durable instructions; aliases exist for client ergonomics."]
        }),
        "resource://tokenzero/roots" => json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "effective_allowed_roots": engine
                .config
                .allowed_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "allowed_roots": engine
                .config
                .allowed_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "path_rule": "read/find/grep/glob/tree paths and shell cwd must stay under one allowed root.",
            "allowlist_algorithm": "effective roots = call root (CodeMode options.root / CLI --root / zero_execute root) union configured --allowed-root entries, deduped by canonical path. Relative paths join to the execute root; absolute paths under that root are allowed; paths outside every effective root are denied.",
            "next_actions": ["Use tree or glob under one allowed root before reading files.", "Pass root= on tz_execute_code / tokenzero codemode --root for foreign workspaces."]
        }),
        "resource://tokenzero/modes" => json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "modes": [
                {"name": "auto", "use": "default compacting policy"},
                {"name": "passthrough", "use": "show visible output without compacting"},
                {"name": "diagnostic", "use": "surface failures, assertions, and warnings first"},
                {"name": "structured", "use": "summarize larger structured content"},
                {"name": "dedupe", "use": "collapse repeated lines"},
                {"name": "diff-aware", "use": "preserve changed paths and hunks"},
                {"name": "exact", "use": "store exact payload and require expand for visible recovery"},
                {"name": "hybrid", "use": "legacy alias for auto"},
                {"name": "critical", "use": "legacy alias for diagnostic"},
                {"name": "fidelity", "use": "legacy alias for structured"}
            ]
        }),
        "resource://tokenzero/cache" => json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "cache_path": engine.config.cache_path.display().to_string(),
            "max_visible_tokens": engine.config.max_visible_tokens,
            "shell_capture_bytes": engine.config.shell_capture_bytes,
            "shell_spill_bytes": engine.config.shell_spill_bytes,
            "mcp_idle_timeout_seconds": engine.config.mcp_idle_timeout.map(|timeout| timeout.as_secs()),
            "privacy": "Raw payloads stay local behind tz:// refs; this resource does not expose cached payload text.",
            "store_isolation": "Default store is per call root (wqw.2). ZEROSTACK_STORE_ROOT is honored only with TOKENZERO_SHARED_STORE/ZEROSTACK_SHARED_STORE opt-in; otherwise each project uses root/.zerostack or root/.tokenzero.",
            "shared_store_opt_in_envs": ["TOKENZERO_SHARED_STORE", "ZEROSTACK_SHARED_STORE"],
            "engine_binaries": crate::engine_binaries_json(),
        }),
        "resource://tokenzero/session-boot" => engine.session_boot_snapshot(),
        "resource://tokenzero/metrics" => engine.tool_metrics_snapshot(),
        "resource://tokenzero/shell-contract" => {
            return Ok([
                "# TokenZero Shell Contract",
                "",
                "- MCP transport success is separate from child command success.",
                "- Inspect `command_success`, `exit_code`, and `status_label` in the shell text output (or `structuredContent.cli.telemetry` with TOKENZERO_MCP_ENVELOPE=compact|full).",
                "- Stdout, stderr, combined output, and capture records are stored behind refs when available.",
                "- Use `cwd` instead of shell `cd` when choosing a working directory.",
                "- Use read-only commands for safe retries; mutating commands are not generally idempotent.",
            ]
            .join("\n"));
        }
        _ => unreachable!("resource was already resolved"),
    };

    let text = serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string());
    Ok(text)
}

pub(crate) fn read_resource(
    engine: &TokenZeroEngine,
    uri: &str,
) -> Result<Value, JsonRpcErrorData> {
    let resource = resource_specs()
        .into_iter()
        .find(|resource| resource.uri == uri)
        .ok_or_else(|| JsonRpcErrorData::unknown_resource(uri))?;

    let text = build_resource_payload(engine, uri)?;

    Ok(resource_read_result(
        &resource.uri,
        &resource.mime_type,
        text,
    ))
}

fn resource_read_result(uri: &str, mime_type: &str, text: String) -> Value {
    json!({
        "contents": [{
            "uri": uri,
            "mimeType": mime_type,
            "text": text
        }],
        "resultType": "complete",
        "ttlMs": 60000,
        "cacheScope": "workspace"
    })
}
