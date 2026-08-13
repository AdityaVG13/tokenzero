#![forbid(unsafe_code)]
//! TokenZero classic MCP compatibility adapter over the hub-owned FastMCP transport.
//!
//! Domain execution lives in [`tokenzero_engine`]. This crate depends inward and
//! must not re-implement domain auth/root/mutation/ref/telemetry semantics.

mod capability_descriptor;
mod catalog;
mod fastmcp_mode;
mod job_progress;
mod jsonrpc;
mod operation_abi_parity;
mod resources;
mod tools;

#[cfg(test)]
#[path = "../../../tests/mcp-compat/unit/mod.rs"]
mod tests;

// Re-export the domain engine (types, dispatcher, modules) for CLI + adapters.
pub use tokenzero_engine::*;
pub use tokenzero_engine::{
    binary_resolve, cache_maintenance, config, expand_params, ledger, metrics, paths, render,
    session, session_persist, surface_health, usage_telemetry, wall, workspace, write_ladder,
};

pub use catalog::{ResourceSpec, ToolSpec, resource_specs, tool_specs};
pub use tokenzero_engine::codemode_catalog::{
    describe_method as describe_codemode_method, search_catalog as search_codemode_catalog,
};
pub use tokenzero_engine::codemode_wire::{
    CODEMODE_SCHEMA, CodeModeLimits, CodeModeOptions, CodeModeResult, CodeModeStatus,
};

pub use fastmcp_mode::{fastmcp_codemode_instructions, fastmcp_instructions, run_fastmcp_stdio};
pub use jsonrpc::handle_jsonrpc;

pub(crate) use resources::read_resource;
pub(crate) use tools::{call_tool, call_tool_fastmcp};
