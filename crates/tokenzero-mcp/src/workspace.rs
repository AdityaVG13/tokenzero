//! Workspace root and cache-path resolution shared by CodeMode, MCP, and CLI.
//!
//! Multi-project isolation (wqw.2): process-global `ZEROSTACK_STORE_ROOT` does
//! **not** pin every call root into one store by default. Shared/meta store
//! requires `TOKENZERO_SHARED_STORE` / `ZEROSTACK_SHARED_STORE` opt-in.

use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Env vars that opt in to using `ZEROSTACK_STORE_ROOT` as a shared meta store.
pub const SHARED_STORE_OPT_IN_ENVS: &[&str] = &["TOKENZERO_SHARED_STORE", "ZEROSTACK_SHARED_STORE"];

/// Global pin env names.
pub const STORE_ROOT_ENVS: &[&str] = &["ZEROSTACK_STORE_ROOT", "ZERO_STACK_STORE_ROOT"];

/// Workspace root for TokenZero persistence (CLI, CodeMode, MCP).
pub fn tokenzero_work_root(explicit_root: Option<PathBuf>) -> PathBuf {
    explicit_root
        .or_else(|| std::env::var_os("TOKENZERO_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

/// Default single-root allowlist for a workspace.
pub fn default_allowed_roots(root: &Path) -> Vec<PathBuf> {
    vec![root.to_path_buf()]
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    let candidate_cmp = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.clone());
    let exists = paths
        .iter()
        .any(|path| path.canonicalize().unwrap_or_else(|_| path.clone()) == candidate_cmp);
    if !exists {
        paths.push(candidate);
    }
}

/// Merge explicit allowed roots with the workspace root, deduplicating by canonical path.
pub fn allowed_roots_for_workspace(root: &Path, explicit: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = if explicit.is_empty() {
        default_allowed_roots(root)
    } else {
        explicit.to_vec()
    };
    push_unique_path(&mut roots, root.to_path_buf());
    roots
}

fn env_truthy(value: &OsStr) -> bool {
    let raw = value.to_string_lossy();
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "on" | "true" | "yes"
    )
}

/// Whether the process has opted into a shared/meta `ZEROSTACK_STORE_ROOT`.
pub fn shared_store_opt_in_from_env() -> bool {
    for name in SHARED_STORE_OPT_IN_ENVS {
        if let Some(v) = env::var_os(name) {
            if env_truthy(&v) {
                return true;
            }
        }
    }
    false
}

fn first_env(names: &[&str]) -> Option<OsString> {
    names.iter().find_map(env::var_os)
}

/// Pure store-root selection (wqw.2). See crate docs / `docs/core.md`.
pub fn resolve_store_root_with_env(
    repo_root: &Path,
    store_root_pin: Option<&OsStr>,
    shared_opt_in: bool,
) -> Option<PathBuf> {
    let local = repo_root.join(".zerostack");
    if local.is_dir() {
        return Some(local);
    }
    if !shared_opt_in {
        return None;
    }
    let pin = store_root_pin.filter(|v| !v.is_empty())?;
    let path = PathBuf::from(pin);
    Some(if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    })
}

fn zerostack_store_or_detect(repo_root: &Path) -> Option<PathBuf> {
    resolve_store_root_with_env(
        repo_root,
        first_env(STORE_ROOT_ENVS).as_deref(),
        shared_store_opt_in_from_env(),
    )
}

fn resolve_default_cache_path(
    repo_root: &Path,
    unified_relative: &str,
    legacy_relative: &str,
) -> PathBuf {
    let legacy = repo_root.join(legacy_relative);
    if let Some(store) = zerostack_store_or_detect(repo_root) {
        let unified = store.join(unified_relative);
        if unified.exists() || !legacy.exists() {
            unified
        } else {
            legacy
        }
    } else {
        legacy
    }
}

/// Default recovery cache when --cache-path is omitted.
///
/// After wqw.8 this is the single shared store for CLI expand and CodeMode.
pub fn default_recovery_cache_path(repo_root: &Path) -> PathBuf {
    resolve_default_cache_path(
        repo_root,
        "tokenzero/recovery-cache.json",
        ".tokenzero/recovery-cache.json",
    )
}

/// Honor explicit --cache-path, then TOKENZERO_CACHE_PATH, then the default cache.
pub fn resolve_recovery_cache_path(repo_root: &Path, explicit: Option<PathBuf>) -> PathBuf {
    resolve_recovery_cache_path_with_env(repo_root, explicit, env::var_os("TOKENZERO_CACHE_PATH"))
}

pub fn resolve_recovery_cache_path_with_env(
    repo_root: &Path,
    explicit: Option<PathBuf>,
    env_value: Option<OsString>,
) -> PathBuf {
    explicit
        .or_else(|| {
            env_value
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| default_recovery_cache_path(repo_root))
}

/// Whether `store` is under `root` (same project).
pub fn store_is_under_project_root(store: &Path, root: &Path) -> bool {
    let store_cmp = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let root_cmp = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    store_cmp.starts_with(&root_cmp)
}

/// Doctor / status snapshot of effective store resolution for a root.
#[derive(Debug, Clone)]
pub struct StoreResolutionReport {
    pub effective_cache_path: PathBuf,
    pub effective_store_root: Option<PathBuf>,
    pub shared_store_opt_in: bool,
    pub global_pin_set: bool,
    pub global_pin_value: Option<PathBuf>,
    pub isolation_mode: &'static str,
    /// True when effective store is not under the project root (shared meta).
    pub store_project_mismatch: bool,
    pub mismatch_summary: Option<String>,
}

/// Pure resolution report for tests and doctor.
pub fn store_resolution_report_with_env(
    repo_root: &Path,
    explicit_cache: Option<PathBuf>,
    tokenzero_cache_path: Option<OsString>,
    store_root_pin: Option<OsString>,
    shared_opt_in: bool,
) -> StoreResolutionReport {
    let global_pin_set = store_root_pin.as_ref().is_some_and(|v| !v.is_empty());
    let global_pin_value = store_root_pin
        .as_ref()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    let store = resolve_store_root_with_env(repo_root, store_root_pin.as_deref(), shared_opt_in);
    let had_explicit =
        explicit_cache.is_some() || tokenzero_cache_path.as_ref().is_some_and(|v| !v.is_empty());
    let effective_cache_path =
        resolve_recovery_cache_path_with_env(repo_root, explicit_cache, tokenzero_cache_path);
    let effective_store_root = store.clone();
    let isolation_mode = if had_explicit {
        "explicit_cache"
    } else if shared_opt_in
        && global_pin_set
        && store
            .as_ref()
            .is_some_and(|s| !store_is_under_project_root(s, repo_root))
    {
        "shared_opt_in"
    } else {
        "per_root"
    };

    let store_project_mismatch = match &store {
        Some(s) if shared_opt_in && global_pin_set => !store_is_under_project_root(s, repo_root),
        _ => false,
    };
    let mismatch_summary = if store_project_mismatch {
        Some(format!(
            "effective store {} is outside project root {} (shared store opt-in active; TOKENZERO_SHARED_STORE / ZEROSTACK_SHARED_STORE). Unrelated projects sharing this store will collate recovery caches.",
            store
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            repo_root.display()
        ))
    } else if global_pin_set && !shared_opt_in {
        Some(format!(
            "ZEROSTACK_STORE_ROOT is set but ignored for isolation (wqw.2). Default store is under project root {}. Set TOKENZERO_SHARED_STORE=1 (or ZEROSTACK_SHARED_STORE=1) to opt into the shared/meta store.",
            repo_root.display()
        ))
    } else {
        None
    };

    StoreResolutionReport {
        effective_cache_path,
        effective_store_root,
        shared_store_opt_in: shared_opt_in,
        global_pin_set,
        global_pin_value,
        isolation_mode,
        store_project_mismatch,
        mismatch_summary,
    }
}

/// Live-env doctor snapshot for a project root.
pub fn store_resolution_report(
    repo_root: &Path,
    explicit_cache: Option<PathBuf>,
) -> StoreResolutionReport {
    store_resolution_report_with_env(
        repo_root,
        explicit_cache,
        env::var_os("TOKENZERO_CACHE_PATH"),
        first_env(STORE_ROOT_ENVS),
        shared_store_opt_in_from_env(),
    )
}

/// JSON fragment for doctor / status surfaces.
pub fn store_resolution_json(
    repo_root: &Path,
    explicit_cache: Option<PathBuf>,
) -> serde_json::Value {
    let r = store_resolution_report(repo_root, explicit_cache);
    serde_json::json!({
        "schema_version": "tokenzero.store_resolution.v1",
        "effective_cache_path": r.effective_cache_path.display().to_string(),
        "effective_store_root": r.effective_store_root.as_ref().map(|p| p.display().to_string()),
        "shared_store_opt_in": r.shared_store_opt_in,
        "global_pin_set": r.global_pin_set,
        "global_pin_value": r.global_pin_value.as_ref().map(|p| p.display().to_string()),
        "isolation_mode": r.isolation_mode,
        "store_project_mismatch": r.store_project_mismatch,
        "mismatch_summary": r.mismatch_summary,
        "algorithm": "1) repo_root/.zerostack if present; 2) else ZEROSTACK_STORE_ROOT only when TOKENZERO_SHARED_STORE/ZEROSTACK_SHARED_STORE opt-in; 3) else legacy repo_root/.tokenzero/. Explicit --cache-path / TOKENZERO_CACHE_PATH always win.",
        "opt_in_envs": SHARED_STORE_OPT_IN_ENVS,
        "store_root_envs": STORE_ROOT_ENVS,
    })
}
