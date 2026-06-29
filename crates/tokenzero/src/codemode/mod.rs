//! TokenZero CodeMode surface — a CodeMode-style code-plan executor that
//! exposes TokenZero operations as typed methods. Models write JS-like
//! plans; the executor parses, dispatches through TokenZeroEngine, and returns
//! only the final shaped result. Additive to MCP, never replaces it.

mod catalog;
mod exec;
mod parser;
mod result;

#[cfg(test)]
mod tests;

pub use result::{CODEMODE_SCHEMA, CodeModeResult, CodeModeStatus, CodeModeTelemetry};
pub(crate) use result::CodeModeOptions;
pub(crate) use exec::execute_codemode_with_options;
