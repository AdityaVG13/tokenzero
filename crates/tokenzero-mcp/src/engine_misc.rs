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
    split_command_string, tool_specs,
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
    pub fn mem(&self) -> ToolResponse {
        let store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let mut status = store.export_status();
        if let Some(object) = status.as_object_mut() {
            object.insert("session_dedup".to_string(), self.session_rollup());
        }
        let text = serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".to_string());
        ToolResponse::ok(
            "mem",
            Mode::Hybrid,
            text.clone(),
            Vec::new(),
            Accounting {
                raw_tokens: count_tokens(&text),
                visible_tokens: count_tokens(&text),
                recovery_tokens: 0,
                exact_ref_tokens: Some(0),
            },
        )
    }

    pub fn cache_pack(&self, scope: &str) -> ToolResponse {
        let root = self
            .config
            .allowed_roots
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from("."));
        let mut stable_sections = Vec::new();
        let mut source_paths = cache_pack_sources(&root, scope);
        source_paths.sort();
        source_paths.dedup();
        for path in &source_paths {
            if let Ok(text) = fs::read_to_string(path) {
                stable_sections.push(format!(
                    "## {}\n{}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    text.trim_end()
                ));
            }
        }
        let mut repo_rows = Vec::new();
        collect_tree(&root, &root, 3, false, 500, 0, &mut repo_rows);
        repo_rows.retain(|row| {
            !row.rel.contains("recovery-cache")
                && !row.rel.contains("cache.json")
                && !row.rel.contains("cache-packs")
                && !row.rel.starts_with(".tokenzero")
        });
        stable_sections.push(format!(
            "## repo-map\n{}",
            repo_rows
                .iter()
                .map(|row| row.rel.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        ));
        let tool_schema = serde_json::to_string_pretty(&tool_specs()).unwrap_or_default();
        stable_sections.push(format!("## mcp-tool-schema\n{tool_schema}"));
        let stable_text = stable_sections.join("\n\n");
        let volatile_text = format!(
            "volatile_tail:\nroot: {}\nmanifest: {}\nexpand refs for exact current source bytes\n",
            root.display(),
            cache_pack_manifest_path(&self.config.cache_path, scope).display()
        );
        let content_digest = sha256_hex(&stable_text);
        let cache_key = format!(
            "tz-cache-pack-v1:{}:{}",
            scope,
            content_digest.chars().take(16).collect::<String>()
        );
        let manifest_path = cache_pack_manifest_path(&self.config.cache_path, scope);
        let invalidation_reason = previous_cache_digest(&manifest_path)
            .map(|previous| {
                if previous == content_digest {
                    "unchanged".to_string()
                } else {
                    "sources_changed".to_string()
                }
            })
            .unwrap_or_else(|| "new_pack".to_string());
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let stable_stored = store.store_payload_deferred(
            &stable_text,
            ContentType::Markdown,
            Some(Path::new("cache-pack:stable-prefix")),
            None,
            None,
        );
        let volatile_stored = store.store_payload_deferred(
            &volatile_text,
            ContentType::Markdown,
            Some(Path::new("cache-pack:volatile-tail")),
            None,
            None,
        );
        if let Err(err) = store.persist_pending() {
            return ToolResponse::error(
                "cache-pack",
                "cache_write_failed",
                err.to_string(),
                Some("fix recovery cache permissions".to_string()),
            );
        }
        // The manifest embeds these refs; if eviction dropped either during
        // the persist, fail loud instead of publishing dead handles.
        if !store.has_ref(&stable_stored.blob_ref) || !store.has_ref(&volatile_stored.blob_ref) {
            return ToolResponse::error(
                "cache-pack",
                "cache_evicted",
                "cache pack payload was evicted from the recovery cache before it could be advertised",
                Some("increase recovery cache max_bytes or reduce the pack scope".to_string()),
            );
        }
        let cacheable_tokens = count_tokens(&stable_text);
        let volatile_tokens = count_tokens(&volatile_text);
        let invalidation_count = if invalidation_reason == "unchanged" {
            0
        } else {
            1
        };
        let manifest = json!({
            "schema_version": "tokenzero.cache-pack.v1",
            "status": "ok",
            "scope": scope,
            "cache_key": cache_key,
            "content_digest": content_digest,
            "cacheable_tokens": cacheable_tokens,
            "stable_prefix_tokens": cacheable_tokens,
            "volatile_tokens": volatile_tokens,
            "estimated_cached_tokens": cacheable_tokens,
            "estimated_cached_token_savings": cacheable_tokens.saturating_sub(count_tokens(&stable_stored.blob_ref)),
            "prefix_stability_ratio": if cacheable_tokens + volatile_tokens == 0 { 0.0 } else { cacheable_tokens as f64 / (cacheable_tokens + volatile_tokens) as f64 },
            "invalidation_reason": invalidation_reason,
            "invalidation_count": invalidation_count,
            "daemon_required": false,
            "source_count": source_paths.len(),
            "source_paths": source_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "source_refs": [stable_stored.blob_ref.clone()],
            "volatile_refs": [volatile_stored.blob_ref.clone()],
            "host_hints": {
                "stable_prefix_first": true,
                "volatile_tail_last": true,
                "expand_before_sensitive_action": true
            },
            "manifest_path": manifest_path.display().to_string()
        });
        if let Some(parent) = manifest_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n",
        );
        let visible = serde_json::to_string_pretty(&manifest).unwrap_or_default();
        let refs = vec![
            ref_record("stable_prefix", stable_stored.blob_ref, stable_text.len()),
            ref_record(
                "volatile_tail",
                volatile_stored.blob_ref,
                volatile_text.len(),
            ),
        ];
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = ToolResponse::ok(
            "cache-pack",
            Mode::Structured,
            visible.clone(),
            refs,
            Accounting {
                raw_tokens: cacheable_tokens + volatile_tokens,
                visible_tokens: count_tokens(&visible),
                recovery_tokens: store.recovery_tokens,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(ContentType::JsonConfig.to_string());
        response.telemetry = Some(json!({
            "cache_key": manifest["cache_key"],
            "content_digest": manifest["content_digest"],
            "cacheable_tokens": cacheable_tokens,
            "volatile_tokens": volatile_tokens,
            "invalidation_reason": manifest["invalidation_reason"],
            "daemon_required": false
        }));
        response
    }

    pub(crate) fn path_allowed(&self, path: &Path) -> bool {
        let abs = if path.is_absolute() {
            comparable_path(path)
        } else {
            comparable_path(&self.config.call_root.join(path))
        };
        // canonicalize_existing_prefix can only resolve `..` while the prefix
        // exists on disk; a `..` left behind a nonexistent component would
        // defeat the component-wise root check below, so fail closed. The
        // filesystem rejects such paths anyway (every component before `..`
        // must exist), so nothing readable is lost.
        if abs
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return false;
        }
        self.config.allowed_roots.iter().any(|root| {
            let root = comparable_path(root);
            abs.starts_with(root)
        })
    }
}
