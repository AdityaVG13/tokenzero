//! TokenZero install surface: thin wrapper over the hub install engine.
//!
//! The engine (plan/apply/rollback, doctor, agent detection, archive
//! integrity) lives in the ZeroStack hub (`zerostack-install`). This crate
//! supplies the TokenZero payload identity and re-exports the engine API so
//! existing consumers keep their import paths.

#![forbid(unsafe_code)]

pub use zerostack_install::*;

/// TokenZero payload identity (artifact names differ per repo; the engine
/// itself is product-neutral).
pub const ARTIFACT_MCP: &str = "tokenzero-mcp";
pub const ARTIFACT_RAW_WORKER: &str = "tokenzero-codemode";
pub const ARTIFACT_SHIM: &str = "tokenzero";

/// Legacy alias retained for tests importing `packaging::*` directly.
pub mod packaging {
    pub use zerostack_install::packaging::*;
    pub const ARTIFACT_MCP: &str = super::ARTIFACT_MCP;
    pub const ARTIFACT_RAW_WORKER: &str = super::ARTIFACT_RAW_WORKER;
    pub const ARTIFACT_SHIM: &str = super::ARTIFACT_SHIM;
}

pub mod package_audit {
    pub use zerostack_install::package_audit::*;
}
