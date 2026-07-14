#![forbid(unsafe_code)]

use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use tokenzero_core::{PULSE_SCHEMA_VERSION, savings_ratio};

const EVENT_SQL_COLUMNS: &str = "schema_version, event, timestamp_unix, tool, mode, raw_tokens, visible_tokens, recovery_tokens, task_lossless, cache_hit, retry_count, failure, exact_ref_count, latency_ms, source_hash, session_id, call_id, ref_ids";
const PULSE_SOURCE_OF_TRUTH: &str = "jsonl";
const PULSE_SYNC_SCHEMA_VERSION: &str = "pulse-sync-v1";
const PULSE_EVENT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const PULSE_SYNC_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseEvent {
    pub schema_version: String,
    pub event: String,
    pub timestamp_unix: u64,
    pub tool: String,
    pub mode: String,
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub recovery_tokens: usize,
    pub task_lossless: bool,
    pub cache_hit: bool,
    pub retry_count: usize,
    pub failure: bool,
    pub exact_ref_count: usize,
    pub latency_ms: u128,
    pub source_hash: Option<String>,
    /// Serving session id for expand-time attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Call id within the session (e.g. JSON-RPC id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// Serve/expand tz:// refs — RACC join key.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ref_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseReport {
    pub schema_version: String,
    pub status: String,
    pub event_count: usize,
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub recovery_tokens: usize,
    pub task_lossless_tokens: usize,
    pub failures: usize,
    pub cache_hits: usize,
    pub exact_ref_count: usize,
    pub visible_savings: f64,
    pub recovery_adjusted_savings: f64,
    /// Corrupt/non-empty unparsable ledger lines.
    #[serde(default)]
    pub skipped_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseSyncMeta {
    pub schema_version: String,
    pub source_of_truth: String,
    pub ledger_sha256: String,
    pub event_count: usize,
    pub skipped_lines: usize,
    pub updated_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseSyncStatus {
    pub ok: bool,
    pub source_of_truth: String,
    pub ledger_path: PathBuf,
    pub sqlite_path: PathBuf,
    pub meta_path: PathBuf,
    pub event_count: usize,
    pub skipped_lines: usize,
    pub ledger_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseDoctorReport {
    pub ok: bool,
    pub source_of_truth: String,
    pub ledger_path: PathBuf,
    pub sqlite_path: PathBuf,
    pub meta_path: PathBuf,
    pub event_count: usize,
    pub skipped_lines: usize,
    pub ledger_sha256: String,
    pub sqlite_integrity: String,
    pub marker_match: bool,
    pub hot_index_used: bool,
}

impl PulseEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn tool_call(
        tool: &str,
        mode: &str,
        raw_tokens: usize,
        visible_tokens: usize,
        recovery_tokens: usize,
        exact_ref_count: usize,
        latency_ms: u128,
        source_hint: Option<&str>,
    ) -> Self {
        Self {
            schema_version: PULSE_SCHEMA_VERSION.to_string(),
            event: "tool_call".to_string(),
            timestamp_unix: now_unix(),
            tool: tool.to_string(),
            mode: mode.to_string(),
            raw_tokens,
            visible_tokens,
            recovery_tokens,
            task_lossless: true,
            cache_hit: false,
            retry_count: 0,
            failure: false,
            exact_ref_count,
            latency_ms,
            source_hash: source_hint.map(hash_hint),
            session_id: None,
            call_id: None,
            ref_ids: Vec::new(),
        }
    }

    pub fn with_attribution(
        mut self,
        session_id: Option<String>,
        call_id: Option<String>,
        ref_ids: Vec<String>,
    ) -> Self {
        self.session_id = session_id;
        self.call_id = call_id;
        self.ref_ids = ref_ids;
        self
    }
}

pub fn default_ledger_path(root: &Path) -> PathBuf {
    root.join(".tokenzero/pulse/events.jsonl")
}

pub fn record_event(path: &Path, event: &PulseEvent) -> std::io::Result<()> {
    let _lock = acquire_pulse_lock_wait(path, PULSE_EVENT_LOCK_TIMEOUT)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file_existed = path.exists();
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    // One logical JSONL record per append; no fsync (telemetry, crash-loss ok).
    let mut line = serde_json::to_string(event).map_err(io_other)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    if !file_existed {
        sync_parent(path)?;
    }
    Ok(())
}

pub fn sync_jsonl_to_sqlite(path: &Path) -> std::io::Result<PulseSyncStatus> {
    let _lock = acquire_pulse_lock_wait(path, PULSE_SYNC_LOCK_TIMEOUT)?;
    sync_jsonl_to_sqlite_locked(path)
}

pub fn export_jsonl(path: &Path, output: &Path) -> std::io::Result<PulseSyncStatus> {
    let _lock = acquire_pulse_lock_wait(path, PULSE_SYNC_LOCK_TIMEOUT)?;
    let status = sync_jsonl_to_sqlite_locked(path)?;
    atomic_export_sqlite_jsonl(&status.sqlite_path, output)?;
    let output_scan = scan_jsonl(output, |_| Ok(()))?;
    let meta = meta_from_scan(&output_scan);
    write_sidecar_meta(&export_meta_path(output), &meta)?;
    Ok(status)
}

pub fn import_jsonl(input: &Path, path: &Path) -> std::io::Result<PulseSyncStatus> {
    let _lock = acquire_pulse_lock_wait(path, PULSE_SYNC_LOCK_TIMEOUT)?;
    let input_source = ensure_import_not_older(input, path)?;
    atomic_import_valid_jsonl(input, path, &input_source.scan)?;
    sync_jsonl_to_sqlite_locked(path)
}

pub fn doctor_jsonl_sqlite(path: &Path) -> std::io::Result<PulseDoctorReport> {
    let status = sync_jsonl_to_sqlite(path)?;
    let conn = open_sqlite(&status.sqlite_path)?;
    let sqlite_integrity = sqlite_integrity_check(&conn)?;
    let sqlite_meta = read_sqlite_meta(&conn)?;
    let sidecar_meta = read_sidecar_meta(&status.meta_path)?;
    let marker_match = sqlite_meta.ledger_sha256 == status.ledger_sha256
        && sidecar_meta.ledger_sha256 == status.ledger_sha256
        && sqlite_meta.event_count == status.event_count
        && sidecar_meta.event_count == status.event_count;
    let hot_index_used = hot_index_is_used(&conn)?;
    Ok(PulseDoctorReport {
        ok: status.ok && sqlite_integrity == "ok" && marker_match && hot_index_used,
        source_of_truth: status.source_of_truth, ledger_path: status.ledger_path,
        sqlite_path: status.sqlite_path, meta_path: status.meta_path,
        event_count: status.event_count, skipped_lines: status.skipped_lines,
        ledger_sha256: status.ledger_sha256, sqlite_integrity, marker_match, hot_index_used,
    })
}

pub fn report_for_path(path: &Path) -> std::io::Result<PulseReport> {
    aggregate_for_path(path)
}

pub fn render_text(report: &PulseReport) -> String {
    let mut out = format!(
        "pulse ok: events={} visible_savings={:.2}% recovery_adjusted_savings={:.2}% failures={}\n",
        report.event_count,
        report.visible_savings * 100.0,
        report.recovery_adjusted_savings * 100.0,
        report.failures
    );
    if report.skipped_lines > 0 {
        out.push_str(&format!(
            "pulse warning: skipped {} corrupt ledger line(s)\n",
            report.skipped_lines
        ));
    }
    out
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sync_status_from_scan(
    path: &Path,
    sqlite_path: PathBuf,
    meta_path: PathBuf,
    scan: JsonlScan,
) -> PulseSyncStatus {
    PulseSyncStatus {
        ok: scan.skipped_lines == 0,
        source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
        ledger_path: path.to_path_buf(),
        sqlite_path,
        meta_path,
        event_count: scan.event_count,
        skipped_lines: scan.skipped_lines,
        ledger_sha256: scan.ledger_sha256,
    }
}

fn sync_jsonl_to_sqlite_locked(path: &Path) -> std::io::Result<PulseSyncStatus> {
    let sqlite_path = sqlite_path_for_ledger(path);
    let meta_path = meta_path_for_ledger(path);
    for p in [path.parent(), sqlite_path.parent()].into_iter().flatten() {
        fs::create_dir_all(p)?;
    }

    let scan = sync_jsonl_into_sqlite_cache(path, &sqlite_path)?;

    let meta = meta_from_scan(&scan);
    write_sidecar_meta(&meta_path, &meta)?;

    Ok(sync_status_from_scan(path, sqlite_path, meta_path, scan))
}

fn open_sqlite(path: &Path) -> std::io::Result<Connection> {
    let conn = Connection::open(path).map_err(sqlite_error)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(sqlite_error)?;
    for (key, val) in [
        ("journal_mode", "WAL"),
        ("synchronous", "NORMAL"),
        ("fullfsync", "ON"),
        ("wal_autocheckpoint", "1000"),
        ("foreign_keys", "ON"),
    ] {
        conn.pragma_update(None, key, val).map_err(sqlite_error)?;
    }
    Ok(conn)
}

fn sync_jsonl_into_sqlite_cache(
    ledger_path: &Path,
    sqlite_path: &Path,
) -> std::io::Result<JsonlScan> {
    let scan = scan_jsonl(ledger_path, |_| Ok(()))?;
    let mut conn = open_or_rebuild_sqlite(sqlite_path)?;
    // Marker-equality fast path: skip DELETE+rebuild when meta matches scan.
    if let Ok(sqlite_meta) = read_sqlite_meta(&conn) {
        let meta_path = meta_path_for_ledger(ledger_path);
        if meta_matches_scan(&sqlite_meta, &scan)
            && read_sidecar_meta(&meta_path)
                .map(|meta| meta_matches_scan(&meta, &scan))
                .unwrap_or(false)
        {
            return Ok(scan);
        }
    }
    match write_sqlite_events_from_jsonl(&mut conn, ledger_path) {
        Ok(scan) => Ok(scan),
        Err(err) if sqlite_cache_can_rebuild(&err) => {
            drop(conn);
            remove_sqlite_cache_files(sqlite_path)?;
            let mut conn = open_sqlite(sqlite_path)?;
            init_sqlite(&conn)?;
            write_sqlite_events_from_jsonl(&mut conn, ledger_path)
        }
        Err(err) => Err(err),
    }
}

fn open_or_rebuild_sqlite(path: &Path) -> std::io::Result<Connection> {
    match open_sqlite(path).and_then(|conn| {
        init_sqlite(&conn)?;
        Ok(conn)
    }) {
        Ok(conn) => Ok(conn),
        Err(err) if sqlite_cache_can_rebuild(&err) => {
            remove_sqlite_cache_files(path)?;
            let conn = open_sqlite(path)?;
            init_sqlite(&conn)?;
            Ok(conn)
        }
        Err(err) => Err(err),
    }
}

fn sqlite_cache_can_rebuild(err: &std::io::Error) -> bool {
    if err.kind() != std::io::ErrorKind::InvalidData {
        return false;
    }
    let message = err.to_string();
    [
        "file is not a database",
        "database disk image is malformed",
        "not a database",
        "has no column named",
        "no such column",
        "no such table",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn remove_sqlite_cache_files(path: &Path) -> std::io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        if let Err(err) = fs::remove_file(sqlite_sidecar_path(path, suffix)) {
            if err.kind() != std::io::ErrorKind::NotFound {
                return Err(err);
            }
        }
    }
    Ok(())
}

/// JSON-encode ref ids for the sqlite sidecar; NULL when empty.
fn ref_ids_to_column(ref_ids: &[String]) -> std::io::Result<Option<String>> {
    if ref_ids.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(ref_ids).map(Some).map_err(io_other)
    }
}

fn ref_ids_from_column(column: Option<String>) -> Vec<String> {
    column
        .as_deref()
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or_default()
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut target = path.as_os_str().to_os_string();
    target.push(suffix);
    PathBuf::from(target)
}

fn init_sqlite(conn: &Connection) -> std::io::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS events (
            line_no INTEGER PRIMARY KEY,
            schema_version TEXT NOT NULL, event TEXT NOT NULL, timestamp_unix INTEGER NOT NULL,
            tool TEXT NOT NULL, mode TEXT NOT NULL,
            raw_tokens INTEGER NOT NULL, visible_tokens INTEGER NOT NULL, recovery_tokens INTEGER NOT NULL,
            task_lossless INTEGER NOT NULL, cache_hit INTEGER NOT NULL, retry_count INTEGER NOT NULL,
            failure INTEGER NOT NULL, exact_ref_count INTEGER NOT NULL, latency_ms INTEGER NOT NULL,
            source_hash TEXT, session_id TEXT, call_id TEXT, ref_ids TEXT, record_hash TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS idx_events_tool_time ON events(tool, timestamp_unix DESC);
        CREATE INDEX IF NOT EXISTS idx_events_event_time ON events(event, timestamp_unix DESC);",
    )
    .map_err(sqlite_error)?;
    for ddl in [
        "ALTER TABLE events ADD COLUMN session_id TEXT",
        "ALTER TABLE events ADD COLUMN call_id TEXT",
        "ALTER TABLE events ADD COLUMN ref_ids TEXT",
    ] {
        let _ = conn.execute(ddl, []);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JsonlScan {
    event_count: usize,
    skipped_lines: usize,
    ledger_sha256: String,
}

fn write_sqlite_events_from_jsonl(
    conn: &mut Connection,
    path: &Path,
) -> std::io::Result<JsonlScan> {
    let tx = conn.transaction().map_err(sqlite_error)?;
    tx.execute("DELETE FROM events", []).map_err(sqlite_error)?;
    let scan = {
        let mut stmt = tx.prepare(
            "INSERT INTO events (line_no, schema_version, event, timestamp_unix, tool, mode, raw_tokens, visible_tokens, recovery_tokens, task_lossless, cache_hit, retry_count, failure, exact_ref_count, latency_ms, source_hash, session_id, call_id, ref_ids, record_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        ).map_err(sqlite_error)?;
        let mut line_no = 0i64;
        scan_jsonl(path, |event| {
            line_no += 1;
            stmt.execute(params![
                line_no, &event.schema_version, &event.event, event.timestamp_unix as i64,
                &event.tool, &event.mode, clamp_i64(event.raw_tokens), clamp_i64(event.visible_tokens),
                clamp_i64(event.recovery_tokens), bool_i64(event.task_lossless), bool_i64(event.cache_hit),
                clamp_i64(event.retry_count), bool_i64(event.failure), clamp_i64(event.exact_ref_count),
                clamp_u128_i64(event.latency_ms), event.source_hash.as_deref(), event.session_id.as_deref(),
                event.call_id.as_deref(), ref_ids_to_column(&event.ref_ids)?, hash_event(event)?,
            ])
            .map_err(sqlite_error)?;
            Ok(())
        })?
    };
    for (k, v) in [
        ("schema_version", PULSE_SYNC_SCHEMA_VERSION.to_string()),
        ("source_of_truth", PULSE_SOURCE_OF_TRUTH.to_string()),
        ("ledger_sha256", scan.ledger_sha256.clone()),
        ("event_count", scan.event_count.to_string()),
        ("skipped_lines", scan.skipped_lines.to_string()),
        ("updated_unix", now_unix().to_string()),
    ] {
        set_meta(&tx, k, &v)?;
    }
    tx.commit().map_err(sqlite_error)?;
    Ok(scan)
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> std::io::Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map_err(sqlite_error)?;
    Ok(())
}

fn read_sqlite_meta(conn: &Connection) -> std::io::Result<PulseSyncMeta> {
    Ok(PulseSyncMeta {
        schema_version: sqlite_meta_value(conn, "schema_version")?,
        source_of_truth: sqlite_meta_value(conn, "source_of_truth")?,
        ledger_sha256: sqlite_meta_value(conn, "ledger_sha256")?,
        event_count: sqlite_meta_value(conn, "event_count")?.parse().unwrap_or(0),
        skipped_lines: sqlite_meta_value(conn, "skipped_lines")?.parse().unwrap_or(0),
        updated_unix: sqlite_meta_value(conn, "updated_unix")?.parse().unwrap_or(0),
    })
}

fn sqlite_meta_value(conn: &Connection, key: &str) -> std::io::Result<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get::<_, String>(0)
    })
    .map_err(sqlite_error)
}

fn sqlite_integrity_check(conn: &Connection) -> std::io::Result<String> {
    conn.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(sqlite_error)
}

fn hot_index_is_used(conn: &Connection) -> std::io::Result<bool> {
    let mut stmt = conn
        .prepare("EXPLAIN QUERY PLAN SELECT line_no FROM events WHERE tool = ?1 ORDER BY timestamp_unix DESC LIMIT 10")
        .map_err(sqlite_error)?;
    for detail in stmt.query_map(["read"], |row| row.get::<_, String>(3)).map_err(sqlite_error)? {
        if detail.map_err(sqlite_error)?.contains("idx_events_tool_time") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_sidecar_meta(path: &Path, meta: &PulseSyncMeta) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(meta).map_err(io_other)?;
    atomic_write(path, &bytes)
}

fn read_sidecar_meta(path: &Path) -> std::io::Result<PulseSyncMeta> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(io_other)
}

struct VerifiedImportSource {
    scan: JsonlScan,
    meta: Option<PulseSyncMeta>,
}

fn import_err(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
}

fn ensure_import_not_older(
    input: &Path,
    current_ledger: &Path,
) -> std::io::Result<VerifiedImportSource> {
    if !fs::metadata(input)?.is_file() {
        return Err(import_err("import source is not a regular file"));
    }
    let input_source = verify_import_source(input)?;
    let current_scan = scan_jsonl(current_ledger, |_| Ok(()))?;
    if input_source.scan.ledger_sha256 == current_scan.ledger_sha256 {
        return Ok(input_source);
    }

    let Some(current_meta) = read_trusted_sidecar_meta(&meta_path_for_ledger(current_ledger))? else {
        if current_scan.event_count == 0 && current_scan.skipped_lines == 0 {
            return Ok(input_source);
        }
        return Err(import_err("current Pulse ledger has no version marker; refusing to overwrite it"));
    };
    let Some(input_meta) = &input_source.meta else {
        return Err(import_err(
            "import snapshot has no version marker; refusing to overwrite the current Pulse ledger",
        ));
    };
    if !meta_matches_scan(&current_meta, &current_scan) {
        if current_scan.skipped_lines > 0 && input_meta.updated_unix > current_meta.updated_unix {
            return Ok(input_source);
        }
        return Err(import_err(
            "current Pulse ledger has unsynced changes; run `tokenzero pulse sync` before importing a different snapshot",
        ));
    }
    if input_meta.updated_unix <= current_meta.updated_unix {
        return Err(import_err("import snapshot is not newer than the current Pulse ledger marker"));
    }
    Ok(input_source)
}

fn verify_import_source(input: &Path) -> std::io::Result<VerifiedImportSource> {
    let scan = scan_jsonl(input, |_| Ok(()))?;
    if scan.skipped_lines > 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "import source contains corrupt JSONL line(s)",
        ));
    }
    let meta = read_trusted_sidecar_meta(&export_meta_path(input))?;
    if let Some(meta) = &meta {
        if !meta_matches_scan(meta, &scan) {
            return Err(import_err("import snapshot marker does not match source JSONL"));
        }
    }
    Ok(VerifiedImportSource { scan, meta })
}

fn read_trusted_sidecar_meta(path: &Path) -> std::io::Result<Option<PulseSyncMeta>> {
    match read_sidecar_meta(path) {
        Ok(meta)
            if meta.schema_version == PULSE_SYNC_SCHEMA_VERSION
                && meta.source_of_truth == PULSE_SOURCE_OF_TRUTH =>
        {
            Ok(Some(meta))
        }
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Pulse marker has an unexpected schema or source at {}", path.display()),
        )),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn meta_from_scan(scan: &JsonlScan) -> PulseSyncMeta {
    PulseSyncMeta {
        schema_version: PULSE_SYNC_SCHEMA_VERSION.to_string(),
        source_of_truth: PULSE_SOURCE_OF_TRUTH.to_string(),
        ledger_sha256: scan.ledger_sha256.clone(),
        event_count: scan.event_count,
        skipped_lines: scan.skipped_lines,
        updated_unix: now_unix(),
    }
}

fn meta_matches_scan(meta: &PulseSyncMeta, scan: &JsonlScan) -> bool {
    meta.ledger_sha256 == scan.ledger_sha256
        && meta.event_count == scan.event_count
        && meta.skipped_lines == scan.skipped_lines
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    persist_temp(file, path)?;
    sync_parent(path)?;
    Ok(())
}

fn persist_temp(file: NamedTempFile, path: &Path) -> std::io::Result<()> {
    file.persist(path).map(|_| ()).map_err(|err| std::io::Error::new(err.error.kind(), err.error))
}

fn create_temp_writer(path: &Path) -> std::io::Result<(NamedTempFile, BufWriter<std::fs::File>)> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file = NamedTempFile::new_in(parent)?;
    let writer = BufWriter::new(file.reopen()?);
    Ok((file, writer))
}

fn finish_temp_writer(
    file: NamedTempFile,
    mut writer: BufWriter<std::fs::File>,
    output: &Path,
) -> std::io::Result<()> {
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    persist_temp(file, output)?;
    sync_parent(output)?;
    Ok(())
}

fn pulse_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PulseEvent> {
    Ok(PulseEvent {
        schema_version: row.get(0)?, event: row.get(1)?,
        timestamp_unix: i64_u64(row.get(2)?), tool: row.get(3)?, mode: row.get(4)?,
        raw_tokens: i64_usize(row.get(5)?), visible_tokens: i64_usize(row.get(6)?),
        recovery_tokens: i64_usize(row.get(7)?), task_lossless: i64_bool(row.get(8)?),
        cache_hit: i64_bool(row.get(9)?), retry_count: i64_usize(row.get(10)?),
        failure: i64_bool(row.get(11)?), exact_ref_count: i64_usize(row.get(12)?),
        latency_ms: i64_u128(row.get(13)?), source_hash: row.get(14)?,
        session_id: row.get(15)?, call_id: row.get(16)?,
        ref_ids: ref_ids_from_column(row.get(17)?),
    })
}

fn atomic_export_sqlite_jsonl(sqlite_path: &Path, output: &Path) -> std::io::Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let conn = open_sqlite(sqlite_path)?;
    let mut stmt = conn
        .prepare(
            &format!("SELECT {EVENT_SQL_COLUMNS} FROM events ORDER BY line_no ASC"),
        )
        .map_err(sqlite_error)?;
    (|| -> std::io::Result<()> {
        let (file, mut writer) = create_temp_writer(output)?;
        let rows = stmt.query_map([], pulse_event_from_row).map_err(sqlite_error)?;
        for row in rows {
            let event = row.map_err(sqlite_error)?;
            serde_json::to_writer(&mut writer, &event).map_err(io_other)?;
            writer.write_all(b"\n")?;
        }
        finish_temp_writer(file, writer, output)
    })()
}

fn atomic_import_valid_jsonl(
    input: &Path,
    output: &Path,
    expected_scan: &JsonlScan,
) -> std::io::Result<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    (|| -> std::io::Result<()> {
        let (file, mut writer) = create_temp_writer(output)?;
        let copied_scan = {
            let input_file = fs::File::open(input)?;
            let mut reader = BufReader::new(input_file);
            let mut hasher = Sha256::new();
            let mut line = Vec::new();
            let mut event_count = 0usize;
            loop {
                line.clear();
                let read = reader.read_until(b'\n', &mut line)?;
                if read == 0 {
                    break;
                }
                hasher.update(&line);
                match parse_event_line(&line) {
                    Ok(Some(_)) => event_count += 1,
                    Ok(None) => {}
                    Err(()) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            "import source contains corrupt JSONL line(s)",
                        ));
                    }
                }
                writer.write_all(&line)?;
            }

            JsonlScan {
                event_count,
                skipped_lines: 0,
                ledger_sha256: hex_digest(hasher),
            }
        };
        if &copied_scan != expected_scan {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "import source changed while it was being copied",
            ));
        }
        finish_temp_writer(file, writer, output)
    })()
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        match fs::File::open(parent).and_then(|file| file.sync_all()) {
            Ok(()) => {}
            Err(err) if cfg!(windows) && err.kind() == std::io::ErrorKind::PermissionDenied => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

struct PulseLock {
    file: fs::File,
}

impl Drop for PulseLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn acquire_pulse_lock(path: &Path) -> std::io::Result<PulseLock> {
    let lock_path = lock_path_for_ledger(path);
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path)?;
    match FileExt::try_lock(&file) {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Err(pulse_lock_held_error(&lock_path)),
        Err(TryLockError::Error(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {
            return Err(pulse_lock_held_error(&lock_path));
        }
        Err(TryLockError::Error(err)) => return Err(err),
    }

    // Keep the lock-file anchor stable across processes (do not unlink on drop).
    file.set_len(0)?;
    let token = lock_token();
    writeln!(file, "token={token}")?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "created_unix={}", now_unix())?;
    Ok(PulseLock { file })
}

fn pulse_lock_held_error(lock_path: &Path) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::WouldBlock,
        format!("pulse sync lock is held at {}", lock_path.display()),
    )
}

// macOS advisory locks can transiently surface EINVAL while another writer is
// cycling the same lock anchor under heavy local contention. Treat it like
// WouldBlock only for bounded wait paths; direct lock acquisition still returns
// the platform error.
fn retryable_pulse_lock_wait_error(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::InvalidInput
    )
}

fn acquire_pulse_lock_wait(path: &Path, timeout: Duration) -> std::io::Result<PulseLock> {
    let start = Instant::now();
    let lock_path = lock_path_for_ledger(path);
    loop {
        match acquire_pulse_lock(path) {
            Ok(lock) => return Ok(lock),
            Err(err) if retryable_pulse_lock_wait_error(&err) => {
                if start.elapsed() >= timeout {
                    return Err(pulse_lock_held_error(&lock_path));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err),
        }
    }
}

fn lock_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn scan_jsonl<F>(path: &Path, mut on_event: F) -> std::io::Result<JsonlScan>
where
    F: FnMut(&PulseEvent) -> std::io::Result<()>,
{
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Ok(JsonlScan {
                event_count: 0,
                skipped_lines: 0,
                ledger_sha256: hex_sha256(&[]),
            });
        }
        Err(err) => return Err(err),
    };
    let mut hasher = Sha256::new();
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut event_count = 0usize;
    let mut skipped_lines = 0usize;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        hasher.update(&line);
        match parse_event_line(&line) {
            Ok(Some(event)) => { on_event(&event)?; event_count += 1; }
            Ok(None) => {}
            Err(()) => skipped_lines += 1,
        }
    }
    Ok(JsonlScan {
        event_count,
        skipped_lines,
        ledger_sha256: hex_digest(hasher),
    })
}

fn parse_event_line(line: &[u8]) -> Result<Option<PulseEvent>, ()> {
    let trimmed = trim_ascii(line);
    if trimmed.is_empty() {
        return Ok(None);
    }
    let event = serde_json::from_slice::<PulseEvent>(trimmed).map_err(|_| ())?;
    if event.schema_version != PULSE_SCHEMA_VERSION {
        return Err(());
    }
    Ok(Some(event))
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) { value = &value[1..]; }
    while value.last().is_some_and(u8::is_ascii_whitespace) { value = &value[..value.len() - 1]; }
    value
}

fn aggregate_for_path(path: &Path) -> std::io::Result<PulseReport> {
    let mut raw_tokens = 0usize;
    let mut visible_tokens = 0usize;
    let mut recovery_tokens = 0usize;
    let mut task_lossless_tokens = 0usize;
    let mut failures = 0usize;
    let mut cache_hits = 0usize;
    let mut exact_ref_count = 0usize;
    let scan = scan_jsonl(path, |event| {
        raw_tokens = raw_tokens.saturating_add(event.raw_tokens);
        visible_tokens = visible_tokens.saturating_add(event.visible_tokens);
        recovery_tokens = recovery_tokens.saturating_add(event.recovery_tokens);
        if event.task_lossless && !event.failure {
            task_lossless_tokens = task_lossless_tokens
                .saturating_add(event.visible_tokens.saturating_add(event.recovery_tokens));
        }
        failures += usize::from(event.failure);
        cache_hits += usize::from(event.cache_hit);
        exact_ref_count = exact_ref_count.saturating_add(event.exact_ref_count);
        Ok(())
    })?;
    Ok(PulseReport {
        schema_version: PULSE_SCHEMA_VERSION.to_string(),
        status: "ok".to_string(),
        event_count: scan.event_count,
        raw_tokens, visible_tokens, recovery_tokens, task_lossless_tokens,
        failures, cache_hits, exact_ref_count,
        visible_savings: savings_ratio(raw_tokens, visible_tokens),
        recovery_adjusted_savings: savings_ratio(raw_tokens, visible_tokens.saturating_add(recovery_tokens)),
        skipped_lines: scan.skipped_lines,
    })
}

fn hash_event(event: &PulseEvent) -> std::io::Result<String> {
    let bytes = serde_json::to_vec(event).map_err(io_other)?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher)
}

fn hex_digest(hasher: Sha256) -> String {
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn ledger_sibling(path: &Path, name: &str) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new(".")).join(name)
}
fn sqlite_path_for_ledger(path: &Path) -> PathBuf { ledger_sibling(path, "events.sqlite") }
fn meta_path_for_ledger(path: &Path) -> PathBuf { ledger_sibling(path, "events.meta.json") }
fn export_meta_path(path: &Path) -> PathBuf { path.with_extension("meta.json") }
fn lock_path_for_ledger(path: &Path) -> PathBuf { ledger_sibling(path, "sync.lock") }

fn clamp_i64(value: usize) -> i64 { value.min(i64::MAX as usize) as i64 }
fn clamp_u128_i64(value: u128) -> i64 { value.min(i64::MAX as u128) as i64 }
fn bool_i64(value: bool) -> i64 { i64::from(value) }
fn i64_bool(value: i64) -> bool { value != 0 }
fn i64_usize(value: i64) -> usize { value.max(0) as usize }
fn i64_u64(value: i64) -> u64 { value.max(0) as u64 }
fn i64_u128(value: i64) -> u128 { value.max(0) as u128 }

fn hash_hint(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex_encode(&hasher.finalize()[..8])
}

fn io_other(err: serde_json::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

fn sqlite_error(err: rusqlite::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, err)
}

// Session Ledger (bfu): per-session mass × turns accounting.
pub const SESSION_LEDGER_SCHEMA_VERSION: &str = "session-ledger-v1";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionLedgerEntry {
    pub session_id: String,
    pub turns: usize,
    pub raw_tokens: usize,
    pub visible_tokens: usize,
    pub recovery_tokens: usize,
    pub exact_ref_count: usize,
    pub failures: usize,
    pub cache_hits: usize,
    pub tools: BTreeMap<String, usize>,
    pub source_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLedgerReport {
    pub schema_version: String,
    pub total_sessions: usize,
    pub total_turns: usize,
    pub total_raw_tokens: usize,
    pub total_visible_tokens: usize,
    pub total_recovery_tokens: usize,
    pub total_exact_refs: usize,
    pub total_failures: usize,
    pub total_cache_hits: usize,
    pub sessions: Vec<SessionLedgerEntry>,
}

impl SessionLedgerReport {
    pub fn from_ledger(path: &Path) -> std::io::Result<Self> {
        let mut sessions: BTreeMap<String, SessionLedgerEntry> = BTreeMap::new();
        scan_jsonl(path, |event| {
            let sid = event
                .session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let entry = sessions.entry(sid.clone()).or_insert_with(|| SessionLedgerEntry {
                session_id: sid,
                source_hash: event.source_hash.clone(),
                ..SessionLedgerEntry::default()
            });
            entry.turns += 1;
            entry.raw_tokens += event.raw_tokens;
            entry.visible_tokens += event.visible_tokens;
            entry.recovery_tokens += event.recovery_tokens;
            entry.exact_ref_count += event.exact_ref_count;
            entry.failures += usize::from(event.failure);
            entry.cache_hits += usize::from(event.cache_hit);
            *entry.tools.entry(event.tool.clone()).or_insert(0) += 1;
            Ok(())
        })?;
        let sessions_vec: Vec<SessionLedgerEntry> = sessions.into_values().collect();
        let sum = |f: fn(&SessionLedgerEntry) -> usize| sessions_vec.iter().map(f).sum::<usize>();
        Ok(Self {
            schema_version: SESSION_LEDGER_SCHEMA_VERSION.to_string(),
            total_sessions: sessions_vec.len(),
            total_turns: sum(|s| s.turns),
            total_raw_tokens: sum(|s| s.raw_tokens),
            total_visible_tokens: sum(|s| s.visible_tokens),
            total_recovery_tokens: sum(|s| s.recovery_tokens),
            total_exact_refs: sum(|s| s.exact_ref_count),
            total_failures: sum(|s| s.failures),
            total_cache_hits: sum(|s| s.cache_hits),
            sessions: sessions_vec,
        })
    }

    pub fn schema_json() -> serde_json::Value {
        serde_json::json!({
            "schema_version": SESSION_LEDGER_SCHEMA_VERSION,
            "description": "Per-session cost ledger: mass × turns accounting per session, per repo, per agent",
            "entry": {
                "session_id": "string — stable session identifier (MCP session id or 'unknown')",
                "turns": "usize — number of tool calls in this session",
                "raw_tokens": "usize — total raw (uncompressed) tokens across all turns",
                "visible_tokens": "usize — total visible (compressed) tokens across all turns",
                "recovery_tokens": "usize — tokens recovered via expand (charged back to original serve)",
                "exact_ref_count": "usize — total exact refs emitted across all turns",
                "failures": "usize — number of failed tool calls",
                "cache_hits": "usize — number of cache-hit serves",
                "tools": "BTreeMap<String, usize> — per-tool call counts",
                "source_hash": "Option<String> — repo source hash if available"
            },
            "report": {
                "schema_version": "string — session-ledger-v1",
                "total_sessions": "usize — number of distinct sessions",
                "total_turns": "usize — total tool calls across all sessions",
                "total_raw_tokens": "usize", "total_visible_tokens": "usize",
                "total_recovery_tokens": "usize", "total_exact_refs": "usize",
                "total_failures": "usize", "total_cache_hits": "usize",
                "sessions": "Vec<SessionLedgerEntry>"
            },
            "cli": {
                "stats": "tokenzero session-ledger stats [--json] [--root PATH]",
                "export": "tokenzero session-ledger export [--json] [--root PATH]",
                "schema": "tokenzero session-ledger schema"
            }
        })
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Session Cost Ledger (session-ledger-v1)\n");
        out.push_str("═══════════════════════════════════════\n\n");
        out.push_str(&format!(
            "Sessions: {}  Turns: {}  Raw: {}  Visible: {}  Refs: {}  Failures: {}\n\n",
            self.total_sessions,
            self.total_turns,
            self.total_raw_tokens,
            self.total_visible_tokens,
            self.total_exact_refs,
            self.total_failures,
        ));
        out.push_str("Per-session breakdown:\n");
        out.push_str("───────────────────────────────────────\n");
        for s in &self.sessions {
            let savings = if s.raw_tokens > 0 {
                ((s.raw_tokens - s.visible_tokens) as f64 / s.raw_tokens as f64) * 100.0
            } else {
                0.0
            };
            out.push_str(&format!(
                "  {} — turns={} raw={} visible={} (savings {:.1}%) refs={} failures={}\n",
                s.session_id,
                s.turns,
                s.raw_tokens,
                s.visible_tokens,
                savings,
                s.exact_ref_count,
                s.failures
            ));
            let tools: Vec<String> = s.tools.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            out.push_str(&format!("    tools: {}\n", tools.join(", ")));
        }
        out
    }

}

#[cfg(test)]
mod tests;
