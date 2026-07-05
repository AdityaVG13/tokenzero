//! Unified `.zerostack/` store-root resolution for TokenZero cache paths.

use std::env;
use std::ffi::OsString;
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
            repo_root.join(path)
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

/// Default recovery cache when --cache-path is omitted.
pub fn default_recovery_cache_path(repo_root: &Path) -> PathBuf {
    resolve_default_cache_path(
        repo_root,
        "tokenzero/recovery-cache.json",
        ".tokenzero/recovery-cache.json",
    )
}

/// CodeMode compact/expand recovery store (unified or legacy layout).
#[allow(dead_code)]
pub fn default_codemode_recovery_cache_path(repo_root: &Path) -> PathBuf {
    resolve_default_cache_path(
        repo_root,
        "tokenzero/codemode-recovery.json",
        ".tokenzero/codemode-recovery.json",
    )
}

/// Honor explicit --cache-path, then TOKENZERO_CACHE_PATH, then the default cache.
pub fn resolve_recovery_cache_path(repo_root: &Path, explicit: Option<PathBuf>) -> PathBuf {
    resolve_recovery_cache_path_with_env(repo_root, explicit, env::var_os("TOKENZERO_CACHE_PATH"))
}

fn resolve_recovery_cache_path_with_env(
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
    fn unified_recovery_falls_back_to_legacy_when_only_legacy_exists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".zerostack/tokenzero")).unwrap();
        let legacy = root.join(".tokenzero/recovery-cache.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}\n").unwrap();
        assert_eq!(default_recovery_cache_path(root), legacy);
    }

    #[test]
    fn recovery_cache_path_honors_env_between_explicit_and_default() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let explicit = root.join("explicit.json");
        let env_path = root.join("env.json");
        assert_eq!(
            resolve_recovery_cache_path_with_env(
                root,
                Some(explicit.clone()),
                Some(env_path.clone().into_os_string()),
            ),
            explicit
        );
        assert_eq!(
            resolve_recovery_cache_path_with_env(
                root,
                None,
                Some(env_path.clone().into_os_string())
            ),
            env_path
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

    #[test]
    fn unified_codemode_cache_falls_back_to_legacy_when_only_legacy_exists() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".zerostack")).unwrap();
        let legacy = root.join(".tokenzero/codemode-recovery.json");
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}\n").unwrap();
        assert_eq!(default_codemode_recovery_cache_path(root), legacy);
    }

    #[test]
    fn cwd_dot_zerostack_does_not_contaminate_tempdir_resolution() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        // Create .zerostack in an unrelated tempdir to confirm resolution
        // uses ONLY the passed repo_root, never cwd or any other directory.
        let unrelated = tempdir().unwrap();
        let _ = fs::create_dir_all(unrelated.path().join(".zerostack/tokenzero"));
        assert_eq!(
            default_recovery_cache_path(root),
            root.join(".tokenzero/recovery-cache.json")
        );
    }

    #[test]
    fn allowed_roots_dedupes_canonical_workspace_paths() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let alias = dir.path().join(".");
        let roots = allowed_roots_for_workspace(root, &[alias]);
        assert_eq!(roots.len(), 1);
    }
}
