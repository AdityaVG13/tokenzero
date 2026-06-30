//! Workspace root and cache-path resolution shared by CodeMode and MCP.

use std::path::{Path, PathBuf};

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

fn zerostack_store_or_detect(repo_root: &Path) -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("ZEROSTACK_STORE_ROOT")
        .or_else(|| std::env::var_os("ZERO_STACK_STORE_ROOT"))
    {
        let path = PathBuf::from(v);
        return Some(if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| repo_root.to_path_buf())
                .join(path)
        });
    }
    let candidate = repo_root.join(".zerostack");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
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

/// Default recovery cache when `--cache-path` is omitted.
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
