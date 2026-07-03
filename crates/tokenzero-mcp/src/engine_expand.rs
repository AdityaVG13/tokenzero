//! `TokenZeroEngine` expand + recall (extracted from `lib.rs`).
#![allow(unused_imports)]

use super::cache_pack::{
    cache_pack_manifest_path, cache_pack_sources, previous_cache_digest, read_line_range_from_file,
};
use super::collect::*;
use super::config::{
    EngineConfig, FETCH_ALLOW_ENV, FETCH_DENY_ENV, FETCH_ENABLED_ENV, ServeFlight, new_session_id,
};
use super::expand_params::ExpandParams;
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
use crate::diff;
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

fn norm_opt(value: &Option<String>) -> String {
    value.as_deref().unwrap_or("").to_string()
}

fn expand_serve_key(params: &ExpandParams) -> ServeKey {
    ServeKey::Expand {
        ref_id: params.ref_id.clone(),
        start_line: params.start_line,
        end_line: params.end_line,
        selector_norm: norm_opt(&params.selector),
        symbol_norm: norm_opt(&params.symbol),
        anchor_kind_norm: norm_opt(&params.anchor_kind),
    }
}

fn resolve_slice(
    store: &mut RecoveryStore,
    params: &ExpandParams,
) -> Result<ExpansionResult, Box<ToolResponse>> {
    let selector = params.selector.as_deref().or(Some("raw"));
    let anchor = params.anchor_kind.as_deref();
    let symbol = params.symbol.as_deref();
    let result = store.expand(
        &params.ref_id,
        selector,
        params.start_line,
        params.end_line,
        anchor,
        symbol,
    );
    if result.found {
        Ok(result)
    } else {
        Err(Box::new(expansion_response(result, store.recovery_tokens)))
    }
}

impl TokenZeroEngine {
    pub fn expand_with_params(&self, params: ExpandParams) -> ToolResponse {
        if !params.ref_id.starts_with("tz://") {
            return ToolResponse::error(
                "expand",
                "invalid_ref",
                format!("ref must start with tz://, got: {}", params.ref_id),
                None,
            );
        }

        let key = expand_serve_key(&params);
        let _flight = if self.config.session_dedup {
            self.begin_serve_flight(vec![key.clone()])
        } else {
            self.begin_serve_flight(Vec::new())
        };

        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let mut summary = SessionSummary::default();
        let mut pending: Vec<(ServeKey, ServedRecord)> = Vec::new();

        if let Some(since_ref) = params.since.as_deref().filter(|_| !params.fresh) {
            if !since_ref.starts_with("tz://") {
                return ToolResponse::error(
                    "expand",
                    "invalid_ref",
                    format!("since must start with tz://, got: {since_ref}"),
                    None,
                );
            }
            let since_result = store.expand(
                since_ref,
                params.selector.as_deref().or(Some("raw")),
                params.start_line,
                params.end_line,
                params.anchor_kind.as_deref(),
                params.symbol.as_deref(),
            );
            if !since_result.found {
                return ToolResponse::error(
                    "expand",
                    match since_result.reason.as_str() {
                        "stale-ref" => "ref_stale",
                        "dangling-ref" => "ref_not_found",
                        "invalid-ref" => "invalid_ref",
                        _ => "expand_failed",
                    },
                    format!("since ref is not recoverable: {since_ref}"),
                    None,
                );
            }
            let target = match resolve_slice(&mut store, &params) {
                Ok(t) => t,
                Err(resp) => return *resp,
            };
            if since_result.content == target.content {
                let text = unchanged_since_expand_ack(since_ref);
                let tokens = count_tokens(&text);
                let mut response = ToolResponse::ok(
                    "expand",
                    Mode::Exact,
                    text,
                    Vec::new(),
                    Accounting {
                        raw_tokens: tokens,
                        visible_tokens: tokens,
                        recovery_tokens: store.recovery_tokens,
                        exact_ref_tokens: Some(count_tokens(&params.ref_id)),
                    },
                );
                if self.config.session_dedup {
                    pending.push(self.pending_expand_record(&params, &target.content, &mut store));
                }
                if let Some(telemetry) = summary.telemetry() {
                    response.telemetry = Some(telemetry);
                }
                self.session_apply(pending, &summary);
                return response;
            }
            let render = match diff::unified_diff(&since_result.content, &target.content) {
                Some(r) => r,
                None => {
                    let text = unchanged_since_expand_ack(since_ref);
                    let tokens = count_tokens(&text);
                    if self.config.session_dedup {
                        pending.push(self.pending_expand_record(
                            &params,
                            &target.content,
                            &mut store,
                        ));
                    }
                    let response = ToolResponse::ok(
                        "expand",
                        Mode::Exact,
                        text,
                        Vec::new(),
                        Accounting {
                            raw_tokens: tokens,
                            visible_tokens: tokens,
                            recovery_tokens: store.recovery_tokens,
                            exact_ref_tokens: Some(count_tokens(&params.ref_id)),
                        },
                    );
                    self.session_apply(pending, &summary);
                    return response;
                }
            };
            let assembled = expand_since_diff_text(since_ref, &params.ref_id, &render.text);
            let tokens = count_tokens(&assembled);
            summary.note_diff(
                DiffTelemetry {
                    hunks: render.hunks,
                    plus: render.plus,
                    minus: render.minus,
                    base_ref: since_ref.to_string(),
                },
                0,
            );
            let mut response = ToolResponse::ok(
                "expand",
                Mode::Exact,
                assembled,
                Vec::new(),
                Accounting {
                    raw_tokens: tokens,
                    visible_tokens: tokens,
                    recovery_tokens: store.recovery_tokens,
                    exact_ref_tokens: Some(count_tokens(&params.ref_id)),
                },
            );
            if self.config.session_dedup {
                pending.push(self.pending_expand_record(&params, &target.content, &mut store));
            }
            if let Some(telemetry) = summary.telemetry() {
                response.telemetry = Some(telemetry);
            }
            self.session_apply(pending, &summary);
            return response;
        }

        let target = match resolve_slice(&mut store, &params) {
            Ok(t) => t,
            Err(resp) => return *resp,
        };

        // Explicit expand is the recovery contract: it ALWAYS returns exact
        // bytes. Replacing content with an "identical to … (unchanged)" ack
        // here broke byte-exact recovery (release-claim audits) and forced a
        // fresh re-call exactly when the model had decided it needed the
        // bytes — the capability-loss the compression doctrine forbids.
        // Seen-set economics stay on the implicit serve paths (read/find
        // spills) and on explicit `since=` diffs; serves are still RECORDED
        // below so those paths keep learning from expands.

        let mut response = expansion_response(target.clone(), store.recovery_tokens);
        if self.config.session_dedup {
            pending.push(self.pending_expand_record(&params, &target.content, &mut store));
        }
        if let Some(telemetry) = summary.telemetry() {
            response.telemetry = Some(telemetry);
        }
        self.session_apply(pending, &summary);
        response
    }

    fn pending_expand_record(
        &self,
        params: &ExpandParams,
        content: &str,
        store: &mut RecoveryStore,
    ) -> (ServeKey, ServedRecord) {
        let key = expand_serve_key(params);
        let content_sha256 = sha256_hex(content);
        let stored = store.store_payload_deferred_batch(
            content,
            ContentType::Unknown,
            None,
            params.start_line,
            params.end_line,
        );
        let _ = store.persist_pending();
        let record = ServedRecord {
            content_sha256,
            blob_ref: stored.blob_ref.clone(),
            file_ref: stored.file_ref.clone(),
            raw_tokens: stored.raw_tokens,
            line_count: content.lines().count(),
            byte_len: content.len(),
            served_at: SystemTime::now(),
            serve_count: 1,
        };
        (key, record)
    }

    pub fn expand(
        &self,
        ref_id: &str,
        selector: Option<&str>,
        start_line: Option<usize>,
        end_line: Option<usize>,
        anchor_kind: Option<&str>,
        symbol: Option<&str>,
    ) -> ToolResponse {
        self.expand_with_params(ExpandParams {
            ref_id: ref_id.to_string(),
            selector: selector.map(str::to_string),
            start_line,
            end_line,
            anchor_kind: anchor_kind.map(str::to_string),
            symbol: symbol.map(str::to_string),
            since: None,
            fresh: false,
        })
    }

    /// Lossless full-text search over the persisted recovery cache.
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
