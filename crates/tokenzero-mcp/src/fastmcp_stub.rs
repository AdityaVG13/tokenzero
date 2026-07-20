//! FastMCP surface stub when `surface-mcp` is not compiled (tokenzero-codemode-only).
//!
//! Hand-rolled stdio (`run_stdio`) remains available for CodeMode catalog serving.
//! Calling `run_fastmcp_stdio` fails closed instead of linking fastmcp-rust.

use crate::EngineConfig;
use serde_json::Value;

/// Shared by benches/tests that normalize FastMCP wire texts; without FastMCP
/// the function is unavailable and callers must be feature-gated.
#[allow(dead_code)]
pub(crate) fn fastmcp_content_texts_from_tool_result(
    _result: &Value,
) -> Result<Vec<String>, String> {
    Err(
        "FastMCP content rendering was not compiled into this artifact \
(missing feature surface-mcp / fastmcp-rust). Install tokenzero-mcp."
            .into(),
    )
}

pub fn fastmcp_instructions() -> &'static str {
    "TokenZero MCP surface is not compiled into this artifact. Install tokenzero-mcp."
}

pub fn fastmcp_codemode_instructions() -> &'static str {
    "TokenZero CodeMode surface is served via hand-rolled stdio on this artifact. \
Install tokenzero-mcp for the FastMCP per-op catalog."
}

/// Fail closed — single-surface codemode packages use `run_stdio` instead.
pub fn run_fastmcp_stdio(_config: EngineConfig) -> ! {
    eprintln!(
        "tokenzero: FastMCP surface was not compiled into this artifact \
(missing feature surface-mcp / fastmcp-rust). Install tokenzero-mcp for the \
per-operation FastMCP catalog, or launch with the hand-rolled CodeMode stdio path."
    );
    std::process::exit(2);
}
