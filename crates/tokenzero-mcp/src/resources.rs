use serde_json::{Value, json};
use tokenzero_core::MCP_SCHEMA_VERSION;

use crate::TokenZeroEngine;
use crate::catalog::{canonical_tool_names_for_surface, resource_specs, tool_clusters, tool_docs};
use crate::codemode::catalog::codemode_method_catalog;
use crate::jsonrpc::{JsonRpcErrorData, SUPPORTED_PROTOCOL_VERSIONS, tool_filter_discovery};

pub(crate) fn read_resource(
    engine: &TokenZeroEngine,
    uri: &str,
) -> Result<Value, JsonRpcErrorData> {
    let resource = resource_specs()
        .into_iter()
        .find(|resource| resource.uri == uri)
        .ok_or_else(|| JsonRpcErrorData::unknown_resource(uri))?;

    let payload = match uri {
        "resource://tokenzero/capabilities" => json!({
            "schema_version": MCP_SCHEMA_VERSION,
            "status": "ok",
            "server": "tokenzero",
            "version": env!("CARGO_PKG_VERSION"),
            "protocolVersions": SUPPORTED_PROTOCOL_VERSIONS,
            "tool_clusters": tool_clusters(),
            "toolFiltering": tool_filter_discovery(engine.config.tool_surface),
            "tool_surface": engine.config.tool_surface.as_str(),
            "canonical_tools": canonical_tool_names_for_surface(engine.config.tool_surface),
            "aliases": {
                "read": "tz_read",
                "find": "tz_find",
                "grep": "tz_grep",
                "glob": "tz_glob",
                "tree": "tz_tree",
                "edit": "tz_edit",
                "recall": "tz_recall",
                "batch": "tz_batch",
                "fetch": "tz_fetch",
                "shell": "tz_shell",
                "ingest": "tz_ingest",
                "expand": "tz_expand",
                "mem": "tz_mem",
                "cache_pack": "tz_cache_pack",
                "cache-pack": "tz_cache_pack",
                "rewrite": "tz_rewrite",
                "discover": "tz_discover",
            },
            "codemode": {
                "schema": "tokenzero.codemode.v1",
                "cli": "tokenzero codemode --json --plan '<plan>'",
                "note": "CodeMode is a separate plan-based execution layer on the same base tools/engine (Cloudflare-style, fewer round-trips). Use `tokenzero codemode` or resource://tokenzero/codemode for discovery."
            },
            "resources": resource_specs(),
            "next_actions": [
                "Call tools/list for JSON Schema 2020-12 input contracts.",
                "Read resource://tokenzero/roots before passing paths or cwd.",
                "Inspect tool text output: shell reports command_success inline and other tools carry a refs: footer; set TOKENZERO_MCP_ENVELOPE=compact|full for structuredContent envelopes."
            ]
        }),
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
            "allowed_roots": engine
                .config
                .allowed_roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>(),
            "path_rule": "read/find/grep/glob/tree paths and shell cwd must stay under one allowed root.",
            "next_actions": ["Use tree or glob under one allowed root before reading files."]
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
            "privacy": "Raw payloads stay local behind tz:// refs; this resource does not expose cached payload text."
        }),
        "resource://tokenzero/metrics" => engine.tool_metrics_snapshot(),
        "resource://tokenzero/shell-contract" => {
            return Ok(resource_read_result(
                &resource.uri,
                &resource.mime_type,
                [
                    "# TokenZero Shell Contract",
                    "",
                    "- MCP transport success is separate from child command success.",
                    "- Inspect `command_success`, `exit_code`, and `status_label` in the shell text output (or `structuredContent.cli.telemetry` with TOKENZERO_MCP_ENVELOPE=compact|full).",
                    "- Stdout, stderr, combined output, and capture records are stored behind refs when available.",
                    "- Use `cwd` instead of shell `cd` when choosing a working directory.",
                    "- Use read-only commands for safe retries; mutating commands are not generally idempotent.",
                ]
                .join("\n"),
            ));
        }
        _ => unreachable!("resource was already resolved"),
    };

    Ok(resource_read_result(
        &resource.uri,
        &resource.mime_type,
        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string()),
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
