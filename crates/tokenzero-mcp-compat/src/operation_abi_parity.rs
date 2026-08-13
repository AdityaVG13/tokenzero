//! Live-boundary parity for tokenzero-irx9.1.
//!
//! These tests intentionally compare the ABI registry against **independent**
//! live surfaces that are not re-derived from the same `all_operations()` view:
//!
//! - `ToolKind` / `tool_table!` product names + `dispatch_tool` exhaustiveness
//! - `TOOL_ALIASES` table
//! - `resource_specs()` URI list
//! - CodeMode `METHOD_CATALOG` paths
//! - Wire `tools/list` / FastMCP definitions (input **and** output schemas)
//! - CodeMode `describe_method` / method catalog I/O schemas
//!
//! Kill tests mutate independent fixtures (name sets, cloned wire schemas)
//! so missing/extra tools and structural I/O drift fail closed.

#[cfg(test)]
#[path = "../../../tests/mcp-compat/inline/operation_abi_parity__tests.rs"]
mod tests;
