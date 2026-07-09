//! CLI re-exports of the canonical store resolver in `tokenzero_mcp::workspace`.
//!
//! Keep this module as a thin facade so doctor/CLI call sites stay stable while
//! MCP and CodeMode share one implementation (wqw.2 / wqw.8).

pub use tokenzero_mcp::{
    allowed_roots_for_workspace, default_allowed_roots, default_recovery_cache_path,
    resolve_recovery_cache_path, resolve_recovery_cache_path_with_env, resolve_store_root_with_env,
    store_resolution_json, store_resolution_report, store_resolution_report_with_env,
    tokenzero_work_root,
};

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
    fn cwd_dot_zerostack_does_not_contaminate_tempdir_resolution() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let unrelated = tempdir().unwrap();
        let _ = fs::create_dir_all(unrelated.path().join(".zerostack/tokenzero"));
        assert_eq!(
            default_recovery_cache_path(root),
            root.join(".tokenzero/recovery-cache.json")
        );
    }

    #[test]
    fn pure_resolve_ignores_pin_without_opt_in() {
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
    fn doctor_report_flags_ignored_global_pin() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let pin = root.join("shared-store");
        let report = store_resolution_report_with_env(
            root,
            None,
            None,
            Some(pin.clone().into_os_string()),
            false,
        );
        assert!(!report.shared_store_opt_in);
        assert!(report.global_pin_set);
        assert_eq!(report.isolation_mode, "per_root");
        assert!(
            report
                .mismatch_summary
                .as_ref()
                .is_some_and(|s| s.contains("ignored for isolation"))
        );
    }

    #[test]
    fn doctor_report_shared_opt_in_outside_project() {
        let proj = tempdir().unwrap();
        let shared = tempdir().unwrap();
        let report = store_resolution_report_with_env(
            proj.path(),
            None,
            None,
            Some(shared.path().as_os_str().to_os_string()),
            true,
        );
        assert!(report.shared_store_opt_in);
        assert!(report.store_project_mismatch);
        assert_eq!(report.isolation_mode, "shared_opt_in");
        assert!(
            report
                .mismatch_summary
                .as_ref()
                .is_some_and(|s| s.contains("outside project root"))
        );
    }
}
