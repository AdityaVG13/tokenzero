//! TokenZero CodeMode surface — a CodeMode-style code-plan executor that
//! exposes TokenZero operations as typed methods. Models write JS-like
//! plans; the executor parses, dispatches through TokenZeroEngine, and returns
//! only the final shaped result. Classic MCP and CodeMode are mutually exclusive install surfaces.

pub(crate) mod catalog;
mod exec;
mod parser;
mod result;
mod sandbox;
mod store;

#[allow(dead_code)]
pub mod audit;
#[cfg(test)]
mod audit_tests;
#[allow(dead_code)]
pub mod bench;
#[cfg(test)]
mod bench_tests;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod tests;

pub use exec::execute_codemode_with_options;
pub use result::{CODEMODE_SCHEMA, CodeModeOptions, CodeModeResult, CodeModeStatus};
pub use store::{CODEMODE_LIMITS_SCHEMA, CodeModeLimits};
