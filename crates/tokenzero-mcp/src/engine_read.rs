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
    pub fn read(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        self.read_with_options(
            paths,
            mode,
            start_line,
            end_line,
            raw,
            max_files,
            max_visible_tokens,
            ServeOptions::default(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn read_with_options(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        let response = self.read_with_options_inner(
            paths,
            mode,
            start_line,
            end_line,
            raw,
            max_files,
            max_visible_tokens,
            options,
        );
        let ok = response.error.is_none();
        let code = response.error.as_ref().map(|err| err.code.as_str());
        self.surface_health().record_read_outcome(ok, code);
        response
    }

    #[allow(clippy::too_many_arguments)]
    fn read_with_options_inner(
        &self,
        paths: &[PathBuf],
        mode: Mode,
        start_line: Option<usize>,
        end_line: Option<usize>,
        raw: bool,
        max_files: usize,
        max_visible_tokens: usize,
        options: ServeOptions,
    ) -> ToolResponse {
        // Single-flight the serve so a second pipelined identical read waits
        // for this one to record its serve before it looks up the seen-set
        // (otherwise both miss and both serve full). Keyed per path+range, so
        // disjoint reads still run fully concurrently. Held until after
        // session_apply via the guard's lifetime.
        let _flight = if self.config.session_dedup {
            let keys = paths
                .iter()
                .take(max_files)
                .map(|path| ServeKey::File {
                    path: comparable_path(path),
                    start: start_line,
                    end: end_line,
                })
                .collect();
            self.begin_serve_flight(keys)
        } else {
            self.begin_serve_flight(Vec::new())
        };
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let mut visible_parts = Vec::new();
        let mut raw_visible_parts = Vec::new();
        let mut refs = Vec::new();
        let mut raw_tokens = 0usize;
        let mut visible_tokens = 0usize;
        let mut storage_errors = Vec::new();
        let mut content_types = Vec::new();
        let mut bytes_read = 0usize;
        let mut summary = SessionSummary::default();
        // Serve records are applied only after every path succeeded: an
        // error response serves nothing, so nothing may be marked as seen.
        let mut pending: Vec<(ServeKey, ServedRecord)> = Vec::new();
        // Dedup/diff substitutions are buffered and applied only after this
        // call's refs persist: a note replaces content with refs, which is
        // only safe when the refs are actually recoverable.
        let mut substitutions: Vec<PendingSubstitution> = Vec::new();
        for path in paths.iter().take(max_files) {
            if !self.path_allowed(path) {
                return ToolResponse::error(
                    "read",
                    "path_not_allowed",
                    format!("path is outside allowed roots: {}", path.display()),
                    None,
                );
            }
            let source_start = start_line;
            let source_end = end_line;
            let text_result = if let Some(start) = start_line {
                read_line_range_from_file(path, start, end_line.unwrap_or(start))
            } else {
                fs::read_to_string(path)
            };
            let text = match text_result {
                Ok(text) => text,
                Err(err) => {
                    // "could not read X (read_failed)" with no cause stranded
                    // live sessions guessing between missing file, directory,
                    // and permissions. Name the reason and the obvious next op.
                    let hint = if path.is_dir() {
                        " (path is a directory - use tree)"
                    } else if !path.exists() {
                        " (no such file)"
                    } else {
                        ""
                    };
                    return ToolResponse::error(
                        "read",
                        "read_failed",
                        format!("could not read {}: {err}{hint}", path.display()),
                        None,
                    );
                }
            };
            bytes_read += text.len();
            let ctype = detect_content_type(&text, Some(path));
            content_types.push(ctype);
            let stored = store.store_payload_deferred_batch(
                &text,
                ctype,
                Some(path),
                source_start,
                source_end,
            );
            refs.push(ref_record("blob", stored.blob_ref.clone(), text.len()));
            refs.push(ref_record("file", stored.file_ref.clone(), text.len()));
            let capsule = if raw {
                tokenzero_core::Capsule {
                    text: text.trim_end().to_string(),
                    raw_tokens: stored.raw_tokens,
                    visible_tokens: stored.raw_tokens,
                    omitted_lines: 0,
                    mode,
                }
            } else {
                tokenzero_core::make_capsule_with_recovery_ref(
                    &text,
                    stored.raw_tokens,
                    mode,
                    max_visible_tokens,
                    Some(&path.display().to_string()),
                    Some(&stored.file_ref),
                )
            };
            let part_text = capsule.text;
            let part_tokens = capsule.visible_tokens;
            // Session redundancy layer (docs/routing.md §5). Zero-payload
            // notes are cheap and stay untouched: empty payloads skip the
            // layer entirely (notes are never deduped).
            if self.config.session_dedup && !text.is_empty() {
                let key = ServeKey::File {
                    path: comparable_path(path),
                    start: source_start,
                    end: source_end,
                };
                let content_sha256 = sha256_hex(&text);
                // raw keeps the verbatim-slice contract, passthrough keeps
                // its verbatim-payload contract, and fresh is the per-call
                // opt-out; all three bypass the replacement render but still
                // record the serve below so later calls can dedup.
                let bypass = raw || matches!(mode, Mode::Passthrough) || options.fresh;
                match self.session_lookup(&key, &content_sha256) {
                    SeenState::Unchanged { serve_count } if !bypass => {
                        let note = unchanged_read_note(path, &text, &stored);
                        let note_tokens = count_tokens(&note);
                        // ROI guard: a note that costs as much as the full
                        // render is never emitted.
                        if note_tokens < part_tokens {
                            substitutions.push(PendingSubstitution::Dedup {
                                idx: visible_parts.len(),
                                note,
                                note_tokens,
                                full_tokens: part_tokens,
                                serve_count: serve_count + 1,
                            });
                        }
                    }
                    SeenState::Changed { previous } if !bypass && self.config.diff_reads => {
                        if let Some((diff_text, diff_tokens, telemetry)) = diff_since_served(
                            &mut store,
                            path,
                            &text,
                            &previous,
                            &stored,
                            part_tokens,
                        ) {
                            substitutions.push(PendingSubstitution::Diff {
                                idx: visible_parts.len(),
                                text: diff_text,
                                diff_tokens,
                                full_tokens: part_tokens,
                                telemetry,
                            });
                        }
                    }
                    _ => {}
                }
                pending.push((
                    key,
                    ServedRecord {
                        content_sha256,
                        blob_ref: stored.blob_ref.clone(),
                        file_ref: stored.file_ref.clone(),
                        raw_tokens: stored.raw_tokens,
                        line_count: text.lines().count(),
                        byte_len: text.len(),
                        served_at: SystemTime::now(),
                        serve_count: 1,
                    },
                ));
            }
            raw_tokens += capsule.raw_tokens;
            visible_tokens += part_tokens;
            raw_visible_parts.push(text.trim_end().to_string());
            visible_parts.push(part_text);
        }
        if !refs.is_empty() {
            if let Err(err) = store.persist_pending() {
                storage_errors.push(err.to_string());
                refs.clear();
            }
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        if !refs_complete {
            visible_parts = raw_visible_parts;
            visible_tokens = raw_tokens;
        }
        // Dedup/diff notes advertise refs in place of content: apply them
        // only when persistence succeeded AND every ref survived eviction.
        // Degraded storage always serves full — the bytes are in the text,
        // which is unconditionally safe.
        if storage_errors.is_empty() && refs_complete {
            for substitution in substitutions {
                match substitution {
                    PendingSubstitution::Dedup {
                        idx,
                        note,
                        note_tokens,
                        full_tokens,
                        serve_count,
                    } => {
                        summary.note_dedup(serve_count, full_tokens - note_tokens);
                        visible_tokens -= full_tokens - note_tokens;
                        visible_parts[idx] = note;
                    }
                    PendingSubstitution::Diff {
                        idx,
                        text,
                        diff_tokens,
                        full_tokens,
                        telemetry,
                    } => {
                        summary.note_diff(telemetry, full_tokens - diff_tokens);
                        visible_tokens -= full_tokens - diff_tokens;
                        visible_parts[idx] = text;
                    }
                }
            }
        }
        let exact_refs_available = !refs.is_empty();
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = ToolResponse::ok(
            "read",
            mode,
            visible_parts.join("\n\n"),
            refs,
            Accounting {
                raw_tokens,
                visible_tokens,
                recovery_tokens: store.recovery_tokens,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(common_content_type(&content_types).to_string());
        if !storage_errors.is_empty() {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: "could not persist recovery cache for one or more read paths".to_string(),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
            response.telemetry = Some(json!({
                "transport_status": "degraded",
                "degraded": true,
                "storage_errors": storage_errors,
                "exact_refs_available": exact_refs_available
            }));
        }
        // Merge — never overwrite — so degraded-storage markers survive a
        // dedup/diff serve in the same response.
        if let Some(extra) = summary.telemetry() {
            merge_telemetry(&mut response, extra);
        }
        // A serve whose refs failed to persist (or were evicted before the
        // response returned) must not become a dedup base.
        if storage_errors.is_empty() && refs_complete {
            self.session_apply(pending, &summary);
        }
        // Raw reads keep the verbatim slice contract even when it is empty;
        // raw=true does not imply Mode::Passthrough, so guard it explicitly.
        if !raw && bytes_read == 0 {
            let label = zero_hit_label(
                &paths
                    .iter()
                    .take(max_files)
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            apply_zero_hit_note(&mut response, mode, format!("# read {label} — 0 bytes"));
        }
        response
    }
}
