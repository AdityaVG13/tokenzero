#![forbid(unsafe_code)]
//! TokenZero MCP / FastMCP / CodeMode transport adapters.
//!
//! Domain execution lives in [`tokenzero_engine`]. This crate depends inward and
//! must not re-implement domain auth/root/mutation/ref/telemetry semantics.

// Process/artifact mutual exclusion (tokenzero-irx9.3): one process must never
// compile both user-facing surface runtimes (fastmcp-rust + rquickjs).
#[cfg(all(feature = "surface-mcp", feature = "surface-codemode"))]
compile_error!(
    "tokenzero surfaces are mutually exclusive (tokenzero-irx9.3): enable exactly one of \
feature surface-mcp or surface-codemode -- never both. One process must not contain both \
catalogs. Build tokenzero-mcp with surface-mcp, or tokenzero-codemode with surface-codemode."
);
mod capability_descriptor;
mod catalog;
mod codemode;
#[cfg(feature = "surface-mcp")]
mod fastmcp_mode;
#[cfg(not(feature = "surface-mcp"))]
mod fastmcp_stub;
#[cfg(not(feature = "surface-mcp"))]
use fastmcp_stub as fastmcp_mode;
mod jsonrpc;
mod operation_abi_parity;
mod resources;
mod stdio;
mod supervisor;
mod tools;

#[cfg(test)]
mod tests;

// Re-export the domain engine (types, dispatcher, modules) for CLI + adapters.
pub use tokenzero_engine::*;
pub use tokenzero_engine::{
    binary_resolve, cache_maintenance, config, expand_params, ledger, metrics, paths, render,
    session, session_persist, surface_health, usage_telemetry, wall, workspace, write_ladder,
};

pub use catalog::{ResourceSpec, ToolSpec, resource_specs, tool_specs};
pub use codemode::{
    CODEMODE_SCHEMA, CodeModeLimits, CodeModeOptions, CodeModeResult, CodeModeStatus,
    describe_codemode_method, execute_codemode_with_options, search_codemode_catalog,
};
pub use fastmcp_mode::{fastmcp_codemode_instructions, fastmcp_instructions, run_fastmcp_stdio};
pub use jsonrpc::handle_jsonrpc;
pub use stdio::run_stdio;
pub use supervisor::run_supervised_stdio;

pub(crate) use jsonrpc::{JsonRpcErrorData, handle_jsonrpc_value, jsonrpc_error};
pub(crate) use resources::{build_resource_payload, read_resource};
pub(crate) use tools::{call_tool, call_tool_fastmcp};
