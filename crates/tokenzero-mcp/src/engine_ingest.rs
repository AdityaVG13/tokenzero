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
    pub fn ingest(&self, text: &str, kind: ContentType, mode: Mode, source: &str) -> ToolResponse {
        let mut store = self.recovery_store();
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.store_payload(text, kind, None, None, None) {
            Ok(stored) => {
                refs.push(ref_record("blob", stored.blob_ref, text.len()));
                refs.push(ref_record("file", stored.file_ref, text.len()));
            }
            Err(err) => {
                storage_error = Some(err.to_string());
            }
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        let capsule = if refs_complete {
            make_capsule(text, mode, self.config.max_visible_tokens, Some(source))
        } else {
            let raw_tokens = count_tokens(text);
            tokenzero_core::Capsule {
                text: text.trim_end().to_string(),
                raw_tokens,
                visible_tokens: raw_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = ToolResponse::ok(
            "ingest",
            mode,
            capsule.text,
            refs,
            Accounting {
                raw_tokens: capsule.raw_tokens,
                visible_tokens: capsule.visible_tokens,
                recovery_tokens: store.recovery_tokens,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(kind.to_string());
        if let Some(error) = storage_error {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: "could not persist recovery cache for ingested content".to_string(),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
            response.telemetry = Some(json!({
                "transport_status": "degraded",
                "degraded": true,
                "storage_error": error,
                "exact_refs_available": false
            }));
        }
        if text.is_empty() {
            apply_zero_hit_note(&mut response, mode, "# ingest — 0 bytes".to_string());
        }
        response
    }
}
