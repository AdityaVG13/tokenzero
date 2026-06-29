//! Unified `.zerostack/` store-root resolution for TokenZero cache paths.

use std::path::{Path, PathBuf};

/// Workspace root for TokenZero persistence (matches CLI `root_from` / CodeMode).
pub fn tokenzero_work_root(explicit_root: Option<PathBuf>) -> PathBuf {
    explicit_root
        .or_else(|| std::env::var_os("TOKENZERO_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
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

/// Default recovery cache when `--cache-path` is omitted.
pub fn default_recovery_cache_path(repo_root: &Path) -> PathBuf {
    if let Some(store) = zerostack_store_or_detect(repo_root) {
        store.join("tokenzero/recovery-cache.json")
    } else {
        repo_root.join(".tokenzero/recovery-cache.json")
    }
}

/// CodeMode compact/expand recovery store (unified or legacy layout).
#[allow(dead_code)]
pub fn default_codemode_recovery_cache_path(repo_root: &Path) -> PathBuf {
    if let Some(store) = zerostack_store_or_detect(repo_root) {
        store.join("tokenzero/codemode-recovery.json")
    } else {
        repo_root.join(".tokenzero/codemode-recovery.json")
    }
}

/// Honor explicit `--cache-path`; otherwise apply unified-root or legacy default.
pub fn resolve_recovery_cache_path(repo_root: &Path, explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| default_recovery_cache_path(repo_root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn legacy_recovery_default_without_unified_root() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        assert_eq!(
            default_recovery_cache_path(root),
            root.join(".tokenzero/recovery-cache.json")
        );
    }

    #[test]
    fn unified_recovery_default_when_dot_zerostack_exists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();
        assert_eq!(
            default_recovery_cache_path(root),
            root.join(".zerostack/tokenzero/recovery-cache.json")
        );
    }

    #[test]
    fn unified_codemode_cache_under_tokenzero_subdir() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".zerostack")).unwrap();
        assert_eq!(
            default_codemode_recovery_cache_path(root),
            root.join(".zerostack/tokenzero/codemode-recovery.json")
        );
    }
}
