//! TokenZero CodeMode surface adapter: the runtime lives in the standalone
//! tokenzero-codemode crate over the canonical dispatcher (tokenzero-4uql.8.1).
//! This module re-exports it and keeps the MCP-side FastMCP parity bench.

pub use tokenzero_codemode::*;

// Bench harness is test-only and compares against the FastMCP renderer, so it
// stays on the MCP surface side (tokenzero-wpay / e99a0d8).
#[cfg(all(test, feature = "surface-codemode"))]
pub mod bench;
