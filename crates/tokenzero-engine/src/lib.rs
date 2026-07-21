#![forbid(unsafe_code)]
// Engine modules use `use super::*` / crate-level imports after the split.
#![allow(unused_imports)]

//! Transport-neutral TokenZero domain engine and typed dispatcher.
//!
//! # Dependency direction (tokenzero-irx9.2)
//!
//! This crate must not depend on FastMCP, MCP JSON-RPC framing, or the CodeMode
//! sandbox. Transport adapters (`tokenzero-mcp`, CLI) depend inward and call
//! [`dispatch_operation`] exactly once per domain op.

pub mod binary_resolve;
pub mod cache_maintenance;
mod cache_pack;
mod collect;
pub mod config;
mod diff;
mod dispatcher;
mod domain;
pub mod raw_worker;
pub mod surface_handshake;
mod engine_common;
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
pub mod expand_params;
mod fetch_cache;
mod fetch_guard;
pub mod ledger;
pub mod metrics;
pub mod paths;
mod recall;
pub mod render;
mod report_tool;
pub mod session;
pub mod session_persist;
pub mod shell_hooks;
pub mod surface_health;
pub mod usage_telemetry;
pub mod wall;
pub mod workspace;
pub mod write_ladder;

pub use binary_resolve::{
    BinaryResolution, ResolveError, ResolvedBinary, TOKENZERO_BIN_ENV, TOKENZERO_CURL_PATH_ENV,
    TOKENZERO_RG_PATH_ENV, engine_binaries_json, resolve_all_engine_binaries, resolve_curl_binary,
    resolve_rg_binary, resolve_tokenzero_binary,
};
pub use cache_maintenance::{
    cache_maintenance, cache_maintenance_coalesced, session_pack, shell_spill_dir,
};
pub use dispatcher::{
    DispatchOutcome, DispatchProfile, DispatchSurface, all_domain_operations, dispatch_cli,
    dispatch_codemode_method, dispatch_count, dispatch_mcp_tool, dispatch_operation,
    dispatch_raw_worker, domain_fastmcp_ops, is_domain_operation, last_dispatch_profile,
    operation_is_domain, tool_response_to_domain,
};
pub use domain::{DomainDispatchError, execute_domain_op};
pub use raw_worker::{
    RawWorkerError, RawWorkerRequest, RawWorkerResponse, RawWorkerServeOptions,
    execute_raw_worker_frame, execute_raw_worker_json, maybe_run_raw_worker_from_args,
    parse_raw_worker_argv, raw_worker_print_handshake, response_from_outcome,
    run_raw_worker_once, run_raw_worker_serve,
};
pub use surface_handshake::{
    CompressionOwner, HandshakeSurface, PlannerOwner, RAW_WORKER_PROTOCOL_VERSION,
    SURFACE_CAPABILITY_SCHEMA, SurfaceCapability, SurfaceLimits, build_surface_capability,
    check_contract_compatibility, composition_trace, surface_capability_json,
};
pub use render::{cli_json, exact_ref_token_count, prune_dead_refs, render_text};
pub use fetch_cache::{load_fetch_index, record_fetch};
pub use collect::{find_rg_in_path, parse_rg_line};

pub use report_tool::{build_tool_issue_report, is_reportable_tool_name, record_tool_issue};
pub use shell_hooks::{ShellHooks, install as install_shell_hooks};
pub use workspace::{
    SHARED_STORE_OPT_IN_ENVS, STORE_ROOT_ENVS, StoreResolutionReport, allowed_roots_for_workspace,
    default_allowed_roots, default_recovery_cache_path, resolve_recovery_cache_path,
    resolve_recovery_cache_path_with_env, resolve_store_root_with_env,
    shared_store_opt_in_from_env, store_is_under_project_root, store_resolution_json,
    store_resolution_report, store_resolution_report_with_env, tokenzero_work_root,
};
pub use write_ladder::{
    WRITE_ESCAPE_ENV, WRITE_RECOVERY_LADDER, annotate_write_failure, write_escape_ack_active,
};

use cache_pack::{
    cache_pack_manifest_path, cache_pack_sources, previous_cache_digest, read_line_range_from_file,
};
use collect::*;
use engine_common::*;
use fetch_cache::{epoch_secs, fetch_index_path};
use fetch_guard::{FETCH_META_MARKER, split_fetch_meta, validate_fetch_target};
use globset::{GlobBuilder, GlobMatcher};
use paths::*;
use render::*;
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
    render_shell, sha256_hex, shell_combined_output, shell_raw_tokens,
};
use tokenzero_filters::rewrite_command;
use tokenzero_recovery::{ExpansionResult, RecoveryStore, StoredPayload};
use tokenzero_runtime::{
    RunOutputPolicy, StreamCapture, contains_platform_shell_syntax, run_command_with_policy,
    split_command_string,
};

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
    DEFAULT_SHELL_INLINE_BUDGET, EngineConfig, FETCH_ALLOW_ENV, FETCH_DENY_ENV, FETCH_ENABLED_ENV,
    SHELL_INLINE_BUDGET_ENV, SearchBackend, TELEMETRY_ENV, default_mcp_idle_timeout,
    default_shell_timeout, mcp_idle_timeout_from_secs, mcp_tool_surface_from_env,
    resolve_telemetry, shell_inline_budget_from_env, shell_timeout_from_secs,
    telemetry_env_enabled,
};
pub use usage_telemetry::{
    ExecutionPath, TelemetryInspection, UsageRecord, inspect_usage_telemetry,
    record_codemode_accounting, record_mcp_accounting, usage_telemetry_enabled,
    usage_telemetry_path_for_cache,
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
    /// §5). Loaded from `session-memory.json` when dedup is enabled.
    // None until a tool actually needs the persisted working set. Session boot must not
    // deserialize session-memory.json on the compatible manifest+delta path.
    /// Visible to integration tests in tokenzero-mcp (same-process harness).
    pub session: Mutex<Option<SessionMemory>>,
    /// Prompt-resident spans; bodies page to durable refs under budget pressure.
    /// Visible to integration tests in tokenzero-mcp.
    pub working_set: Mutex<tokenzero_recovery::working_set::WorkingSet>,
    /// Reused RecoveryStore for working-set admission (one per engine lifetime).
    recovery_store: Mutex<tokenzero_recovery::RecoveryStore>,
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
    /// Disk-backed seen-set; `None` when session dedup is off.
    session_persist: Option<session_persist::SessionPersistence>,
    /// Expand/read surface health + crash-only recovery unlock (wqw.9).
    /// Shared with CodeMode plan engines so expand outcomes update the same gate.
    /// Lazily opened on first `session_boot_snapshot` so cheap CLI tools do not
    /// pay boot I/O when they never ask for the capsule.
    session_boot: OnceLock<Option<tokenzero_recovery::boot::SessionBoot>>,
    surface_health: std::sync::Arc<surface_health::SurfaceHealth>,
    /// Fail-open append-only response accounting beside the recovery cache.
    ledger: ledger::LedgerWriter,
    /// Per-connection MCP initialize lifecycle (stdio session / engine).
    pub lifecycle: Mutex<InitializeState>,
}

/// MCP initialize lifecycle for one engine/connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitializeState {
    #[default]
    Uninitialized,
    /// `initialize` succeeded; waiting for `notifications/initialized`.
    Negotiated,
    /// Client completed initialize + initialized; tools/list and peers allowed.
    Ready,
}
