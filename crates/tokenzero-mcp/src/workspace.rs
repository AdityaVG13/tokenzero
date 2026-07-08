//! Workspace root and cache-path resolution shared by CodeMode and MCP.
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
    names.iter().find_map(|name| env::var_os(name))
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
#[allow(dead_code)]
pub fn default_recovery_cache_path(repo_root: &Path) -> PathBuf {
    resolve_default_cache_path(
        repo_root,
        "tokenzero/recovery-cache.json",
        ".tokenzero/recovery-cache.json",
    )
}

/// CodeMode compact/expand recovery store (unified or legacy layout).
#[allow(dead_code)] // used via re-export or tests in dependent crates; keep for API symmetry
pub fn default_codemode_recovery_cache_path(repo_root: &Path) -> PathBuf {
    resolve_default_cache_path(
        repo_root,
        "tokenzero/codemode-recovery.json",
        ".tokenzero/codemode-recovery.json",
    )
}

/// Whether `store` is under `root` (same project).
#[allow(dead_code)] // public API for doctor/status callers; mirrored on CLI crate
pub fn store_is_under_project_root(store: &Path, root: &Path) -> bool {
    let store_cmp = store.canonicalize().unwrap_or_else(|_| store.to_path_buf());
    let root_cmp = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    store_cmp.starts_with(&root_cmp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn cwd_dot_zerostack_does_not_contaminate_tempdir_resolution() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Create .zerostack in an unrelated tempdir to confirm resolution
        // uses ONLY the passed repo_root, never cwd or any other directory.
        let unrelated = tempdir().unwrap();
        let _ = fs::create_dir_all(unrelated.path().join(".zerostack/tokenzero"));
        // Resolution MUST derive from the passed repo_root only
        assert_eq!(
            default_recovery_cache_path(root),
            root.join(".tokenzero/recovery-cache.json")
        );
    }

    #[test]
    fn pure_resolve_store_root_ignores_pin_without_opt_in() {
        let proj = tempdir().unwrap();
        let shared = tempdir().unwrap();
        assert!(
            resolve_store_root_with_env(proj.path(), Some(shared.path().as_os_str()), false)
                .is_none()
        );
        assert_eq!(
            resolve_store_root_with_env(proj.path(), Some(shared.path().as_os_str()), true)
                .unwrap(),
            shared.path()
        );
    }

    #[test]
    fn pure_two_roots_isolate_when_pin_present_without_opt_in() {
        let a = tempdir().unwrap();
        let b = tempdir().unwrap();
        let shared = tempdir().unwrap();
        let pin = Some(shared.path().as_os_str());
        assert!(resolve_store_root_with_env(a.path(), pin, false).is_none());
        assert!(resolve_store_root_with_env(b.path(), pin, false).is_none());
        assert_ne!(
            default_recovery_cache_path(a.path()),
            default_recovery_cache_path(b.path())
        );
        assert!(default_recovery_cache_path(a.path()).starts_with(a.path()));
        assert!(default_recovery_cache_path(b.path()).starts_with(b.path()));
    }
}
