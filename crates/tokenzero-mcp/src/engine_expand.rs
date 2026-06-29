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
    pub fn expand(
        &self,
        ref_id: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> ToolResponse {
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let result = store.expand(
            ref_id,
            selector.or(Some("raw")),
            start_line,
            end_line,
            anchor_kind,
            symbol,
        );
        expansion_response(result, store.recovery_tokens)
    }

    /// Lossless full-text search over the persisted recovery cache: every
    /// payload TokenZero has stored this workspace is searchable, every hit
    /// line carries its exact `tz://` ref, and `tz_expand` recovers the full
    /// bytes. Read-only — never stores or mutates anything.
    pub fn recall(
        &self,
        query: &str,
        max_hits: usize,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if query.trim().is_empty() {
            return ToolResponse::error(
                "recall",
                "invalid_query",
                "recall requires a non-empty query".to_string(),
                None,
            );
        }
        let outcome = recall::recall_search(&self.config.cache_path, query, max_hits.max(1));
        let mut refs = Vec::new();
        let mut listed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut lines = Vec::with_capacity(outcome.hits.len() + 1);
        if !outcome.hits.is_empty() {
            lines.push(format!(
                "# recall {} — {} hits across {} stored payloads{}",
                zero_hit_label(query),
                outcome.hits.len(),
                outcome.payloads_searched,
                if outcome.truncated {
                    " (hit limit reached)"
                } else {
                    ""
                }
            ));
        }
        for hit in &outcome.hits {
            lines.push(format!(
                "{} {}:{}: {}",
                hit.ref_id, hit.label, hit.line, hit.text
            ));
            if listed.insert(hit.ref_id.as_str()) {
                refs.push(ref_record("recall", hit.ref_id.clone(), 0));
            }
        }
        let assembled = lines.join("\n");
        let raw_tokens = count_tokens(&assembled);
        let capsule = make_capsule_with_raw_tokens(
            &assembled,
            raw_tokens,
            mode,
            max_visible_tokens,
            Some(&format!("recall {}", zero_hit_label(query))),
        );
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = ToolResponse::ok(
            "recall",
            mode,
            capsule.text,
            refs,
            Accounting {
                raw_tokens: capsule.raw_tokens,
                visible_tokens: capsule.visible_tokens,
                recovery_tokens: 0,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(ContentType::SearchResult.to_string());
        response.telemetry = Some(json!({
            "query": query,
            "hits": outcome.hits.len(),
            "payloads_searched": outcome.payloads_searched,
            "truncated_by_results": outcome.truncated,
            "transport_status": if outcome.unreadable { "degraded" } else { "ok" },
            "degraded": outcome.unreadable
        }));
        if outcome.unreadable {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "recall_cache_unreadable".to_string(),
                message: "recovery cache exists but could not be read or parsed".to_string(),
                repair: Some(
                    "run tokenzero mem to inspect the cache, or pass --cache-path".to_string(),
                ),
            });
        }
        if outcome.hits.is_empty() {
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# recall {} — 0 matches", zero_hit_label(query)),
            );
        }
        response
    }

    /// Store `stored_text` as the canonical recoverable payload while
    /// rendering `rendered_text` (a lossless compact projection of it) as the
    /// visible capsule. Accounting keeps raw tokens from the stored payload.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ingest_with_tool(
        &self,
        tool: &str,
        stored_text: &str,
        rendered_text: &str,
        kind: ContentType,
        mode: Mode,
        source: &str,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        let mut response = self.ingest(stored_text, kind, mode, source);
        response.tool = tool.to_string();
        if let Some(accounting) = response.accounting.as_mut() {
            let capsule = make_capsule_with_raw_tokens(
                rendered_text,
                accounting.raw_tokens,
                mode,
                max_visible_tokens,
                Some(source),
            );
            accounting.visible_tokens = capsule.visible_tokens;
            if let Some(visible) = response.visible.as_mut() {
                visible.text = capsule.text;
            }
        }
        response
    }
}
