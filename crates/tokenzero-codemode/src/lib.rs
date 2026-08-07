#![forbid(unsafe_code)]
#![cfg(feature = "surface-codemode")]

//! TokenZero CodeMode surface: a CodeMode-style code-plan executor that
//! exposes TokenZero operations as typed methods. Models write JS-like
//! plans; the executor parses, dispatches through TokenZeroEngine, and returns
//! only the final shaped result. Classic MCP and CodeMode are mutually exclusive install surfaces.

pub mod catalog;
pub(crate) mod containment;
#[cfg(feature = "js")]
mod exec;
#[cfg(not(feature = "js"))]
mod exec_stub;
#[cfg(not(feature = "js"))]
use exec_stub as exec;
pub mod journal;
mod parser;
pub(crate) mod recipe_registry;
mod result;
mod sandbox;
pub mod sentinel;
mod store;

pub use catalog::{
    describe_method as describe_codemode_method, search_catalog as search_codemode_catalog,
};
pub use exec::execute_codemode_with_options;
pub use result::{CODEMODE_SCHEMA, CodeModeOptions, CodeModeResult, CodeModeStatus};
pub use store::{CODEMODE_LIMITS_SCHEMA, CodeModeLimits};

#[cfg(all(test, feature = "js"))]
mod e2e_tests;

/// Wire CodeMode containment into the domain shell hooks (idempotent).
pub fn install_shell_hooks() {
    tokenzero_engine::shell_hooks::install(tokenzero_engine::shell_hooks::ShellHooks {
        note_child: containment::note_child,
        reserve_background_job: containment::reserve_background_job,
        note_background_child: |id, pid, pgid| containment::note_background_child(id, pid, pgid),
        finish_background_job: containment::finish_background_job,
        containment_snapshot: containment::snapshot,
    });
}

/// Install the real JS-backed executor into the canonical engine hook so the
/// MCP compatibility adapter (tokenzero-mcp-compat) serves zero.execute over
/// one dispatcher. Safe to call once at process start; later calls are no-ops.
pub fn install_mcp_bridge() {
    let _ = tokenzero_engine::codemode_wire::register_codemode_execute_hook(|plan, options| {
        execute_codemode_with_options(plan, options.clone())
    });
}
