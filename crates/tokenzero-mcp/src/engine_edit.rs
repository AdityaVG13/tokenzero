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
    /// One-call multi-hunk read+verify+edit. Hunks apply sequentially against
    /// the evolving text and the batch is all-or-nothing: any failed hunk
    /// aborts before a single byte reaches disk. The pre-image blob ref is
    /// the undo ref.
    pub fn edit(
        &self,
        path: &Path,
        edits: &[EditHunk],
        create: bool,
        dry_run: bool,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if !self.path_allowed(path) {
            return ToolResponse::error(
                "edit",
                "path_not_allowed",
                format!("path is outside allowed roots: {}", path.display()),
                None,
            );
        }
        if edits.is_empty() {
            return ToolResponse::error(
                "edit",
                "edit_failed",
                "no edit hunks provided".to_string(),
                Some("pass at least one {find, replace} hunk".to_string()),
            );
        }
        if create && (edits.len() != 1 || !edits[0].find.is_empty()) {
            return ToolResponse::error(
                "edit",
                "edit_failed",
                "create=true requires exactly one hunk with an empty find".to_string(),
                Some(
                    r#"pass edits=[{"find": "", "replace": "<full new-file content>"}]"#
                        .to_string(),
                ),
            );
        }
        let old_text = if create {
            if path.exists() {
                return ToolResponse::error(
                    "edit",
                    "edit_failed",
                    format!("create=true but file already exists: {}", path.display()),
                    Some("drop create=true to edit the existing content".to_string()),
                );
            }
            String::new()
        } else {
            let bytes = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(err) => {
                    return ToolResponse::error(
                        "edit",
                        "edit_failed",
                        format!("could not read {}: {err}", path.display()),
                        Some("pass create=true to create a new file".to_string()),
                    );
                }
            };
            match String::from_utf8(bytes) {
                Ok(text) => text,
                Err(_) => {
                    return ToolResponse::error(
                        "edit",
                        "not_utf8",
                        format!(
                            "{} is not valid UTF-8; edit only handles text files",
                            path.display()
                        ),
                        None,
                    );
                }
            }
        };
        let applied = if create {
            create_file_hunk(&edits[0])
        } else {
            apply_edit_hunks(&old_text, edits)
        };
        let applied = match applied {
            Ok(applied) => applied,
            Err(failure) => {
                return ToolResponse::error("edit", failure.code, failure.message, failure.repair);
            }
        };
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        // Pre-image blob is the undo ref; post-image blob/file refs recover
        // the new content. Persist before writing so undo survives the write.
        let pre_stored = store.store_payload_deferred(
            &old_text,
            detect_content_type(&old_text, Some(path)),
            Some(path),
            None,
            None,
        );
        let post_stored = store.store_payload_deferred(
            &applied.text,
            detect_content_type(&applied.text, Some(path)),
            Some(path),
            None,
            None,
        );
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.persist_pending() {
            Ok(()) => {
                refs.push(ref_record(
                    "blob",
                    post_stored.blob_ref.clone(),
                    applied.text.len(),
                ));
                refs.push(ref_record(
                    "file",
                    post_stored.file_ref.clone(),
                    applied.text.len(),
                ));
                refs.push(ref_record("undo", pre_stored.blob_ref, old_text.len()));
            }
            Err(err) => storage_error = Some(err.to_string()),
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        if !dry_run {
            if let Err(err) = write_atomic(path, applied.text.as_bytes()) {
                return ToolResponse::error(
                    "edit",
                    "edit_failed",
                    format!("could not write {}: {err}", path.display()),
                    Some("check directory permissions".to_string()),
                );
            }
            // Seed the seen-set with the post-image so the canonical
            // read → edit → re-read flow serves an unchanged note instead of
            // re-paying the hunks as a diff. Same persistence rule as
            // read/search serves: refs that failed to persist never become a
            // dedup base.
            if storage_error.is_none()
                && refs_complete
                && self.config.session_dedup
                && !applied.text.is_empty()
            {
                self.session_apply(
                    vec![(
                        ServeKey::File {
                            path: comparable_path(path),
                            start: None,
                            end: None,
                        },
                        ServedRecord {
                            content_sha256: sha256_hex(&applied.text),
                            blob_ref: post_stored.blob_ref.clone(),
                            file_ref: post_stored.file_ref.clone(),
                            raw_tokens: post_stored.raw_tokens,
                            line_count: applied.text.lines().count(),
                            byte_len: applied.text.len(),
                            served_at: SystemTime::now(),
                            serve_count: 1,
                        },
                    )],
                    &SessionSummary::default(),
                );
            }
        }
        let header = if dry_run {
            format!(
                "# edit {} — dry-run: {} hunks would apply (+{} -{} lines)",
                path.display(),
                edits.len(),
                applied.lines_added,
                applied.lines_removed
            )
        } else {
            format!(
                "# edit {} — {} hunks applied (+{} -{} lines)",
                path.display(),
                edits.len(),
                applied.lines_added,
                applied.lines_removed
            )
        };
        let assembled = if applied.diff.is_empty() {
            header
        } else {
            format!("{header}\n{}", applied.diff)
        };
        let assembled_tokens = count_tokens(&assembled);
        let capsule = if refs_complete {
            make_capsule_with_raw_tokens(
                &assembled,
                assembled_tokens,
                mode,
                max_visible_tokens,
                Some(&format!("edit {}", path.display())),
            )
        } else {
            tokenzero_core::Capsule {
                text: assembled.trim_end().to_string(),
                raw_tokens: assembled_tokens,
                visible_tokens: assembled_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let exact_refs_available = !refs.is_empty();
        let mut response = ToolResponse::ok(
            "edit",
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
        response.content_type = Some(ContentType::Diff.to_string());
        if storage_error.is_some() {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: "could not persist recovery cache for edit pre/post images".to_string(),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
        }
        response.telemetry = Some(json!({
            "path": path.display().to_string(),
            "hunks": edits.len(),
            "lines_added": applied.lines_added,
            "lines_removed": applied.lines_removed,
            "create": create,
            "dry_run": dry_run,
            "transport_status": if storage_error.is_some() { "degraded" } else { "ok" },
            "degraded": storage_error.is_some(),
            "storage_error": storage_error,
            "exact_refs_available": exact_refs_available
        }));
        response
    }
}
