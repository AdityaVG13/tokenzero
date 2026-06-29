#![forbid(unsafe_code)]
// `collect`, `paths`, and `render` pull these via `use crate::*`; engine_* modules
// import what they need directly after the impl split.
#![allow(unused_imports)]

mod cache_maintenance;
mod cache_pack;
mod catalog;
mod codemode;
mod collect;
mod config;
mod diff;
mod engine_edit;
mod engine_expand;
mod engine_fetch;
mod engine_find;
mod engine_ingest;
mod engine_misc;
mod engine_read;
mod engine_search;
mod engine_session;
mod engine_shell;
mod fetch_cache;
mod fetch_guard;
mod jsonrpc;
mod metrics;
mod paths;
mod recall;
mod render;
mod resources;
mod session;
mod stdio;
mod supervisor;
mod tools;
mod workspace;

pub use cache_maintenance::{cache_maintenance, session_pack, shell_spill_dir};
pub use catalog::{ResourceSpec, ToolSpec, resource_specs, tool_specs};
pub use codemode::{
    execute_codemode_with_options, CodeModeOptions, CodeModeResult, CodeModeStatus, CODEMODE_SCHEMA,
};
pub use jsonrpc::handle_jsonrpc;
pub use render::{cli_json, render_text};
pub use stdio::run_stdio;
pub use supervisor::run_supervised_stdio;

use cache_pack::{
    cache_pack_manifest_path, cache_pack_sources, previous_cache_digest, read_line_range_from_file,
};
use collect::*;
use fetch_cache::{epoch_secs, fetch_index_path, load_fetch_index, record_fetch};
use fetch_guard::{FETCH_META_MARKER, split_fetch_meta, validate_fetch_target};
use globset::{GlobBuilder, GlobMatcher};
pub(crate) use jsonrpc::{JsonRpcErrorData, handle_jsonrpc_value, jsonrpc_error};
use paths::*;
use render::*;
pub(crate) use resources::read_resource;
use serde_json::{Value, json};
use session::{DiffTelemetry, SeenState, ServeKey, ServedRecord, SessionMemory, SessionSummary};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tokenzero_core::{
    Accounting, CLI_SCHEMA_VERSION, ContentType, Mode, ShellRenderInput, ToolResponse,
    count_tokens, detect_content_type, make_capsule, make_capsule_with_raw_tokens, ref_record,
    render_shell, sha256_hex, shell_combined_output,
};
use tokenzero_filters::rewrite_command;
use tokenzero_recovery::{ExpansionResult, RecoveryStore, StoredPayload};
use tokenzero_runtime::{
    RunOutputPolicy, StreamCapture, contains_platform_shell_syntax, run_command_with_policy,
    split_command_string,
};
pub(crate) use tools::call_tool;

pub const DEFAULT_SHELL_TIMEOUT_SECS: u64 = 60;
pub const MAX_SHELL_TIMEOUT_SECS: u64 = 3600;
/// Idle exit is disabled by default: agent sessions can sit idle for hours
/// between tool calls, and an idle-exited server reads as a disconnect to MCP
/// clients. Stale-process cleanup relies on stdin EOF when the client goes
/// away; idle exit remains available as an explicit opt-in.
pub const DEFAULT_MCP_IDLE_TIMEOUT_SECS: u64 = 0;
pub const MAX_MCP_IDLE_TIMEOUT_SECS: u64 = 24 * 60 * 60;
const SEARCH_VISIT_MULTIPLIER: usize = 500;
const MIN_SEARCH_VISITED_FILES: usize = 1_000;
const MAX_SEARCH_VISITED_FILES: usize = 50_000;
pub const SEARCH_BACKEND_ENV: &str = "TOKENZERO_SEARCH_BACKEND";
pub const RG_PATH_ENV: &str = "TOKENZERO_RG_PATH";
pub const SESSION_DEDUP_ENV: &str = "TOKENZERO_MCP_DEDUP";
pub const DIFF_READS_ENV: &str = "TOKENZERO_MCP_DIFF_READS";
/// Diff-aware re-reads skip diffing when either side exceeds these bounds;
/// oversized payloads get a full serve instead (docs/routing.md §5b).
const DIFF_MAX_BYTES: usize = 2 * 1024 * 1024;
const DIFF_MAX_LINES: usize = 50_000;

pub use config::{
    EngineConfig, FETCH_ALLOW_ENV, FETCH_DENY_ENV, FETCH_ENABLED_ENV, SearchBackend,
    default_mcp_idle_timeout, default_shell_timeout, mcp_idle_timeout_from_secs,
    mcp_tool_surface_from_env, shell_timeout_from_secs,
};

/// One find/replace hunk for [`TokenZeroEngine::edit`]. `find` must match the
/// evolving file text exactly once unless `replace_all` is set. for [`TokenZeroEngine::edit`]. `find` must match the
/// evolving file text exactly once unless `replace_all` is set.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditHunk {
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub replace_all: bool,
}

/// Per-call serving options for read/find/grep. Existing positional methods
/// delegate here with defaults so their signatures stay stable.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServeOptions {
    /// Bypass the session redundancy layer for this call: always serve the
    /// full render. The serve is still recorded so later calls can dedup.
    pub fresh: bool,
}

#[derive(Debug)]
pub struct TokenZeroEngine {
    pub config: EngineConfig,
    /// Resolved rg binary, looked up once per engine instance.
    rg_binary: OnceLock<Option<PathBuf>>,
    /// Session-lifetime seen-set for the redundancy layer (docs/routing.md
    /// §5). In-memory only; dies with the server process by design.
    session: Mutex<SessionMemory>,
    /// Single-flight gate: ServeKeys currently being served, with a condvar
    /// to wake waiters. Two pipelined identical reads on the 4-worker pool
    /// would otherwise both miss the seen-set (the first has not recorded its
    /// serve yet) and both serve full — the dedup race behind the
    /// unreproducible repeat-read benchmark. A second request for a key in
    /// flight waits for the first to record, then dedups.
    in_flight: (Mutex<HashSet<ServeKey>>, Condvar),
    /// Stable id for Pulse attribution of every call this engine serves
    /// (one engine per MCP session or CLI command).
    session_id: String,
    /// Per-tool call observability; session counters plus a cross-session
    /// sidecar next to the recovery cache.
    metrics: metrics::ToolMetrics,
}

#[cfg(test)]
mod tests;
