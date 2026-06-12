#![forbid(unsafe_code)]

mod catalog;
mod diff;
mod fetch_guard;
mod jsonrpc;
mod recall;
mod resources;
mod session;
mod stdio;
mod supervisor;
mod tools;

pub use catalog::{ResourceSpec, ToolSpec, resource_specs, tool_specs};
pub use jsonrpc::handle_jsonrpc;
pub use stdio::run_stdio;
pub use supervisor::run_supervised_stdio;

use fetch_guard::{FETCH_META_MARKER, split_fetch_meta, validate_fetch_target};
use globset::{GlobBuilder, GlobMatcher};
pub(crate) use jsonrpc::{JsonRpcErrorData, handle_jsonrpc_value, jsonrpc_error};
pub(crate) use resources::read_resource;
use serde_json::{Value, json};
use session::{DiffTelemetry, SeenState, ServeKey, ServedRecord, SessionMemory, SessionSummary};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tokenzero_core::{
    Accounting, CLI_SCHEMA_VERSION, ContentType, Mode, ShellRenderInput, ToolResponse,
    count_tokens, detect_content_type, make_capsule, make_capsule_with_raw_tokens, ref_record,
    render_shell, sha256_hex, shell_combined_output,
};
use tokenzero_filters::rewrite_command;
use tokenzero_recovery::{ExpansionResult, RecoveryStore, StoredPayload};
use tokenzero_runtime::{
    RunOutputPolicy, StreamCapture, contains_platform_shell_syntax, run_command_with_policy,
    split_command_string,
};
pub(crate) use tools::call_tool;

pub const DEFAULT_SHELL_TIMEOUT_SECS: u64 = 60;
pub const MAX_SHELL_TIMEOUT_SECS: u64 = 3600;
/// Idle exit is disabled by default: agent sessions can sit idle for hours
/// between tool calls, and an idle-exited server reads as a disconnect to MCP
/// clients. Stale-process cleanup relies on stdin EOF when the client goes
/// away; idle exit remains available as an explicit opt-in.
pub const DEFAULT_MCP_IDLE_TIMEOUT_SECS: u64 = 0;
pub const MAX_MCP_IDLE_TIMEOUT_SECS: u64 = 24 * 60 * 60;
const SEARCH_VISIT_MULTIPLIER: usize = 500;
const MIN_SEARCH_VISITED_FILES: usize = 1_000;
const MAX_SEARCH_VISITED_FILES: usize = 50_000;
pub const SEARCH_BACKEND_ENV: &str = "TOKENZERO_SEARCH_BACKEND";
pub const RG_PATH_ENV: &str = "TOKENZERO_RG_PATH";
pub const SESSION_DEDUP_ENV: &str = "TOKENZERO_MCP_DEDUP";
pub const DIFF_READS_ENV: &str = "TOKENZERO_MCP_DIFF_READS";
/// Diff-aware re-reads skip diffing when either side exceeds these bounds;
/// oversized payloads get a full serve instead (docs/routing.md §5b).
const DIFF_MAX_BYTES: usize = 2 * 1024 * 1024;
const DIFF_MAX_LINES: usize = 50_000;

/// Which scanner backs `tz_find`/`tz_grep`. `Auto` uses ripgrep when a usable
/// binary is found and the internal scanner otherwise; a broken or missing rg
/// always falls back to the internal scanner instead of failing the search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchBackend {
    #[default]
    Auto,
    Rg,
    Internal,
}

impl SearchBackend {
    pub fn from_env() -> Self {
        match std::env::var(SEARCH_BACKEND_ENV).ok().as_deref() {
            Some("rg") => Self::Rg,
            Some("internal") => Self::Internal,
            _ => Self::Auto,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub allowed_roots: Vec<PathBuf>,
    pub cache_path: PathBuf,
    pub max_visible_tokens: usize,
    pub mode: Mode,
    pub shell_timeout: Duration,
    pub shell_capture_bytes: usize,
    pub shell_spill_bytes: usize,
    pub mcp_idle_timeout: Option<Duration>,
    pub search_backend: SearchBackend,
    /// Explicit rg binary path (`TOKENZERO_RG_PATH`); skips the PATH lookup.
    /// Tests set this field directly instead of mutating process-global env.
    pub rg_path_override: Option<PathBuf>,
    /// Session redundancy layer master switch (seen-set dedup; docs/routing.md
    /// §5a). Default comes from `TOKENZERO_MCP_DEDUP`, parsed once at
    /// construction; tests set this field instead of mutating env.
    pub session_dedup: bool,
    /// Diff-aware re-reads (docs/routing.md §5b). Default comes from
    /// `TOKENZERO_MCP_DIFF_READS`, parsed once at construction; only
    /// consulted while `session_dedup` is on.
    pub diff_reads: bool,
    /// Explicit curl binary for `tz_fetch` (`TOKENZERO_CURL_PATH`); tests set
    /// this field directly instead of mutating process-global env.
    pub curl_path_override: Option<PathBuf>,
    /// `tz_fetch` network access is off by default (SSRF surface); opt in
    /// with `TOKENZERO_FETCH=on`. Tests set this field directly.
    pub fetch_enabled: bool,
    /// Hosts (suffix match) explicitly trusted for fetch; they bypass the
    /// post-DNS IP checks. From `TOKENZERO_FETCH_ALLOW`, comma-separated.
    pub fetch_allow_hosts: Vec<String>,
    /// Hosts (suffix match) always refused. From `TOKENZERO_FETCH_DENY`.
    pub fetch_deny_hosts: Vec<String>,
}

impl EngineConfig {
    pub fn for_root(root: &Path) -> Self {
        let output_policy = RunOutputPolicy::default();
        Self {
            allowed_roots: vec![root.to_path_buf()],
            cache_path: root.join(".tokenzero/recovery-cache.json"),
            max_visible_tokens: 4000,
            mode: Mode::Auto,
            shell_timeout: default_shell_timeout(),
            shell_capture_bytes: output_policy.per_stream_capture_bytes,
            shell_spill_bytes: output_policy.spill_threshold_bytes,
            mcp_idle_timeout: default_mcp_idle_timeout(),
            search_backend: SearchBackend::from_env(),
            rg_path_override: std::env::var_os(RG_PATH_ENV).map(PathBuf::from),
            session_dedup: session_dedup_default(),
            diff_reads: diff_reads_default(),
            curl_path_override: std::env::var_os(CURL_PATH_ENV).map(PathBuf::from),
            fetch_enabled: env_opt_in(FETCH_ENABLED_ENV),
            fetch_allow_hosts: env_host_list(FETCH_ALLOW_ENV),
            fetch_deny_hosts: env_host_list(FETCH_DENY_ENV),
        }
    }
}

pub const FETCH_ENABLED_ENV: &str = "TOKENZERO_FETCH";
pub const FETCH_ALLOW_ENV: &str = "TOKENZERO_FETCH_ALLOW";
pub const FETCH_DENY_ENV: &str = "TOKENZERO_FETCH_DENY";

/// Opt-in toggle parse: only `1`/`on`/`true`/`yes` (case-insensitive) enable.
fn env_opt_in(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        ),
        Err(_) => false,
    }
}

fn env_host_list(name: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(|entry| entry.trim().to_ascii_lowercase())
        .filter(|entry| !entry.is_empty())
        .collect()
}

const CURL_PATH_ENV: &str = "TOKENZERO_CURL_PATH";

pub(crate) fn session_dedup_default() -> bool {
    env_toggle_enabled(SESSION_DEDUP_ENV)
}

fn new_session_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    format!("tz-{}-{nanos:x}", std::process::id())
}

pub(crate) fn diff_reads_default() -> bool {
    env_toggle_enabled(DIFF_READS_ENV)
}

/// Opt-out toggle parse: unset means enabled; `0`/`off`/`false`/`no`
/// (case-insensitive) disable.
fn env_toggle_enabled(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "off" | "false" | "no"
        ),
        Err(_) => true,
    }
}

pub fn default_shell_timeout() -> Duration {
    let from_env = std::env::var("TOKENZERO_SHELL_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    shell_timeout_from_secs(from_env)
}

pub fn shell_timeout_from_secs(seconds: Option<u64>) -> Duration {
    let seconds = seconds
        .unwrap_or(DEFAULT_SHELL_TIMEOUT_SECS)
        .clamp(1, MAX_SHELL_TIMEOUT_SECS);
    Duration::from_secs(seconds)
}

pub fn default_mcp_idle_timeout() -> Option<Duration> {
    let from_env = std::env::var("TOKENZERO_MCP_IDLE_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    mcp_idle_timeout_from_secs(from_env)
}

pub fn mcp_idle_timeout_from_secs(seconds: Option<u64>) -> Option<Duration> {
    let seconds = seconds.unwrap_or(DEFAULT_MCP_IDLE_TIMEOUT_SECS);
    if seconds == 0 {
        return None;
    }
    Some(Duration::from_secs(
        seconds.clamp(1, MAX_MCP_IDLE_TIMEOUT_SECS),
    ))
}

/// One find/replace hunk for [`TokenZeroEngine::edit`]. `find` must match the
/// evolving file text exactly once unless `replace_all` is set.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EditHunk {
    pub find: String,
    pub replace: String,
    #[serde(default)]
    pub replace_all: bool,
}

/// Per-call serving options for read/find/grep. Existing positional methods
/// delegate here with defaults so their signatures stay stable.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServeOptions {
    /// Bypass the session redundancy layer for this call: always serve the
    /// full render. The serve is still recorded so later calls can dedup.
    pub fresh: bool,
}

#[derive(Debug)]
pub struct TokenZeroEngine {
    pub config: EngineConfig,
    /// Resolved rg binary, looked up once per engine instance.
    rg_binary: OnceLock<Option<PathBuf>>,
    /// Session-lifetime seen-set for the redundancy layer (docs/routing.md
    /// §5). In-memory only; dies with the server process by design.
    session: Mutex<SessionMemory>,
    /// Stable id for Pulse attribution of every call this engine serves
    /// (one engine per MCP session or CLI command).
    session_id: String,
}

impl TokenZeroEngine {
    pub fn new(config: EngineConfig) -> Self {
        // Self-cleaning storage: every engine (one per MCP session or CLI
        // command) reclaims abandoned temp files and aged spills, so users
        // never have to run cache maintenance by hand.
        let _ = cache_maintenance(&config.cache_path, false);
        Self {
            config,
            rg_binary: OnceLock::new(),
            session: Mutex::new(SessionMemory::default()),
            session_id: new_session_id(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Fail-open lookup: a poisoned session mutex reads as a miss (full
    /// serve, nothing recorded) instead of failing the call.
    fn session_lookup(&self, key: &ServeKey, content_sha256: &str) -> SeenState {
        match self.session.lock() {
            Ok(memory) => memory.lookup(key, content_sha256),
            Err(_) => SeenState::Miss,
        }
    }

    /// Fail-open write-back of this call's serve records and rollup counters.
    fn session_apply(&self, pending: Vec<(ServeKey, ServedRecord)>, summary: &SessionSummary) {
        let Ok(mut memory) = self.session.lock() else {
            return;
        };
        for (key, record) in pending {
            memory.record(key, record);
        }
        memory.absorb(summary);
    }

    fn session_rollup(&self) -> Value {
        match self.session.lock() {
            Ok(memory) => memory.rollup(),
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

    fn rg_binary(&self) -> Option<&Path> {
        self.rg_binary
            .get_or_init(|| match &self.config.rg_path_override {
                Some(path) => path.is_file().then(|| path.clone()),
                None => find_rg_in_path(),
            })
            .as_deref()
    }

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
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let mut visible_parts = Vec::new();
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
            let Ok(text) = text_result else {
                return ToolResponse::error(
                    "read",
                    "read_failed",
                    format!("could not read {}", path.display()),
                    None,
                );
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
            visible_parts.push(part_text);
        }
        if !refs.is_empty() {
            if let Err(err) = store.persist_pending() {
                storage_errors.push(err.to_string());
                refs.clear();
            }
        }
        let refs_complete = prune_dead_refs(&store, &mut refs);
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
        let capsule = make_capsule_with_raw_tokens(
            &assembled,
            count_tokens(&assembled),
            mode,
            max_visible_tokens,
            Some(&format!("edit {}", path.display())),
        );
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

    pub fn ingest(&self, text: &str, kind: ContentType, mode: Mode, source: &str) -> ToolResponse {
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let capsule = make_capsule(text, mode, self.config.max_visible_tokens, Some(source));
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
        prune_dead_refs(&store, &mut refs);
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

    #[allow(clippy::too_many_arguments)]
    fn search(
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
        let max_visited_files = max_search_visited_files(max_files);
        let run_internal = |stats: &mut SearchStats, matches: &mut Vec<SearchMatch>| {
            for root in roots {
                collect_search(
                    root,
                    root,
                    query,
                    max_files,
                    max_visited_files,
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
        let capsule = make_capsule_with_raw_tokens(
            visible_source,
            stored.raw_tokens,
            mode,
            max_visible_tokens,
            Some(&format!("{tool} {query}")),
        );
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
                tool: tool.to_string(),
                query: query.to_string(),
                roots: canonical_roots,
            };
            let content_sha256 = sha256_hex(&output);
            let bypass = matches!(mode, Mode::Passthrough) || options.fresh;
            if let SeenState::Unchanged { serve_count } = self.session_lookup(&key, &content_sha256)
            {
                if !bypass {
                    let note = unchanged_search_note(tool, query, &output, &stored);
                    let note_tokens = count_tokens(&note);
                    // ROI guard: emit only when strictly cheaper than the
                    // full render.
                    if note_tokens < final_visible_tokens {
                        summary.note_dedup(serve_count + 1, final_visible_tokens - note_tokens);
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
        // Merge — never overwrite — so backend/storage telemetry survives a
        // dedup serve in the same response.
        if let Some(extra) = summary.telemetry() {
            merge_telemetry(&mut response, extra);
        }
        self.session_apply(pending, &summary);
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

    #[allow(clippy::too_many_arguments)]
    pub fn shell(
        &self,
        command: &str,
        argv: Option<Vec<String>>,
        cwd: Option<&Path>,
        mode: Mode,
        rewrite: Option<&str>,
        no_rewrite: bool,
        env: Option<BTreeMap<String, String>>,
        stdin: Option<&str>,
        timeout_override: Option<Duration>,
    ) -> ToolResponse {
        let rewrite_mode = rewrite.unwrap_or("off");
        let rewrite_result =
            rewrite_command(command, rewrite_mode, !no_rewrite && rewrite_mode != "off");
        let run_argv = argv.unwrap_or_else(|| {
            if contains_platform_shell_syntax(command, tokenzero_runtime::current_platform()) {
                vec![command.to_string()]
            } else {
                split_command_string(command)
            }
        });
        let env_summary = env
            .as_ref()
            .map(|values| values.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        // Children of any TokenZero-executed command must never re-enter the
        // PATH shim layer (a shim re-wrapping an inner grep corrupts piped
        // results); the shim guard checks this variable. Caller-provided
        // values win. Command::envs is additive, so the inherited
        // environment is unaffected.
        let mut child_env = env.unwrap_or_default();
        child_env
            .entry("TOKENZERO_INNER".to_string())
            .or_insert_with(|| "1".to_string());
        let output_policy = RunOutputPolicy {
            per_stream_capture_bytes: self.config.shell_capture_bytes,
            spill_threshold_bytes: self.config.shell_spill_bytes,
            spill_dir: Some(shell_spill_dir(&self.config.cache_path)),
        }
        .normalized();
        let result = match run_command_with_policy(
            &run_argv,
            cwd,
            Some(&child_env),
            stdin,
            timeout_override.unwrap_or(self.config.shell_timeout),
            false,
            output_policy,
        ) {
            Ok(result) => result,
            Err(err) => {
                return ToolResponse::error(
                    "shell",
                    "spawn_failed",
                    err.to_string(),
                    Some("verify command path and cwd".to_string()),
                );
            }
        };
        let stdout_display = captured_stream_text(&result.stdout, &result.stdout_capture, "stdout");
        let stderr_display = captured_stream_text(&result.stderr, &result.stderr_capture, "stderr");
        let streams_truncated = result.stdout_capture.truncated || result.stderr_capture.truncated;
        let display_command = result.command.as_str();
        let output = shell_combined_output(
            display_command,
            result.exit_code,
            &stdout_display,
            &stderr_display,
        );
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let command_digest = sha256_hex(display_command);
        let stdout_source = PathBuf::from(format!("shell:stdout:{command_digest}"));
        let stderr_source = PathBuf::from(format!("shell:stderr:{command_digest}"));
        let combined_source = PathBuf::from(format!("shell:combined:{command_digest}"));
        let stdout_stored = store.store_payload_deferred(
            &stdout_display,
            ContentType::ShellOutput,
            Some(&stdout_source),
            None,
            None,
        );
        let stderr_stored = store.store_payload_deferred(
            &stderr_display,
            ContentType::ShellOutput,
            Some(&stderr_source),
            None,
            None,
        );
        let combined_stored = store.store_payload_deferred(
            &output,
            ContentType::ShellOutput,
            Some(&combined_source),
            None,
            None,
        );
        let render = render_shell(ShellRenderInput {
            command: display_command,
            stdout: &stdout_display,
            stderr: &stderr_display,
            exit_code: result.exit_code,
            timed_out: result.timed_out,
            mode,
            max_visible_tokens: self.config.max_visible_tokens,
            stdout_ref: Some(&stdout_stored.blob_ref),
            stderr_ref: Some(&stderr_stored.blob_ref),
            combined_ref: Some(&combined_stored.blob_ref),
        });
        let capture = json!({
            "schema_version": "tokenzero.capture.v1",
            "command": display_command,
            "argv": result.argv,
            "cwd": result.cwd,
            "env_summary": env_summary,
            "timing": {"duration_ms": result.duration_ms, "timed_out": result.timed_out},
            "exit_code": result.exit_code,
            "stdout": {
                "bytes": stdout_display.len(),
                "bytes_seen": result.stdout_capture.bytes_seen,
                "captured_bytes": result.stdout_capture.captured_bytes,
                "truncated": result.stdout_capture.truncated,
                "spill_path": result.stdout_capture.spill_path,
                "spill_bytes": result.stdout_capture.spill_bytes,
                "sha256": sha256_hex(&stdout_display),
                "sha256_scope": "captured_display",
                "ref": stdout_stored.blob_ref
            },
            "stderr": {
                "bytes": stderr_display.len(),
                "bytes_seen": result.stderr_capture.bytes_seen,
                "captured_bytes": result.stderr_capture.captured_bytes,
                "truncated": result.stderr_capture.truncated,
                "spill_path": result.stderr_capture.spill_path,
                "spill_bytes": result.stderr_capture.spill_bytes,
                "sha256": sha256_hex(&stderr_display),
                "sha256_scope": "captured_display",
                "ref": stderr_stored.blob_ref
            },
            "combined": {
                "bytes": output.len(),
                "truncated": streams_truncated,
                "sha256": sha256_hex(&output),
                "ref": combined_stored.blob_ref
            },
            "allocator_pressure_relief": result.allocator_pressure_relief,
            "parser_metadata": {
                "policy": render.policy.policy.clone(),
                "policy_reason": render.policy.reason.clone(),
                "family": render.policy.family.clone(),
                "output_strategy": render.output_strategy.clone()
            },
            "command_status": render.command_status.clone()
        });
        let capture_text = serde_json::to_string(&capture).unwrap_or_else(|_| "{}".to_string());
        let capture_source = PathBuf::from(format!("shell:capture:{command_digest}"));
        let capture_stored = store.store_payload_deferred(
            &capture_text,
            ContentType::JsonConfig,
            Some(&capture_source),
            None,
            None,
        );
        if let Err(err) = store.persist_pending() {
            return degraded_shell_response(command, mode, &output, err.to_string());
        }
        let mut refs = vec![
            ref_record(
                "stdout",
                stdout_stored.blob_ref.clone(),
                stdout_display.len(),
            ),
            ref_record(
                "stderr",
                stderr_stored.blob_ref.clone(),
                stderr_display.len(),
            ),
            ref_record("combined", combined_stored.blob_ref.clone(), output.len()),
            ref_record(
                "capture",
                capture_stored.blob_ref.clone(),
                capture_text.len(),
            ),
        ];
        prune_dead_refs(&store, &mut refs);
        let raw_tokens = count_tokens(&output);
        let visible_tokens = count_tokens(&render.visible);
        let mut response = ToolResponse::ok(
            "shell",
            render
                .policy
                .policy
                .parse()
                .unwrap_or(mode.effective_policy()),
            render.visible,
            refs,
            Accounting {
                raw_tokens,
                visible_tokens,
                recovery_tokens: store.recovery_tokens,
                exact_ref_tokens: Some(
                    count_tokens(&stdout_stored.blob_ref)
                        + count_tokens(&stderr_stored.blob_ref)
                        + count_tokens(&combined_stored.blob_ref)
                        + count_tokens(&capture_stored.blob_ref),
                ),
            },
        );
        response.content_type = Some(ContentType::ShellOutput.to_string());
        if streams_truncated {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "shell_output_truncated".to_string(),
                message: "shell output exceeded per-stream capture limits; refs contain captured display output and spill paths point to full local stream logs".to_string(),
                repair: Some("rerun with a narrower command or increase TOKENZERO_SHELL_CAPTURE_BYTES".to_string()),
            });
        }
        response.telemetry = Some(json!({
            "command": display_command,
            "argv": capture["argv"],
            "execution_mode": result.execution_mode,
            "alias_dependency": result.alias_dependency,
            "cwd": capture["cwd"],
            "transport_status": if streams_truncated { "degraded" } else { "ok" },
            "command_success": capture["command_status"]["command_success"],
            "exit_code": capture["exit_code"],
            "failed_segment": capture["command_status"]["failed_segment"],
            "status_label": capture["command_status"]["status_label"],
            "pipeline_masking_warning": capture["command_status"]["pipeline_masking_warning"],
            "pipeline_rerun_command": capture["command_status"]["pipeline_rerun_command"],
            "shell_syntax_summary": capture["command_status"]["shell_syntax_summary"],
            "policy": capture["parser_metadata"]["policy"],
            "policy_reason": capture["parser_metadata"]["policy_reason"],
            "family": capture["parser_metadata"]["family"],
            "timeout": result.timed_out,
            "background_io_terminated": result.io_grace_expired,
            "stdout_preview": preview(&stdout_display),
            "stderr_preview": preview(&stderr_display),
            "stdout_capture": capture["stdout"],
            "stderr_capture": capture["stderr"],
            "allocator_pressure_relief": capture["allocator_pressure_relief"],
            "output_truncated": streams_truncated,
            "rewrite_applied": rewrite_result.applied,
            "rewrite_skip_reason": rewrite_result.reason,
            "latency_ms": result.duration_ms,
            "raw_tokens": raw_tokens,
            "visible_tokens": visible_tokens,
            "recovery_tokens": store.recovery_tokens,
            "capture_ref": capture_stored.blob_ref,
            "stdout_ref": stdout_stored.blob_ref,
            "stderr_ref": stderr_stored.blob_ref,
            "combined_ref": combined_stored.blob_ref,
            "output_strategy": capture["parser_metadata"]["output_strategy"]
        }));
        response.safety = Some(json!({
            "schema_version": "tokenzero.shell_safety.v1",
            "secret_masking": render.policy.policy != "exact" && render.policy.policy != "passthrough",
            "hidden_critical_evidence_requires_ref": true,
            "refs_available": true,
            "refs_cover_full_output": !streams_truncated
        }));
        response
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
        let capsule = make_capsule_with_raw_tokens(
            body,
            stored.raw_tokens,
            mode,
            max_visible_tokens,
            Some(&format!("fetch {}", zero_hit_label(url))),
        );
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

    /// Store `stored_text` as the canonical recoverable payload while
    /// rendering `rendered_text` (a lossless compact projection of it) as the
    /// visible capsule. Accounting keeps raw tokens from the stored payload.
    #[allow(clippy::too_many_arguments)]
    fn ingest_with_tool(
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

    fn search_result_response(
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
        prune_dead_refs(&store, &mut refs);
        let exact_ref_tokens = exact_ref_token_count(&refs);
        let capsule = match rendered {
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

    fn path_allowed(&self, path: &Path) -> bool {
        let abs = comparable_path(path);
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

fn cache_pack_sources(root: &Path, scope: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let common = [
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
        "README.md",
        "Cargo.toml",
        "docs/core.md",
        "docs/mcp.md",
        "docs/command-coverage.md",
    ];
    for rel in common {
        let path = root.join(rel);
        if path.exists() {
            paths.push(path);
        }
    }
    if scope == "agent" || scope == "goal" {
        let goals = root.join("docs/goals");
        if let Ok(entries) = fs::read_dir(goals) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|v| v.to_str()) == Some("md") {
                    paths.push(path);
                }
            }
        }
    }
    paths
}

fn cache_pack_manifest_path(cache_path: &Path, scope: &str) -> PathBuf {
    cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("cache-packs")
        .join(format!("{scope}.json"))
}

fn previous_cache_digest(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    value
        .get("content_digest")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn read_line_range_from_file(path: &Path, start: usize, end: usize) -> std::io::Result<String> {
    let start = start.max(1);
    let end = end.max(start);
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut out = String::new();
    for (idx, line) in reader.lines().enumerate() {
        let line_no = idx + 1;
        if line_no < start {
            continue;
        }
        if line_no > end {
            break;
        }
        out.push_str(&line?);
        out.push('\n');
    }
    Ok(out.trim_end().to_string())
}

/// The disk-spill directory for shell streams, beside the recovery cache.
pub fn shell_spill_dir(cache_path: &Path) -> PathBuf {
    cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("shell-spills")
}

/// Reclaim storage that outlived its session: abandoned recovery-cache temp
/// files (crashed mid-persist) and shell spills past their TTL or the
/// directory byte ceiling. Runs automatically on engine construction;
/// `tokenzero cache prune` runs it explicitly and reports the result.
pub fn cache_maintenance(cache_path: &Path, dry_run: bool) -> Value {
    let tmp_sweep = tokenzero_recovery::sweep_stale_tmp_files(
        cache_path,
        tokenzero_recovery::STALE_TMP_MAX_AGE,
        dry_run,
    );
    let spill_prune = tokenzero_runtime::prune_spill_dir(
        &shell_spill_dir(cache_path),
        tokenzero_runtime::DEFAULT_SPILL_TTL,
        tokenzero_runtime::DEFAULT_SPILL_MAX_TOTAL_BYTES,
        dry_run,
    );
    json!({
        "tmp_sweep": tmp_sweep,
        "spill_prune": spill_prune,
    })
}

/// Build the post-compaction session pack over a workspace's recovery
/// cache: the most recently served payloads with exact refs, token-budgeted.
/// `None` when there is nothing to restore.
pub fn session_pack(cache_path: &Path, max_tokens: usize) -> Option<String> {
    recall::build_session_pack(cache_path, max_tokens)
}

/// `tz_fetch`'s TTL index: url → (blob ref, fetch time). Lives beside the
/// recovery cache; bodies themselves are in the content-addressed store.
/// All IO here is fail-open — a lost index only costs a re-fetch.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct FetchIndex {
    #[serde(default)]
    entries: BTreeMap<String, FetchIndexEntry>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct FetchIndexEntry {
    blob_ref: String,
    fetched_at_secs: u64,
    bytes: usize,
}

fn fetch_index_path(cache_path: &Path) -> PathBuf {
    cache_path
        .parent()
        .map(|dir| dir.join("fetch-cache.json"))
        .unwrap_or_else(|| PathBuf::from("fetch-cache.json"))
}

fn load_fetch_index(path: &Path) -> FetchIndex {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn record_fetch(path: &Path, url: &str, blob_ref: &str, bytes: usize) {
    const MAX_FETCH_INDEX_ENTRIES: usize = 200;
    let mut index = load_fetch_index(path);
    index.entries.insert(
        url.to_string(),
        FetchIndexEntry {
            blob_ref: blob_ref.to_string(),
            fetched_at_secs: epoch_secs(),
            bytes,
        },
    );
    while index.entries.len() > MAX_FETCH_INDEX_ENTRIES {
        let oldest = index
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.fetched_at_secs)
            .map(|(url, _)| url.clone());
        match oldest {
            Some(url) => {
                index.entries.remove(&url);
            }
            None => break,
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(serialized) = serde_json::to_string(&index) {
        let _ = std::fs::write(path, serialized);
    }
}

mod collect;
mod paths;
mod render;

use collect::*;
use paths::*;
use render::*;

pub use render::{cli_json, render_text};

#[cfg(test)]
mod tests;
