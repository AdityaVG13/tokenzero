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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search(
        &self,
        tool: &str,
        query: &str,
        roots: &[PathBuf],
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        for root in roots {
            if !self.path_allowed(root) {
                return ToolResponse::error(
                    tool,
                    "path_not_allowed",
                    format!("path is outside allowed roots: {}", root.display()),
                    None,
                );
            }
        }
        // Single-flight identical searches so a second pipelined call dedups
        // against the first's recorded serve instead of racing it. Same key
        // the session block uses below; held until after session_apply.
        let _flight = if self.config.session_dedup {
            let mut canonical_roots: Vec<PathBuf> =
                roots.iter().map(|root| comparable_path(root)).collect();
            canonical_roots.sort();
            self.begin_serve_flight(vec![ServeKey::Output {
                tool: format!("{tool}:{:?}", self.config.search_backend),
                query: query.to_string(),
                roots: canonical_roots,
            }])
        } else {
            self.begin_serve_flight(Vec::new())
        };
        let max_visited_files = max_search_visited_files(max_files);
        let run_internal = |stats: &mut SearchStats, matches: &mut Vec<SearchMatch>| {
            for root in roots {
                collect_search(
                    root,
                    root,
                    query,
                    max_files,
                    max_visited_files,
                    MAX_WALK_DEPTH,
                    stats,
                    matches,
                );
                if stats.truncated_by_results || stats.truncated_by_visit {
                    break;
                }
            }
        };
        let mut matches: Vec<SearchMatch> = Vec::new();
        let mut stats = SearchStats::default();
        let mut backend = "internal";
        let mut fallback_reason: Option<String> = None;
        // With the EXPLICIT rg backend, grep's pattern is a regex by
        // contract; silently degrading to the internal substring scanner
        // would change result semantics, so unavailability is an error for
        // grep (find keeps identical substring semantics either way and may
        // fall back). Auto mode always falls back.
        let explicit_rg = matches!(self.config.search_backend, SearchBackend::Rg);
        let backend_unavailable = |reason: &str| {
            ToolResponse::error(
                tool,
                "backend_unavailable",
                format!("TOKENZERO_SEARCH_BACKEND=rg but ripgrep is unusable: {reason}"),
                Some(
                    "install ripgrep, set TOKENZERO_RG_PATH, or use auto/internal \
                     (internal matches literal substrings, not regex)"
                        .to_string(),
                ),
            )
        };
        let rg = match self.config.search_backend {
            SearchBackend::Internal => None,
            SearchBackend::Rg | SearchBackend::Auto => {
                let resolved = self.rg_binary();
                if resolved.is_none() {
                    if explicit_rg && tool == "grep" {
                        return backend_unavailable("rg_not_found");
                    }
                    fallback_reason = Some("rg_not_found".to_string());
                }
                resolved
            }
        };
        match rg {
            Some(rg_path) => match rg_search(rg_path, tool, query, roots, max_files) {
                Ok((rg_matches, rg_stats)) => {
                    matches = rg_matches;
                    stats = rg_stats;
                    backend = "rg";
                }
                // Only the rg backend treats grep patterns as regex; surface
                // its parse error instead of silently degrading to substring
                // semantics that would return different results.
                Err(RgFailure::InvalidPattern(message)) => {
                    return ToolResponse::error(
                        tool,
                        "invalid_pattern",
                        message,
                        Some(
                            "fix the regex, or use tz_find for literal substring search"
                                .to_string(),
                        ),
                    );
                }
                Err(RgFailure::Unavailable(reason)) => {
                    if explicit_rg && tool == "grep" {
                        return backend_unavailable(&reason);
                    }
                    fallback_reason = Some(reason);
                    run_internal(&mut stats, &mut matches);
                }
            },
            None => run_internal(&mut stats, &mut matches),
        }
        // Canonical recoverable payload keeps the grep-compatible flat format;
        // the grouped rendering is visible-only and used when strictly cheaper.
        let output = flat_search_output(&matches);
        let compact = grouped_search_output(&matches);
        let (visible_source, grouped) = pick_cheaper(&output, &compact);
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let search_refs = store.store_search_output_deferred(&output, Some(query));
        let stored =
            store.store_payload_deferred(&output, ContentType::SearchResult, None, None, None);
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.persist_pending() {
            Ok(()) => {
                refs.push(ref_record("blob", stored.blob_ref.clone(), output.len()));
                refs.push(ref_record("file", stored.file_ref.clone(), output.len()));
                refs.extend(search_refs.into_iter().map(|r| ref_record("search", r, 0)));
            }
            Err(err) => storage_error = Some(err.to_string()),
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let exact_refs_available = !refs.is_empty();
        let capsule = if refs_complete {
            make_capsule_with_raw_tokens(
                visible_source,
                stored.raw_tokens,
                mode,
                max_visible_tokens,
                Some(&format!("{tool} {query}")),
            )
        } else {
            tokenzero_core::Capsule {
                text: output.trim_end().to_string(),
                raw_tokens: stored.raw_tokens,
                visible_tokens: stored.raw_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let full_bytes = capsule.text.len();
        let mut visible_text = capsule.text;
        let mut final_visible_tokens = capsule.visible_tokens;
        let mut summary = SessionSummary::default();
        let mut pending: Vec<(ServeKey, ServedRecord)> = Vec::new();
        // Session redundancy layer (docs/routing.md §5a): identical flat
        // output already served this session collapses to a note. Zero-hit
        // notes below stay untouched (empty output skips the layer; notes
        // are never deduped), and changed output gets a full serve — search
        // results are never diffed. Skipped entirely when this call's refs
        // failed to persist: a note must never advertise unrecoverable refs,
        // and a serve whose refs died must not become a dedup base.
        if self.config.session_dedup
            && !output.is_empty()
            && storage_error.is_none()
            && refs_complete
        {
            let mut canonical_roots: Vec<PathBuf> =
                roots.iter().map(|root| comparable_path(root)).collect();
            canonical_roots.sort();
            let key = ServeKey::Output {
                tool: format!("{tool}:{:?}", self.config.search_backend),
                query: query.to_string(),
                roots: canonical_roots,
            };
            let content_sha256 = sha256_hex(&output);
            let bypass = matches!(mode, Mode::Passthrough) || options.fresh;
            if let SeenState::Unchanged {
                serve_count,
                cross_session,
            } = self.session_lookup(&key, &content_sha256)
            {
                if !bypass {
                    let note = unchanged_search_note(tool, query, &output, &stored);
                    let note_tokens = count_tokens(&note);
                    // ROI guard: emit only when strictly cheaper than the
                    // full render.
                    if note_tokens < final_visible_tokens {
                        summary.note_dedup(
                            serve_count + 1,
                            final_visible_tokens - note_tokens,
                            cross_session,
                        );
                        visible_text = note;
                        final_visible_tokens = note_tokens;
                    }
                }
            }
            pending.push((
                key,
                ServedRecord {
                    content_sha256,
                    blob_ref: stored.blob_ref.clone(),
                    file_ref: stored.file_ref.clone(),
                    raw_tokens: stored.raw_tokens,
                    line_count: output.lines().count(),
                    byte_len: output.len(),
                    served_at: SystemTime::now(),
                    serve_count: 1,
                },
            ));
        }
        let mut response = ToolResponse::ok(
            tool,
            mode,
            visible_text,
            refs,
            Accounting {
                raw_tokens: capsule.raw_tokens,
                visible_tokens: final_visible_tokens,
                recovery_tokens: 0,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(ContentType::SearchResult.to_string());
        if storage_error.is_some() {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: format!("could not persist recovery cache for {tool} output"),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
        }
        let mut telemetry = json!({
            "query": query,
            "roots": roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "visited_files": stats.visited_files,
            "matched_files": stats.matched_files,
            "matches": stats.matched_lines,
            "result_limit": max_files,
            "visit_limit": max_visited_files,
            "truncated_by_results": stats.truncated_by_results,
            "truncated_by_visit": stats.truncated_by_visit,
            "search_backend": backend,
            "output_strategy": if grouped { "grouped_by_file" } else { "exact_first_search" },
            "transport_status": if storage_error.is_some() { "degraded" } else { "ok" },
            "degraded": storage_error.is_some(),
            "storage_error": storage_error,
            "exact_refs_available": exact_refs_available
        });
        if let Some(reason) = &fallback_reason {
            telemetry["fallback_reason"] = json!(reason);
        }
        if stats.unparsed_rows > 0 {
            telemetry["rg_unparsed_rows"] = json!(stats.unparsed_rows);
        }
        response.telemetry = Some(telemetry);
        if self.config.session_dedup {
            let delta_bytes = response
                .visible
                .as_ref()
                .map_or(0, |visible| visible.text.len());
            summary.note_wire_bytes(full_bytes, delta_bytes);
        }
        let (from_hwm, to_hwm) = self.session_apply(pending, &summary);
        summary.set_watermark(from_hwm, to_hwm);
        // Merge — never overwrite — so backend/storage telemetry survives a
        // dedup serve in the same response.
        if let Some(extra) = summary.telemetry() {
            merge_telemetry(&mut response, extra);
        }
        if matches.is_empty() {
            let suffix = if stats.truncated_by_results || stats.truncated_by_visit {
                " (scan truncated)"
            } else {
                ""
            };
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# {tool} {} — 0 matches{suffix}", zero_hit_label(query)),
            );
        }
        response
    }

    pub fn glob(
        &self,
        pattern: &str,
        roots: &[PathBuf],
        include_hidden: bool,
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        let matcher = match GlobBuilder::new(pattern).literal_separator(false).build() {
            Ok(glob) => glob.compile_matcher(),
            Err(err) => {
                return ToolResponse::error(
                    "glob",
                    "invalid_glob",
                    err.to_string(),
                    Some("check glob syntax".to_string()),
                );
            }
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        for root in roots {
            if !self.path_allowed(root) {
                return ToolResponse::error(
                    "glob",
                    "path_not_allowed",
                    format!("path is outside allowed roots: {}", root.display()),
                    None,
                );
            }
            collect_glob(
                root,
                root,
                &matcher,
                pattern.contains('/'),
                include_hidden,
                max_files,
                MAX_WALK_DEPTH,
                &mut paths,
            );
        }
        paths.sort();
        paths.dedup();
        let rows = paths.iter().map(|p| display_path(p)).collect::<Vec<_>>();
        let output = rows.join("\n");
        let compact = grouped_path_output(&paths, roots);
        let (visible_source, grouped) = pick_cheaper(&output, &compact);
        let mut response = self.search_result_response(
            "glob",
            pattern,
            &output,
            Some(visible_source),
            mode,
            max_visible_tokens,
        );
        // search_result_response records degraded cache-persist markers in
        // telemetry; fold them into glob's object instead of clobbering them.
        let prior = response.telemetry.take();
        let prior_field = |key: &str| prior.as_ref().and_then(|t| t.get(key)).cloned();
        let degraded = prior_field("degraded")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        response.telemetry = Some(json!({
            "pattern": pattern,
            "roots": roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "matches": rows.len(),
            "include_hidden": include_hidden,
            "output_strategy": if grouped { "grouped_by_root" } else { "exact_first_glob" },
            "transport_status": if degraded { "degraded" } else { "ok" },
            "degraded": degraded,
            "storage_error": prior_field("storage_error").unwrap_or(Value::Null),
            "exact_refs_available": prior_field("exact_refs_available")
                .and_then(|v| v.as_bool())
                .unwrap_or(!response.refs.is_empty())
        }));
        if rows.is_empty() {
            // max_files == 0 stops collect_glob before it scans anything, so
            // an unqualified "0 matches" would be a false affirmative.
            let suffix = if max_files == 0 {
                " (scan truncated)"
            } else {
                ""
            };
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# glob {} — 0 matches{suffix}", zero_hit_label(pattern)),
            );
        }
        response
    }

    pub fn tree(
        &self,
        roots: &[PathBuf],
        depth: usize,
        include_hidden: bool,
        mode: Mode,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        let mut entries: Vec<TreeEntry> = Vec::new();
        let mut spans: Vec<(String, usize)> = Vec::new();
        for root in roots {
            if !self.path_allowed(root) {
                return ToolResponse::error(
                    "tree",
                    "path_not_allowed",
                    format!("path is outside allowed roots: {}", root.display()),
                    None,
                );
            }
            spans.push((root.display().to_string(), entries.len()));
            collect_tree(
                root,
                root,
                depth,
                include_hidden,
                max_files,
                0,
                &mut entries,
            );
        }
        let output = entries
            .iter()
            .map(|entry| entry.rel.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let compact = grouped_tree_output(&entries, &spans, roots.len() > 1);
        let (visible_source, _) = pick_cheaper(&output, &compact);
        let mut response = self.ingest_with_tool(
            "tree",
            &output,
            visible_source,
            ContentType::Tree,
            mode,
            "tree",
            max_visible_tokens,
        );
        if entries.is_empty() {
            // depth == 0 or max_files == 0 stops collect_tree before it scans
            // anything, so an unqualified "0 entries" would be a false
            // affirmative on a populated root.
            let suffix = if max_files == 0 || depth == 0 {
                " (scan truncated)"
            } else {
                ""
            };
            apply_zero_hit_note(&mut response, mode, format!("# tree — 0 entries{suffix}"));
        }
        response
    }

    pub(crate) fn search_result_response(
        &self,
        tool: &str,
        key: &str,
        output: &str,
        rendered: Option<&str>,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let search_refs = store.store_search_output_deferred(output, Some(key));
        let stored =
            store.store_payload_deferred(output, ContentType::SearchResult, None, None, None);
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.persist_pending() {
            Ok(()) => {
                refs.push(ref_record("blob", stored.blob_ref, output.len()));
                refs.push(ref_record("file", stored.file_ref, output.len()));
                refs.extend(search_refs.into_iter().map(|r| ref_record("search", r, 0)));
            }
            Err(err) => storage_error = Some(err.to_string()),
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let capsule = if refs_complete {
            match rendered {
                Some(text) => make_capsule_with_raw_tokens(
                    text,
                    stored.raw_tokens,
                    mode,
                    max_visible_tokens,
                    Some(&format!("{tool} {key}")),
                ),
                None => make_capsule(
                    output,
                    mode,
                    max_visible_tokens,
                    Some(&format!("{tool} {key}")),
                ),
            }
        } else {
            tokenzero_core::Capsule {
                text: output.trim_end().to_string(),
                raw_tokens: stored.raw_tokens,
                visible_tokens: stored.raw_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let mut response = ToolResponse::ok(
            tool,
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
        response.content_type = Some(ContentType::SearchResult.to_string());
        if let Some(error) = storage_error {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: format!("could not persist recovery cache for {tool} output"),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
            response.telemetry = Some(json!({
                "transport_status": "degraded",
                "degraded": true,
                "storage_error": error,
                "exact_refs_available": false
            }));
        }
        response
    }
}
