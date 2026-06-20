use serde_json::{Value, json};
use std::path::{Path, PathBuf};

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
    crate::recall::build_session_pack(cache_path, max_tokens)
}
