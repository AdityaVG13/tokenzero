//! Engine configuration, env toggles, and serve-flight guard.

use crate::session::ServeKey;
use crate::{
    DEFAULT_MCP_IDLE_TIMEOUT_SECS, DEFAULT_SHELL_TIMEOUT_SECS, DIFF_READS_ENV,
    MAX_MCP_IDLE_TIMEOUT_SECS, MAX_SHELL_TIMEOUT_SECS, RG_PATH_ENV, SEARCH_BACKEND_ENV,
    SESSION_DEDUP_ENV, TokenZeroEngine,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tokenzero_core::Mode;
use tokenzero_runtime::RunOutputPolicy;

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
pub(crate) fn env_opt_in(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "on" | "true" | "yes"
        ),
        Err(_) => false,
    }
}

pub(crate) fn env_host_list(name: &str) -> Vec<String> {
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

/// RAII guard releasing a set of in-flight ServeKeys when the serve finishes
/// (or unwinds), waking any request waiting on those keys.
pub(crate) struct ServeFlight<'a> {
    pub(crate) engine: &'a TokenZeroEngine,
    pub(crate) keys: Vec<ServeKey>,
}

impl Drop for ServeFlight<'_> {
    fn drop(&mut self) {
        if self.keys.is_empty() {
            return;
        }
        let (lock, cvar) = &self.engine.in_flight;
        let mut set = lock.lock().unwrap_or_else(|p| p.into_inner());
        for key in &self.keys {
            set.remove(key);
        }
        drop(set);
        cvar.notify_all();
    }
}

pub(crate) fn new_session_id() -> String {
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
pub(crate) fn env_toggle_enabled(name: &str) -> bool {
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
