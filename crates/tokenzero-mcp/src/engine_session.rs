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
use super::session_persist::SessionPersistence;
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
    pub fn new(config: EngineConfig) -> Self {
        // Self-cleaning storage: every engine (one per MCP session or CLI
        // command) reclaims abandoned temp files and aged spills, so users
        // never have to run cache maintenance by hand.
        let _ = cache_maintenance(&config.cache_path, false);
        let metrics = metrics::ToolMetrics::new(&config.cache_path);
        let session_persist =
            SessionPersistence::for_cache(&config.cache_path, config.session_dedup);
        // Persisted session records are a demand-paged working set. Loading them here
        // would make cold boot proportional to prior session size.
        let boot_root = config.allowed_roots.first().cloned().unwrap_or_else(|| {
            config
                .cache_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        });
        let session_boot = tokenzero_recovery::boot::open_session_boot(
            &config.cache_path,
            &boot_root,
            &config.allowed_roots,
        )
        .ok();
        Self {
            config,
            rg_binary: OnceLock::new(),
            session: Mutex::new(None),
            in_flight: (Mutex::new(HashSet::new()), Condvar::new()),
            session_id: new_session_id(),
            metrics,
            session_persist,
            session_boot,
            surface_health: std::sync::Arc::new(crate::surface_health::SurfaceHealth::new()),
        }
    }

    /// Build an engine that shares crash-only health with a parent session.
    pub(crate) fn with_shared_surface_health(
        config: EngineConfig,
        surface_health: std::sync::Arc<crate::surface_health::SurfaceHealth>,
    ) -> Self {
        let mut engine = Self::new(config);
        engine.surface_health = surface_health;
        engine
    }

    /// Stable, bounded boot capsule and exact attribution buckets.
    pub fn session_boot_snapshot(&self) -> Value {
        let working_set_loaded = self
            .session
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false);
        let mut snapshot = match &self.session_boot {
            Some(boot) => serde_json::to_value(boot).unwrap_or_else(|_| json!({})),
            None => {
                let total = count_tokens("TZ/1 fallback=metadata_unavailable");
                json!({
                    "schema": "tokenzero.session-boot.v1",
                    "mode": "legacy_fallback",
                    "status": "metadata_unavailable",
                    "wire": "TZ/1 fallback=metadata_unavailable",
                    "telemetry": {
                        "manifest": 0,
                        "delta": 0,
                        "toc_working_set": 0,
                        "other": total,
                        "total": total
                    }
                })
            }
        };
        if let Some(object) = snapshot.as_object_mut() {
            object.insert(
                "demand_paging".to_string(),
                json!({"working_set_loaded": working_set_loaded}),
            );
        }
        snapshot
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Crash-only recovery health for CodeMode expand/read (wqw.9).
    pub(crate) fn surface_health(&self) -> &crate::surface_health::SurfaceHealth {
        &self.surface_health
    }

    pub(crate) fn surface_health_handle(
        &self,
    ) -> std::sync::Arc<crate::surface_health::SurfaceHealth> {
        std::sync::Arc::clone(&self.surface_health)
    }

    /// Record one tool-call outcome for observability. Fail-open.
    pub(crate) fn record_tool_call(
        &self,
        tool: &str,
        elapsed: std::time::Duration,
        is_error: bool,
    ) {
        self.metrics.record(tool, elapsed, is_error);
    }

    /// Snapshot served by `resource://tokenzero/metrics`.
    pub(crate) fn tool_metrics_snapshot(&self) -> Value {
        let mut snap = self.metrics.snapshot();
        if let Some(obj) = snap.as_object_mut() {
            obj.insert("session_boot".to_string(), self.session_boot_snapshot());
            obj.insert(
                "surface_health".to_string(),
                self.surface_health.telemetry(),
            );
        }
        snap
    }

    /// Fail-open lookup: a poisoned session mutex reads as a miss (full
    /// serve, nothing recorded) instead of failing the call.
    pub(crate) fn session_lookup(&self, key: &ServeKey, content_sha256: &str) -> SeenState {
        match self.session.lock() {
            Ok(mut slot) => Self::load_session_memory(&mut slot, self.session_persist.as_ref())
                .lookup(key, content_sha256),
            Err(_) => SeenState::Miss,
        }
    }

    /// Fail-open write-back of this call's serve records and rollup counters.
    pub(crate) fn session_apply(
        &self,
        pending: Vec<(ServeKey, ServedRecord)>,
        summary: &SessionSummary,
    ) {
        let Ok(mut slot) = self.session.lock() else {
            return;
        };
        let memory = Self::load_session_memory(&mut slot, self.session_persist.as_ref());
        for (key, record) in pending {
            memory.record(key, record);
        }
        memory.absorb(summary);
        if let Some(ref persist) = self.session_persist {
            persist.persist(memory);
        }
    }

    /// Claim a set of ServeKeys for single-flight serving. Blocks until none
    /// of `keys` is already in flight, then marks them all in flight and
    /// returns a guard that releases them (and wakes waiters) on drop. An
    /// empty key set (dedup off, or nothing dedupable) is a no-op.
    pub(crate) fn begin_serve_flight(&self, keys: Vec<ServeKey>) -> ServeFlight<'_> {
        if !keys.is_empty() {
            let (lock, cvar) = &self.in_flight;
            let mut set = lock.lock().unwrap_or_else(|p| p.into_inner());
            // Wait until every requested key is free, then claim them all at
            // once. Claiming atomically avoids a livelock between two calls
            // whose key sets overlap in opposite order.
            while keys.iter().any(|key| set.contains(key)) {
                set = cvar.wait(set).unwrap_or_else(|p| p.into_inner());
            }
            for key in &keys {
                set.insert(key.clone());
            }
        }
        ServeFlight { engine: self, keys }
    }

    fn load_session_memory<'a>(
        slot: &'a mut Option<SessionMemory>,
        persistence: Option<&SessionPersistence>,
    ) -> &'a mut SessionMemory {
        slot.get_or_insert_with(|| {
            let mut memory = SessionMemory::default();
            if let Some(persist) = persistence {
                persist.load_into(&mut memory);
            }
            memory
        })
    }

    pub(crate) fn session_rollup(&self) -> Value {
        match self.session.lock() {
            Ok(mut slot) => {
                Self::load_session_memory(&mut slot, self.session_persist.as_ref()).rollup()
            }
            Err(_) => json!({
                "records": 0,
                "dedup_hits": 0,
                "diff_hits": 0,
                "visible_tokens_saved": 0,
                "diff_tokens_saved": 0,
                "poisoned": true
            }),
        }
    }

    pub(crate) fn rg_binary(&self) -> Option<&Path> {
        self.rg_binary
            .get_or_init(|| {
                // Prefer engine config override, else portable resolver
                // (env TOKENZERO_RG_PATH → PATH → well-known).
                match &self.config.rg_path_override {
                    Some(path) if path.is_file() => Some(path.clone()),
                    Some(_) => None,
                    None => find_rg_in_path(),
                }
            })
            .as_deref()
    }
}
