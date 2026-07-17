use fastmcp_rust::McpErrorCode;
use fastmcp_rust::ResourceHandler;
use fastmcp_rust::ToolHandler;
use fastmcp_rust::prelude::*;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokenzero_core::count_tokens;

use crate::catalog::{TOOL_ALIASES, canonical_tool_specs, resource_specs};
use crate::resources::build_resource_payload;
use crate::surface_health::surface_includes;
use crate::{EngineConfig, TokenZeroEngine, call_tool_fastmcp};
use tokenzero_core::McpToolSurface;

/// A single engine-backed tool that delegates to the existing dispatch path,
/// keeping tool-surface parity byte-level.
struct EngineTool {
    name: String,
    description: String,
    schema: Value,
    engine: Arc<Mutex<TokenZeroEngine>>,
}

pub(crate) fn fastmcp_content_texts_from_tool_result(
    result: &Value,
) -> Result<Vec<String>, String> {
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if is_error {
        let err_text = result
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("tool error");
        return Err(err_text.to_string());
    }
    let primary_text = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(sc) = result.get("structuredContent") {
        if let Some(folded) = scalar_folded_codemode_v2(primary_text, sc) {
            return Ok(vec![folded]);
        }
        let meta_json = serde_json::to_string(sc).unwrap_or_default();
        if meta_json != "null" {
            return Ok(vec![primary_text.to_string(), meta_json]);
        }
        return Ok(vec![primary_text.to_string()]);
    }
    let mut contents = vec![primary_text.to_string()];
    if primary_text.starts_with("ok tz") || primary_text.starts_with("err ") {
        return Ok(contents);
    }
    let mut meta = serde_json::Map::new();
    if let Some(rt) = result.get("resultType").and_then(Value::as_str) {
        meta.insert("resultType".into(), Value::String(rt.to_string()));
    }
    let meta_json = serde_json::to_string(&Value::Object(meta)).unwrap_or_default();
    if meta_json != "{}" {
        contents.push(meta_json);
    }
    Ok(contents)
}

fn scalar_folded_codemode_v2(primary_text: &str, structured: &Value) -> Option<String> {
    let object = structured.as_object()?;
    let value = object.get("value")?;
    if !(value.is_string() || value.is_number() || value.is_boolean()) {
        return None;
    }
    let value_text = serde_json::to_string(value).ok()?;
    if count_tokens(&value_text) > 16 {
        return None;
    }
    let ack = object
        .get("ack")
        .and_then(Value::as_str)
        .unwrap_or(primary_text);
    // Idempotent: the tools layer may already have folded this scalar into
    // the ack; folding again rendered doubled values (=true =true).
    if ack.contains(&format!(" ={value_text}")) {
        return Some(ack.to_string());
    }
    let (prefix, suffix) = ack.rsplit_once(" t:")?;
    Some(format!("{prefix} ={value_text} t:{suffix}"))
}

impl ToolHandler for EngineTool {
    fn definition(&self) -> Tool {
        Tool {
            name: self.name.clone(),
            description: Some(self.description.clone()),
            input_schema: self.schema.clone(),
            output_schema: None,
            icon: None,
            version: None,
            tags: Vec::new(),
            annotations: None,
        }
    }

    fn call(&self, ctx: &McpContext, arguments: Value) -> McpResult<Vec<Content>> {
        ctx.checkpoint()?;
        let engine = self.engine.lock().expect("engine mutex");
        match call_tool_fastmcp(&engine, &self.name, &arguments, None) {
            Ok(result) => {
                // Dual-content pattern: content[0] = old primary text VERBATIM
                // (visible-token parity), content[1] = compact metadata for
                // fields FastMCP cannot carry natively (resultType, refs).
                // Errors — tool-level isError or dispatch-level — map to
                // Err(McpError) so fastmcp sets the envelope isError: true.
                match fastmcp_content_texts_from_tool_result(&result) {
                    Ok(contents) => Ok(contents.into_iter().map(Content::text).collect()),
                    Err(message) => Err(McpError::new(McpErrorCode::Custom(-32000), message)),
                }
            }
            Err(err) => {
                let message = err.message_text();
                Err(McpError::new(McpErrorCode::Custom(-32000), message))
            }
        }
    }
}

/// A single engine-backed resource that delegates to the existing resource-payload
/// builder, keeping resource-surface parity byte-level.
struct TokenZeroResource {
    uri: String,
    name: String,
    description: String,
    mime_type: String,
    engine: Arc<Mutex<TokenZeroEngine>>,
}

impl ResourceHandler for TokenZeroResource {
    fn definition(&self) -> Resource {
        Resource {
            uri: self.uri.clone(),
            name: self.name.clone(),
            description: Some(self.description.clone()),
            mime_type: Some(self.mime_type.clone()),
            icon: None,
            version: None,
            tags: Vec::new(),
        }
    }

    fn read(&self, _ctx: &McpContext) -> McpResult<Vec<ResourceContent>> {
        let engine = self.engine.lock().expect("engine mutex");
        match build_resource_payload(&engine, &self.uri) {
            Ok(text) => Ok(vec![ResourceContent {
                uri: self.uri.clone(),
                mime_type: Some(self.mime_type.clone()),
                text: Some(text),
                blob: None,
            }]),
            Err(err) => Err(McpError::new(
                McpErrorCode::Custom(-32000),
                err.message_text(),
            )),
        }
    }
}

/// One-mode instruction text for FastMCP .instructions().
pub fn fastmcp_instructions() -> &'static str {
    "TokenZero MCP surface. Tools: read, find, grep, glob, tree, edit, recall, batch, \
     fetch, shell, ingest, expand, mem, cache_pack, rewrite, discover, plus tz_* aliases. \
     If your harness supports ZeroStack CodeMode (zero_execute plans), use the CodeMode \
     mode INSTEAD — never both at once. \
     Full per-tool docs: resources/read resource://tokenzero/tools."
}

/// CodeMode-mode instruction text for FastMCP .instructions().
pub fn fastmcp_codemode_instructions() -> &'static str {
    "TokenZero CodeMode surface. Tools: tz_execute_code, tz_codemode_search, \
     tz_codemode_describe, tz_report_tool_issue. Expand/read fallback is \
     engine-internal — per-op MCP tools (tz_expand, tz_read, shell, …) are not \
     listed. Write plans against the `zero` surface. Use tz_codemode_describe \
     name=capabilities for the full contract manifest."
}

/// Start the FastMCP stdio server, replacing the hand-rolled loop.
/// The CodeMode transport is UNTOUCHED — this is TOOL-level FastMCP wiring.
pub fn run_fastmcp_stdio(config: EngineConfig) -> ! {
    let surface = config.tool_surface;
    let engine = Arc::new(Mutex::new(TokenZeroEngine::new(config)));

    let instructions = match surface {
        McpToolSurface::CodeMode => fastmcp_codemode_instructions(),
        McpToolSurface::Classic => fastmcp_instructions(),
    };
    let mut builder =
        Server::new("TokenZero", env!("CARGO_PKG_VERSION")).instructions(instructions);

    for seed in canonical_tool_specs() {
        // Same policy owner as tools/list / tools/call; CodeMode registers
        // primary tools only (expand fallback is engine-internal).
        if !surface_includes(surface, seed.name) {
            continue;
        }
        let handler = EngineTool {
            name: seed.name.to_string(),
            description: seed.summary.to_string(),
            schema: seed.input_schema.clone(),
            engine: Arc::clone(&engine),
        };
        builder = builder.tool(handler);
    }

    // Register alias tool names (read -> tz_read, etc.) so clients see them in tools/list.
    // Only include aliases whose target is on the active surface.
    for &(alias, target) in TOOL_ALIASES {
        if !surface_includes(surface, target) {
            continue;
        }
        let handler = EngineTool {
            name: alias.to_string(),
            description: crate::catalog::alias_summary(target),
            schema: serde_json::json!({"type": "object"}),
            engine: Arc::clone(&engine),
        };
        builder = builder.tool(handler);
    }

    // Register every resource the old hand-rolled surface served.
    for spec in resource_specs() {
        let handler = TokenZeroResource {
            uri: spec.uri.clone(),
            name: spec.name.clone(),
            description: spec.description.clone(),
            mime_type: spec.mime_type.clone(),
            engine: Arc::clone(&engine),
        };
        builder = builder.resource(handler);
    }

    builder.build().run_stdio()
}
