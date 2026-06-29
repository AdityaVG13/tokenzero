//! TokenZero CodeMode surface — a CodeMode-style code-plan executor that
//! exposes TokenZero operations as typed methods. Models write JS-like
//! plans; the executor parses, dispatches through TokenZeroEngine, and returns
//! only the final shaped result. Classic MCP and CodeMode are mutually exclusive install surfaces.

pub(crate) mod catalog;
mod exec;
mod parser;
mod result;

#[cfg(test)]
mod tests;

pub use exec::execute_codemode_with_options;
pub use result::{CodeModeOptions, CodeModeResult, CodeModeStatus, CODEMODE_SCHEMA};
