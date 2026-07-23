use fs4::FileExt;
use serde_json::{Value, json};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const AUTO_MAINTENANCE_COALESCE: Duration = Duration::from_secs(30);
const GC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const JOURNAL_MAX_AGE: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const JOURNAL_MAX_COUNT: usize = 500;
const DEFAULT_BLOB_BUDGET: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_BLOB_MAX_AGE_SECONDS: u64 = 30 * 24 * 60 * 60;
const DEFAULT_GC_MIN_AGE_SECONDS: u64 = 7 * 24 * 60 * 60;

fn auto_maintenance_state() -> &'static Mutex<Option<(PathBuf, Instant)>> {
    static STATE: OnceLock<Mutex<Option<(PathBuf, Instant)>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

pub fn shell_spill_dir(cache_path: &Path) -> PathBuf {
    cache_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("shell-spills")
}

fn engine_store_dir(cache_path: &Path) -> &Path {
    cache_path.parent().unwrap_or_else(|| Path::new("."))
}

fn marker_fresh(path: &Path, interval: Duration) -> bool {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .and_then(|modified| modified.elapsed().map_err(io::Error::other))
        .is_ok_and(|age| age < interval)
}

fn atomic_touch(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("maintenance"),
        std::process::id()
    ));
    match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
    {
        Ok(file) => {
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            fs::remove_file(&temporary)?;
            atomic_touch(path)
        }
        Err(error) => Err(error),
    }
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn prune_plan_journals_at(
    cache_path: &Path,
    dry_run: bool,
    now: SystemTime,
    max_age: Duration,
    max_count: usize,
) -> Value {
    let root = engine_store_dir(cache_path).join("plan-journals");
    let lock_path = root.join("mutation.lock");
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
    {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return json!({"removed": 0, "scanned": 0});
        }
        Err(error) => return json!({"error": error.to_string()}),
    };
    if FileExt::try_lock(&lock).is_err() {
        return json!({"skipped": "mutation_locked"});
    }
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return json!({"removed": 0, "scanned": 0});
        }
        Err(error) => return json!({"error": error.to_string()}),
    };
    let mut journals = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if let Ok(metadata) = entry.metadata()
            && metadata.is_file()
        {
            journals.push((metadata.modified().unwrap_or(UNIX_EPOCH), path));
        }
    }
    journals.sort_by(|left, right| right.0.cmp(&left.0));
    let scanned = journals.len();
    let mut removed = 0_usize;
    for (index, (modified, path)) in journals.into_iter().enumerate() {
        let age = now.duration_since(modified).unwrap_or_default();
        if index < max_count || age <= max_age {
            continue;
        }
        if dry_run || fs::remove_file(path).is_ok() {
            removed += 1;
        }
    }
    json!({"removed": removed, "scanned": scanned})
}

fn prune_plan_journals(cache_path: &Path, dry_run: bool) -> Value {
    prune_plan_journals_at(
        cache_path,
        dry_run,
        SystemTime::now(),
        JOURNAL_MAX_AGE,
        JOURNAL_MAX_COUNT,
    )
}

fn gc_maintenance(cache_path: &Path, dry_run: bool) -> Value {
    let marker = engine_store_dir(cache_path).join("gc.last");
    if marker_fresh(&marker, GC_INTERVAL) {
        return json!({"skipped": "recent"});
    }
    if dry_run {
        return json!({"would_run": true});
    }
    let now = SystemTime::now();
    let now_ms = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let segment = if tokenzero_recovery::segment_store::SegmentStore::exists(cache_path) {
        let cas = tokenzero_recovery::shared_cas::SharedCas::detect_from_cache_path(cache_path);
        match tokenzero_recovery::segment_store::SegmentStore::open(cache_path.to_path_buf(), cas)
            .and_then(|mut store| store.evict_expired(now_ms))
        {
            Ok(evicted) => json!({"evicted_segments": evicted}),
            Err(error) => json!({"error": error.to_string()}),
        }
    } else {
        json!({"skipped": "segment_store_absent"})
    };
    let shared_cas = if let Some(root) =
        tokenzero_recovery::shared_cas::SharedCas::resolve_cache_root(cache_path)
    {
        let grace_seconds = env_u64(
            "TOKENZERO_GC_GRACE_SECONDS",
            tokenzero_recovery::shared_cas::GC_MIN_GRACE_SECONDS,
        )
        .max(tokenzero_recovery::shared_cas::GC_MIN_GRACE_SECONDS);
        let config = tokenzero_recovery::shared_cas::GcConfig {
            run_id: format!("startup-{}-{now_ms}", std::process::id()),
            grace_seconds,
            min_age_seconds: env_u64("TOKENZERO_GC_MIN_AGE_SECONDS", DEFAULT_GC_MIN_AGE_SECONDS),
            apply: true,
            now,
            fault_after_deletes: None,
        };
        match tokenzero_recovery::shared_cas::run_gc(&root, &config) {
            Ok(report) => json!({"evaluated": report.objects.len()}),
            Err(error) => json!({"error": error.to_string()}),
        }
    } else {
        json!({"skipped": "shared_cas_absent"})
    };
    let marker_result = atomic_touch(&marker).err().map(|error| error.to_string());
    json!({
        "segment": segment,
        "shared_cas": shared_cas,
        "marker_error": marker_result,
    })
}

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
    let plan_journals = prune_plan_journals(cache_path, dry_run);
    let blob_budget = env_u64("TOKENZERO_RECOVERY_BLOB_MAX_BYTES", DEFAULT_BLOB_BUDGET);
    let blob_max_age = Duration::from_secs(env_u64(
        "TOKENZERO_RECOVERY_BLOB_MAX_AGE_SECONDS",
        DEFAULT_BLOB_MAX_AGE_SECONDS,
    ));
    let blob_prune =
        tokenzero_recovery::prune_recovery_blobs(cache_path, blob_budget, blob_max_age, dry_run)
            .map_or_else(
                |error| json!({"error": error.to_string()}),
                |report| json!(report),
            );
    let gc = gc_maintenance(cache_path, dry_run);
    let freed_bytes = blob_prune
        .get("freed_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    json!({
        "freed_bytes": freed_bytes,
        "tmp_sweep": tmp_sweep,
        "spill_prune": spill_prune,
        "plan_journals": plan_journals,
        "blob_prune": blob_prune,
        "gc": gc,
        "telemetry": {"owner": "hub", "action": "skipped"},
    })
}

pub fn cache_maintenance_coalesced(cache_path: &Path, dry_run: bool) -> Value {
    let Ok(mut guard) = auto_maintenance_state().lock() else {
        return json!({"coalesced": true, "skipped": "lock_poisoned"});
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
    let marker = engine_store_dir(cache_path).join("maintenance.last");
    if !dry_run && marker_fresh(&marker, AUTO_MAINTENANCE_COALESCE) {
        return json!({"coalesced": true, "skipped": "recent_cross_process"});
    }
    let lock_path = engine_store_dir(cache_path).join("maintenance.lock");
    let lock = match OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
    {
        Ok(lock) => lock,
        Err(error) => return json!({"coalesced": true, "error": error.to_string()}),
    };
    if FileExt::try_lock(&lock).is_err() {
        return json!({"coalesced": true, "skipped": "cross_process_locked"});
    }
    if !dry_run && marker_fresh(&marker, AUTO_MAINTENANCE_COALESCE) {
        return json!({"coalesced": true, "skipped": "recent_cross_process"});
    }
    let report = cache_maintenance(cache_path, dry_run);
    if !dry_run {
        let _ = atomic_touch(&marker);
    }
    *guard = Some((cache_path.to_path_buf(), Instant::now()));
    report
}

pub fn session_pack(cache_path: &Path, max_tokens: usize) -> Option<String> {
    crate::recall::build_session_pack(cache_path, max_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn gc_marker_throttles_second_run() {
        let directory = tempdir().unwrap();
        let engine = directory.path().join("tokenzero");
        fs::create_dir_all(&engine).unwrap();
        let cache = engine.join("recovery-cache.json");
        let marker = engine.join("gc.last");
        atomic_touch(&marker).unwrap();

        assert_eq!(gc_maintenance(&cache, false)["skipped"], "recent");
    }

    #[test]
    fn journal_pruning_keeps_newest_count_and_recent_files() {
        let directory = tempdir().unwrap();
        let engine = directory.path().join("tokenzero");
        let journals = engine.join("plan-journals");
        fs::create_dir_all(&journals).unwrap();
        for index in 0..4 {
            fs::write(journals.join(format!("{index}.json")), b"{}").unwrap();
        }
        let report = prune_plan_journals_at(
            &engine.join("recovery-cache.json"),
            false,
            SystemTime::now() + Duration::from_secs(1),
            Duration::ZERO,
            2,
        );
        assert_eq!(report["removed"], 2);
        assert_eq!(fs::read_dir(journals).unwrap().count(), 3);
    }
}
