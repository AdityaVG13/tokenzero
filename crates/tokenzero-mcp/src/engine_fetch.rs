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
    /// Fetch a URL through the system curl with a TTL'd cache over the
    /// recovery store: a fresh-enough prior fetch serves the stored body
    /// without touching the network. Every serve carries exact refs; `fresh`
    /// bypasses the TTL.
    pub fn fetch(
        &self,
        url: &str,
        ttl_seconds: Option<usize>,
        fresh: bool,
        mode: Mode,
        max_visible_tokens: usize,
    ) -> ToolResponse {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return ToolResponse::error(
                "fetch",
                "invalid_url",
                format!("fetch requires an http(s) URL, got {url}"),
                None,
            );
        }
        if !self.config.fetch_enabled {
            return ToolResponse::error(
                "fetch",
                "fetch_disabled",
                "network fetches are disabled by default",
                Some(format!(
                    "set {FETCH_ENABLED_ENV}=on (optionally {FETCH_ALLOW_ENV}=host1,host2) to enable"
                )),
            );
        }
        let ttl_secs = ttl_seconds.unwrap_or(24 * 60 * 60) as u64;
        let index_path = fetch_index_path(&self.config.cache_path);
        if let Err(blocked) = validate_fetch_target(
            url,
            &self.config.fetch_allow_hosts,
            &self.config.fetch_deny_hosts,
        ) {
            return ToolResponse::error("fetch", blocked.code, blocked.message, blocked.repair);
        }
        if !fresh {
            if let Some(entry) = load_fetch_index(&index_path).entries.get(url) {
                let age = epoch_secs().saturating_sub(entry.fetched_at_secs);
                if age <= ttl_secs {
                    let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
                    let cached = store.expand(&entry.blob_ref, Some("raw"), None, None, None, None);
                    if cached.found {
                        let recovery_tokens = store.recovery_tokens;
                        return self.fetch_response(
                            url,
                            &cached.content,
                            mode,
                            max_visible_tokens,
                            true,
                            age,
                            recovery_tokens,
                            &index_path,
                        );
                    }
                }
            }
        }
        let curl = self
            .config
            .curl_path_override
            .clone()
            .unwrap_or_else(|| PathBuf::from("curl"));
        // Redirects are followed manually so every hop's target is validated
        // (and pinned) like the entry URL — a redirect to an internal address
        // is the classic SSRF bypass.
        const MAX_FETCH_REDIRECTS: usize = 5;
        let mut current_url = url.to_string();
        let mut redirect_hops = 0usize;
        let body = loop {
            let target = match validate_fetch_target(
                &current_url,
                &self.config.fetch_allow_hosts,
                &self.config.fetch_deny_hosts,
            ) {
                Ok(target) => target,
                Err(blocked) => {
                    return ToolResponse::error(
                        "fetch",
                        blocked.code,
                        blocked.message,
                        blocked.repair,
                    );
                }
            };
            let mut argv: Vec<String> = vec![
                curl.display().to_string(),
                "-sS".to_string(),
                "--max-time".to_string(),
                "30".to_string(),
                "--proto".to_string(),
                "=http,https".to_string(),
                "-w".to_string(),
                format!("\n{FETCH_META_MARKER} %{{http_code}} %{{redirect_url}}"),
            ];
            if let Some(ip) = target.pinned_ip {
                argv.push("--resolve".to_string());
                argv.push(format!("{}:{}:{}", target.host, target.port, ip));
            }
            argv.push(current_url.clone());
            let mut child_env = BTreeMap::new();
            child_env.insert("TOKENZERO_INNER".to_string(), "1".to_string());
            let output_policy = RunOutputPolicy {
                per_stream_capture_bytes: self.config.shell_capture_bytes,
                spill_threshold_bytes: self.config.shell_spill_bytes,
                spill_dir: Some(shell_spill_dir(&self.config.cache_path)),
            }
            .normalized();
            let result = match run_command_with_policy(
                &argv,
                None,
                Some(&child_env),
                None,
                Duration::from_secs(45),
                false,
                output_policy,
            ) {
                Ok(result) => result,
                Err(err) => {
                    return ToolResponse::error(
                        "fetch",
                        "fetch_failed",
                        format!("could not run curl: {err}"),
                        Some("install curl or set TOKENZERO_CURL_PATH".to_string()),
                    );
                }
            };
            if !result.ok || result.exit_code != Some(0) {
                let stderr: String = result.stderr.trim().chars().take(300).collect();
                return ToolResponse::error(
                    "fetch",
                    "fetch_failed",
                    format!("curl exited with {:?}: {stderr}", result.exit_code),
                    Some("check the URL and network access".to_string()),
                );
            }
            let (body, http_code, redirect_url) = split_fetch_meta(&result.stdout);
            match (http_code, redirect_url) {
                (Some(code), Some(next)) if (300..400).contains(&code) => {
                    redirect_hops += 1;
                    if redirect_hops > MAX_FETCH_REDIRECTS {
                        return ToolResponse::error(
                            "fetch",
                            "too_many_redirects",
                            format!("more than {MAX_FETCH_REDIRECTS} redirects from {url}"),
                            None,
                        );
                    }
                    current_url = next;
                }
                _ => break body,
            }
        };
        self.fetch_response(
            url,
            &body,
            mode,
            max_visible_tokens,
            false,
            0,
            0,
            &index_path,
        )
    }

    /// Shared fetch render: store the body (content-addressed refs are
    /// identical for cache hits, keeping every serve recoverable), update the
    /// TTL index on fresh fetches, capsule within budget.
    #[allow(clippy::too_many_arguments)]
    fn fetch_response(
        &self,
        url: &str,
        body: &str,
        mode: Mode,
        max_visible_tokens: usize,
        cache_hit: bool,
        age_seconds: u64,
        recovery_tokens: usize,
        index_path: &Path,
    ) -> ToolResponse {
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let ctype = detect_content_type(body, None);
        let stored = store.store_payload_deferred(body, ctype, None, None, None);
        let mut refs = Vec::new();
        let mut storage_error = None;
        match store.persist_pending() {
            Ok(()) => {
                refs.push(ref_record("blob", stored.blob_ref.clone(), body.len()));
                refs.push(ref_record("file", stored.file_ref.clone(), body.len()));
            }
            Err(err) => storage_error = Some(err.to_string()),
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
        // An evicted blob must not enter the fetch index: a later cache hit
        // would advertise a ref that cannot be expanded.
        if !cache_hit && storage_error.is_none() && refs_complete {
            record_fetch(index_path, url, &stored.blob_ref, body.len());
        }
        let capsule = if refs_complete {
            make_capsule_with_raw_tokens(
                body,
                stored.raw_tokens,
                mode,
                max_visible_tokens,
                Some(&format!("fetch {}", zero_hit_label(url))),
            )
        } else {
            tokenzero_core::Capsule {
                text: body.trim_end().to_string(),
                raw_tokens: stored.raw_tokens,
                visible_tokens: stored.raw_tokens,
                omitted_lines: 0,
                mode,
            }
        };
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let mut response = ToolResponse::ok(
            "fetch",
            mode,
            capsule.text,
            refs,
            Accounting {
                raw_tokens: capsule.raw_tokens,
                visible_tokens: capsule.visible_tokens,
                recovery_tokens: recovery_tokens + store.recovery_tokens,
                exact_ref_tokens: Some(exact_ref_tokens),
            },
        );
        response.content_type = Some(ctype.to_string());
        response.telemetry = Some(json!({
            "url": url,
            "cache_hit": cache_hit,
            "age_seconds": age_seconds,
            "bytes": body.len(),
            "transport_status": if storage_error.is_some() { "degraded" } else { "ok" },
            "degraded": storage_error.is_some(),
            "storage_error": storage_error.clone(),
        }));
        if storage_error.is_some() {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "cache_write_failed".to_string(),
                message: "could not persist recovery cache for fetch output".to_string(),
                repair: Some("fix recovery cache permissions or pass --cache-path".to_string()),
            });
        }
        if body.is_empty() {
            apply_zero_hit_note(
                &mut response,
                mode,
                format!("# fetch {} — 0 bytes", zero_hit_label(url)),
            );
        }
        response
    }
}
