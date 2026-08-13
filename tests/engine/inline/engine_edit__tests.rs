use super::*;

/// Engine whose recovery store is forced to fail: the store's parent
/// directory is blocked by a regular file, so `persist()` errors at
/// `create_dir_all` before any snapshot or journal write can succeed.
/// (load_state swallows parse errors, so a non-JSON file at the cache
/// path itself would not trip the store.)
fn engine_with_blocked_cache(dir: &std::path::Path) -> TokenZeroEngine {
    fs::write(dir.join("cache-blocker"), b"not a directory").unwrap();
    let mut config = EngineConfig::for_root(dir);
    config.cache_path = dir.join("cache-blocker").join("recovery-cache.json");
    config.session_dedup = false;
    config.diff_reads = false;
    config.fetch_enabled = false;
    TokenZeroEngine::new(config)
}

#[test]
fn edit_fails_closed_when_recovery_persist_fails_and_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target.txt");
    fs::write(&path, "original-line\n").unwrap();
    let engine = engine_with_blocked_cache(dir.path());

    let response = engine.edit(
        &path,
        &[EditHunk {
            find: "original-line".to_string(),
            replace: "mutated-line".to_string(),
            replace_all: false,
        }],
        false,
        false,
        Mode::Auto,
        4000,
    );

    assert_eq!(response.status, "error", "{response:?}");
    let error = response.error.as_ref().expect("typed error");
    assert_eq!(error.code, "edit_recovery_unavailable", "{response:?}");
    assert!(
        error.repair.is_some(),
        "error must carry a one-step-correctable repair: {error:?}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "original-line\n",
        "file must remain byte-exact when undo cannot persist"
    );
}

#[test]
fn create_fails_closed_when_recovery_persist_fails_and_no_file_written() {
    let dir = tempfile::tempdir().unwrap();
    let new_path = dir.path().join("new.txt");
    let engine = engine_with_blocked_cache(dir.path());

    let response = engine.edit(
        &new_path,
        &[EditHunk {
            find: String::new(),
            replace: "brand-new\n".to_string(),
            replace_all: false,
        }],
        true,
        false,
        Mode::Auto,
        4000,
    );

    assert_eq!(response.status, "error", "{response:?}");
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("edit_recovery_unavailable"),
        "{response:?}"
    );
    assert!(
        !new_path.exists(),
        "create must not write a file when undo cannot persist"
    );
}

#[test]
fn dry_run_stays_non_mutating_and_reports_degraded_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("target.txt");
    fs::write(&path, "original-line\n").unwrap();
    let engine = engine_with_blocked_cache(dir.path());

    let response = engine.edit(
        &path,
        &[EditHunk {
            find: "original-line".to_string(),
            replace: "mutated-line".to_string(),
            replace_all: false,
        }],
        false,
        true,
        Mode::Auto,
        4000,
    );

    assert_eq!(response.status, "ok", "{response:?}");
    assert!(
        response.diagnostic.is_some(),
        "dry-run must report degraded recovery truthfully: {response:?}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "original-line\n",
        "dry-run must never mutate"
    );
}
