//! `TokenZeroEngine` methods extracted from `lib.rs`.
#![allow(unused_imports)]

use super::cache_pack::{
    cache_pack_manifest_path, cache_pack_sources, previous_cache_digest, read_line_range_from_file,
};
use super::collect::*;
use super::config::{
    EngineConfig, FETCH_ALLOW_ENV, FETCH_DENY_ENV, FETCH_ENABLED_ENV, ServeFlight, new_session_id,
};
use super::fetch_cache::{epoch_secs, fetch_index_path, load_fetch_index, record_fetch};
use super::fetch_guard::{FETCH_META_MARKER, split_fetch_meta, validate_fetch_target};
use super::metrics;
use super::paths::*;
use super::render::*;
use super::session::{
    DiffTelemetry, SeenState, ServeKey, ServedRecord, SessionMemory, SessionSummary,
};
use super::{
    Accounting, ContentType, DEFAULT_MCP_IDLE_TIMEOUT_SECS, DEFAULT_SHELL_TIMEOUT_SECS,
    DIFF_MAX_BYTES, DIFF_MAX_LINES, DIFF_READS_ENV, EditHunk, MAX_MCP_IDLE_TIMEOUT_SECS,
    MAX_SEARCH_VISITED_FILES, MAX_SHELL_TIMEOUT_SECS, MIN_SEARCH_VISITED_FILES, Mode, RG_PATH_ENV,
    SEARCH_BACKEND_ENV, SEARCH_VISIT_MULTIPLIER, SESSION_DEDUP_ENV, SearchBackend, ServeOptions,
    ShellRenderInput, TokenZeroEngine, ToolResponse, cache_maintenance, count_tokens,
    detect_content_type, make_capsule, make_capsule_with_raw_tokens, ref_record, render_shell,
    sha256_hex, shell_combined_output, shell_spill_dir, shell_timeout_from_secs,
    split_command_string,
};
use crate::recall;
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tokenzero_filters::rewrite_command;
use tokenzero_recovery::{ExpansionResult, RecoveryStore, StoredPayload};
use tokenzero_runtime::{
    RunOutputPolicy, StreamCapture, contains_platform_shell_syntax, run_command_with_policy,
};

impl TokenZeroEngine {
    pub fn find(
        &self,
        query: &str,
        roots: &[PathBuf],
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        self.find_with_options(
            query,
            roots,
            mode,
            max_files,
            max_visible_tokens,
            ServeOptions::default(),
        )
    }

    pub fn find_with_options(
        &self,
        query: &str,
        roots: &[PathBuf],
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        self.search(
            "find",
            query,
            roots,
            mode,
            max_files,
            max_visible_tokens,
            options,
        )
    }

    pub fn grep(
        &self,
        query: &str,
        roots: &[PathBuf],
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        self.grep_with_options(
            query,
            roots,
            mode,
            max_files,
            max_visible_tokens,
            ServeOptions::default(),
        )
    }

    pub fn grep_with_options(
        &self,
        query: &str,
        roots: &[PathBuf],
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        self.search(
            "grep",
            query,
            roots,
            mode,
            max_files,
            max_visible_tokens,
            options,
        )
    }
}
