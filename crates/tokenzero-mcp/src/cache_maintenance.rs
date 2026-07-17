use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Coalesce window for automatic engine-construction maintenance.
const AUTO_MAINTENANCE_COALESCE: Duration = Duration::from_secs(30);

fn auto_maintenance_state() -> &'static Mutex<Option<(PathBuf, Instant)>> {
    static STATE: OnceLock<Mutex<Option<(PathBuf, Instant)>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
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

/// Automatic construction-time maintenance: serialize per process and skip
/// when the same cache path was cleaned within [`AUTO_MAINTENANCE_COALESCE`].
///
/// Prevents concurrent `TokenZeroEngine::new` calls from multiplying spill-dir
/// scan/sort work against the same directory.
pub fn cache_maintenance_coalesced(cache_path: &Path, dry_run: bool) -> Value {
    let Ok(mut guard) = auto_maintenance_state().lock() else {
        return json!({
            "coalesced": true,
            "skipped": "lock_poisoned",
        });
    };
    if let Some((prev_path, at)) = guard.as_ref()
        && prev_path == cache_path
        && at.elapsed() < AUTO_MAINTENANCE_COALESCE
    {
        return json!({
            "coalesced": true,
            "skipped": "recent",
            "cache_path": cache_path.display().to_string(),
        });
    }
    let report = cache_maintenance(cache_path, dry_run);
    *guard = Some((cache_path.to_path_buf(), Instant::now()));
    report
}

/// Build the post-compaction session pack over a workspace's recovery
/// cache: the most recently served payloads with exact refs, token-budgeted.
/// `None` when there is nothing to restore.
pub fn session_pack(cache_path: &Path, max_tokens: usize) -> Option<String> {
    crate::recall::build_session_pack(cache_path, max_tokens)
}
