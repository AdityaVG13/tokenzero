#![forbid(unsafe_code)]

use fs4::{FileExt, TryLockError};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{
    BufRead, BufReader, BufWriter, Error as IoError, ErrorKind, Result as IoResult, Write,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tempfile::NamedTempFile;
use tokenzero_core::{PULSE_SCHEMA_VERSION, savings_ratio};

trait IntoIo<T> {
    fn into_io(self) -> IoResult<T>;
}

impl<T, E: Into<Box<dyn std::error::Error + Send + Sync>>> IntoIo<T> for Result<T, E> {
    fn into_io(self) -> IoResult<T> {
        self.map_err(|err| IoError::new(ErrorKind::InvalidData, err))
    }
}

const EVENT_SQL_COLUMNS: &str = "schema_version, event, timestamp_unix, tool, mode, raw_tokens, visible_tokens, recovery_tokens, task_lossless, cache_hit, retry_count, failure, exact_ref_count, latency_ms, source_hash, session_id, call_id, ref_ids";
const PULSE_SOURCE_OF_TRUTH: &str = "jsonl";
const PULSE_SYNC_SCHEMA_VERSION: &str = "pulse-sync-v1";
const PULSE_EVENT_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const PULSE_SYNC_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
macro_rules! pulse_structs {
    ($( $(#[$struct_attr:meta])* $name:ident { $($(#[$field_attr:meta])* $field:ident $ty:ty;)* })*) => {
        $(
            #[derive(Debug, Clone, Serialize, Deserialize)]
            $(#[$struct_attr])*
            pub struct $name {
                $(
                    $(#[$field_attr])*
                    pub $field: $ty,
                )*
            }
        )*
    };
}

pulse_structs! {
    PulseEvent {
        schema_version String;
        event String;
        timestamp_unix u64;
        tool String;
        mode String;
        raw_tokens usize;
        visible_tokens usize;
        recovery_tokens usize;
        task_lossless bool;
        cache_hit bool;
        retry_count usize;
        failure bool;
        exact_ref_count usize;
        latency_ms u128;
        source_hash Option<String>;
        /// Serving session id for expand-time attribution.
        #[serde(default, skip_serializing_if = "Option::is_none")] session_id Option<String>;
        /// Call id within the session (e.g. JSON-RPC id).
        #[serde(default, skip_serializing_if = "Option::is_none")] call_id Option<String>;
        /// Serve/expand tz:// refs — RACC join key.
        #[serde(default, skip_serializing_if = "Vec::is_empty")] ref_ids Vec<String>;
    }
    #[derive(Default)]
    PulseReport {
        schema_version String;
        status String;
        event_count usize;
        raw_tokens usize;
        visible_tokens usize;
        recovery_tokens usize;
        task_lossless_tokens usize;
        failures usize;
        cache_hits usize;
        exact_ref_count usize;
        visible_savings f64;
        recovery_adjusted_savings f64;
        /// Corrupt/non-empty unparsable ledger lines.
        #[serde(default)] skipped_lines usize;
    }
    PulseSyncMeta {
        schema_version String;
        source_of_truth String;
        ledger_sha256 String;
        event_count usize;
        skipped_lines usize;
        updated_unix u64;
    }
    PulseSyncStatus {
        ok bool;
        source_of_truth String;
        ledger_path PathBuf;
        sqlite_path PathBuf;
        meta_path PathBuf;
        event_count usize;
        skipped_lines usize;
        ledger_sha256 String;
    }
    PulseDoctorReport {
        ok bool;
        source_of_truth String;
        ledger_path PathBuf;
        sqlite_path PathBuf;
        meta_path PathBuf;
        event_count usize;
        skipped_lines usize;
        ledger_sha256 String;
        sqlite_integrity String;
        marker_match bool;
        hot_index_used bool;
    }
}

macro_rules! data_row {
    ($ty:ident; $($field:ident = $value:expr;)*) => {
        $ty {
            $($field: $value,)*
        }
    };
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
        data_row! { PulseEvent;
            schema_version = PULSE_SCHEMA_VERSION.to_string();
            event = "tool_call".to_string();
            timestamp_unix = now_unix();
            tool = tool.to_string();
            mode = mode.to_string();
            raw_tokens = raw_tokens;
            visible_tokens = visible_tokens;
            recovery_tokens = recovery_tokens;
            task_lossless = true;
            cache_hit = false;
            retry_count = 0;
            failure = false;
            exact_ref_count = exact_ref_count;
            latency_ms = latency_ms;
            source_hash = source_hint.map(hash_hint);
            session_id = None;
            call_id = None;
            ref_ids = Vec::new();
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

fn with_pulse_lock<T>(
    path: &Path,
    timeout: Duration,
    action: impl FnOnce() -> IoResult<T>,
) -> IoResult<T> {
    let _lock = acquire_pulse_lock_wait(path, timeout)?;
    action()
}

pub fn record_event(path: &Path, event: &PulseEvent) -> IoResult<()> {
    with_pulse_lock(path, PULSE_EVENT_LOCK_TIMEOUT, || {
        ensure_parent(path)?;
        let file_existed = path.exists();
        let mut file = OpenOptions::new().append(true).create(true).open(path)?;
        // One logical JSONL record per append; no fsync (telemetry, crash-loss ok).
        serde_json::to_writer(&mut file, event).into_io()?;
        file.write_all(b"\n")?;
        if !file_existed {
            sync_parent(path)?;
        }
        Ok(())
    })
}

pub fn sync_jsonl_to_sqlite(path: &Path) -> IoResult<PulseSyncStatus> {
    with_pulse_lock(path, PULSE_SYNC_LOCK_TIMEOUT, || {
        sync_jsonl_to_sqlite_locked(path)
    })
}

pub fn export_jsonl(path: &Path, output: &Path) -> IoResult<PulseSyncStatus> {
    with_pulse_lock(path, PULSE_SYNC_LOCK_TIMEOUT, || {
        let status = sync_jsonl_to_sqlite_locked(path)?;
        atomic_export_sqlite_jsonl(&status.sqlite_path, output)?;
        write_sidecar_meta(
            &export_meta_path(output),
            &meta_from_scan(&scan_jsonl(output, |_| Ok(()))?),
        )?;
        Ok(status)
    })
}

pub fn import_jsonl(input: &Path, path: &Path) -> IoResult<PulseSyncStatus> {
    with_pulse_lock(path, PULSE_SYNC_LOCK_TIMEOUT, || {
        let input_source = ensure_import_not_older(input, path)?;
        atomic_import_valid_jsonl(input, path, &input_source.scan)?;
        sync_jsonl_to_sqlite_locked(path)
    })
}

pub fn doctor_jsonl_sqlite(path: &Path) -> IoResult<PulseDoctorReport> {
    let status = sync_jsonl_to_sqlite(path)?;
    let conn = open_sqlite(&status.sqlite_path)?;
    let sqlite_integrity = conn
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .into_io()?;
    let sqlite_meta = read_sqlite_meta(&conn)?;
    let sidecar_meta = read_sidecar_meta(&status.meta_path)?;
    let marker_match = sqlite_meta.ledger_sha256 == status.ledger_sha256
        && sidecar_meta.ledger_sha256 == status.ledger_sha256
        && sqlite_meta.event_count == status.event_count
        && sidecar_meta.event_count == status.event_count;
    let hot_index_used = hot_index_is_used(&conn)?;
    Ok(data_row! { PulseDoctorReport;
        ok = status.ok && sqlite_integrity == "ok" && marker_match && hot_index_used;
        source_of_truth = status.source_of_truth;
        ledger_path = status.ledger_path;
        sqlite_path = status.sqlite_path;
        meta_path = status.meta_path;
        event_count = status.event_count;
        skipped_lines = status.skipped_lines;
        ledger_sha256 = status.ledger_sha256;
        sqlite_integrity = sqlite_integrity;
        marker_match = marker_match;
        hot_index_used = hot_index_used;
    })
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

fn sync_jsonl_to_sqlite_locked(path: &Path) -> IoResult<PulseSyncStatus> {
    let sqlite_path = sqlite_path_for_ledger(path);
    let meta_path = meta_path_for_ledger(path);
    for p in [path.parent(), sqlite_path.parent()].into_iter().flatten() {
        fs::create_dir_all(p)?;
    }

    let scan = sync_jsonl_into_sqlite_cache(path, &sqlite_path)?;

    let meta = meta_from_scan(&scan);
    write_sidecar_meta(&meta_path, &meta)?;

    Ok(data_row! { PulseSyncStatus;
        ok = scan.skipped_lines == 0;
        source_of_truth = PULSE_SOURCE_OF_TRUTH.to_string();
        ledger_path = path.to_path_buf();
        sqlite_path = sqlite_path;
        meta_path = meta_path;
        event_count = scan.event_count;
        skipped_lines = scan.skipped_lines;
        ledger_sha256 = scan.ledger_sha256;
    })
}

fn open_sqlite(path: &Path) -> IoResult<Connection> {
    let conn = Connection::open(path).into_io()?;
    conn.busy_timeout(Duration::from_secs(5)).into_io()?;
    for (key, val) in [
        ("journal_mode", "WAL"),
        ("synchronous", "NORMAL"),
        ("fullfsync", "ON"),
        ("wal_autocheckpoint", "1000"),
        ("foreign_keys", "ON"),
    ] {
        conn.pragma_update(None, key, val).into_io()?;
    }
    Ok(conn)
}

fn sync_jsonl_into_sqlite_cache(ledger_path: &Path, sqlite_path: &Path) -> IoResult<JsonlScan> {
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

fn open_or_rebuild_sqlite(path: &Path) -> IoResult<Connection> {
    let open = || {
        let conn = open_sqlite(path)?;
        init_sqlite(&conn)?;
        Ok(conn)
    };
    match open() {
        Ok(conn) => Ok(conn),
        Err(err) if sqlite_cache_can_rebuild(&err) => {
            remove_sqlite_cache_files(path)?;
            open()
        }
        Err(err) => Err(err),
    }
}

fn sqlite_cache_can_rebuild(err: &IoError) -> bool {
    err.kind() == ErrorKind::InvalidData
        && [
            "file is not a database",
            "database disk image is malformed",
            "not a database",
            "has no column named",
            "no such column",
            "no such table",
        ]
        .iter()
        .any(|needle| err.to_string().contains(needle))
}

fn remove_sqlite_cache_files(path: &Path) -> IoResult<()> {
    for suffix in ["", "-wal", "-shm"] {
        match fs::remove_file(sqlite_sidecar_path(path, suffix)) {
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            result => result?,
        }
    }
    Ok(())
}

/// JSON-encode ref ids for the sqlite sidecar; NULL when empty.
fn ref_ids_to_column(ref_ids: &[String]) -> IoResult<Option<String>> {
    if ref_ids.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(ref_ids).map(Some).into_io()
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

fn init_sqlite(conn: &Connection) -> IoResult<()> {
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
    .into_io()?;
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

fn write_sqlite_events_from_jsonl(conn: &mut Connection, path: &Path) -> IoResult<JsonlScan> {
    let tx = conn.transaction().into_io()?;
    tx.execute("DELETE FROM events", []).into_io()?;
    let scan = {
        let mut stmt = tx.prepare(
            "INSERT INTO events (line_no, schema_version, event, timestamp_unix, tool, mode, raw_tokens, visible_tokens, recovery_tokens, task_lossless, cache_hit, retry_count, failure, exact_ref_count, latency_ms, source_hash, session_id, call_id, ref_ids, record_hash) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        ).into_io()?;
        let mut line_no = 0i64;
        scan_jsonl(path, |event| {
            line_no += 1;
            stmt.execute(params![
                line_no,
                &event.schema_version,
                &event.event,
                event.timestamp_unix as i64,
                &event.tool,
                &event.mode,
                clamp_i64(event.raw_tokens),
                clamp_i64(event.visible_tokens),
                clamp_i64(event.recovery_tokens),
                bool_i64(event.task_lossless),
                bool_i64(event.cache_hit),
                clamp_i64(event.retry_count),
                bool_i64(event.failure),
                clamp_i64(event.exact_ref_count),
                clamp_u128_i64(event.latency_ms),
                event.source_hash.as_deref(),
                event.session_id.as_deref(),
                event.call_id.as_deref(),
                ref_ids_to_column(&event.ref_ids)?,
                hex_sha256(&serde_json::to_vec(event).into_io()?),
            ])
            .into_io()?;
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
        tx.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![k, v],
        )
        .into_io()?;
    }
    tx.commit().into_io()?;
    Ok(scan)
}

fn read_sqlite_meta(conn: &Connection) -> IoResult<PulseSyncMeta> {
    Ok(data_row! { PulseSyncMeta;
        schema_version = sqlite_meta_value(conn, "schema_version")?;
        source_of_truth = sqlite_meta_value(conn, "source_of_truth")?;
        ledger_sha256 = sqlite_meta_value(conn, "ledger_sha256")?;
        event_count = sqlite_meta_value(conn, "event_count")?.parse().unwrap_or(0);
        skipped_lines = sqlite_meta_value(conn, "skipped_lines")?.parse().unwrap_or(0);
        updated_unix = sqlite_meta_value(conn, "updated_unix")?.parse().unwrap_or(0);
    })
}

fn sqlite_meta_value(conn: &Connection, key: &str) -> IoResult<String> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
        row.get::<_, String>(0)
    })
    .into_io()
}

fn hot_index_is_used(conn: &Connection) -> IoResult<bool> {
    let mut stmt = conn
        .prepare("EXPLAIN QUERY PLAN SELECT line_no FROM events WHERE tool = ?1 ORDER BY timestamp_unix DESC LIMIT 10")
        .into_io()?;
    for detail in stmt
        .query_map(["read"], |row| row.get::<_, String>(3))
        .into_io()?
    {
        if detail.into_io()?.contains("idx_events_tool_time") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn write_sidecar_meta(path: &Path, meta: &PulseSyncMeta) -> IoResult<()> {
    let bytes = serde_json::to_vec_pretty(meta).into_io()?;
    atomic_write(path, &bytes)
}

fn read_sidecar_meta(path: &Path) -> IoResult<PulseSyncMeta> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes).into_io()
}

struct VerifiedImportSource {
    scan: JsonlScan,
    meta: Option<PulseSyncMeta>,
}

macro_rules! reject {
    ($kind:ident, $message:expr $(,)?) => {
        return Err(IoError::new(ErrorKind::$kind, $message))
    };
}

fn ensure_import_not_older(input: &Path, current_ledger: &Path) -> IoResult<VerifiedImportSource> {
    if !fs::metadata(input)?.is_file() {
        reject!(InvalidInput, "import source is not a regular file");
    }
    let input_source = verify_import_source(input)?;
    let current_scan = scan_jsonl(current_ledger, |_| Ok(()))?;
    if input_source.scan.ledger_sha256 == current_scan.ledger_sha256 {
        return Ok(input_source);
    }

    let Some(current_meta) = read_trusted_sidecar_meta(&meta_path_for_ledger(current_ledger))?
    else {
        if current_scan.event_count == 0 && current_scan.skipped_lines == 0 {
            return Ok(input_source);
        }
        reject!(
            InvalidInput,
            "current Pulse ledger has no version marker; refusing to overwrite it",
        );
    };
    let Some(input_meta) = &input_source.meta else {
        reject!(
            InvalidInput,
            "import snapshot has no version marker; refusing to overwrite the current Pulse ledger",
        );
    };
    if !meta_matches_scan(&current_meta, &current_scan) {
        if current_scan.skipped_lines > 0 && input_meta.updated_unix > current_meta.updated_unix {
            return Ok(input_source);
        }
        reject!(
            InvalidInput,
            "current Pulse ledger has unsynced changes; run `tokenzero pulse sync` before importing a different snapshot",
        );
    }
    if input_meta.updated_unix <= current_meta.updated_unix {
        reject!(
            InvalidInput,
            "import snapshot is not newer than the current Pulse ledger marker"
        );
    }
    Ok(input_source)
}

fn verify_import_source(input: &Path) -> IoResult<VerifiedImportSource> {
    let scan = scan_jsonl(input, |_| Ok(()))?;
    if scan.skipped_lines > 0 {
        reject!(InvalidData, "import source contains corrupt JSONL line(s)");
    }
    let meta = read_trusted_sidecar_meta(&export_meta_path(input))?;
    if meta
        .as_ref()
        .is_some_and(|meta| !meta_matches_scan(meta, &scan))
    {
        reject!(
            InvalidInput,
            "import snapshot marker does not match source JSONL"
        );
    }
    Ok(VerifiedImportSource { scan, meta })
}

fn read_trusted_sidecar_meta(path: &Path) -> IoResult<Option<PulseSyncMeta>> {
    match read_sidecar_meta(path) {
        Ok(meta)
            if meta.schema_version == PULSE_SYNC_SCHEMA_VERSION
                && meta.source_of_truth == PULSE_SOURCE_OF_TRUTH =>
        {
            Ok(Some(meta))
        }
        Ok(_) => Err(IoError::new(
            ErrorKind::InvalidInput,
            format!(
                "Pulse marker has an unexpected schema or source at {}",
                path.display()
            ),
        )),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn meta_from_scan(scan: &JsonlScan) -> PulseSyncMeta {
    data_row! { PulseSyncMeta;
        schema_version = PULSE_SYNC_SCHEMA_VERSION.to_string();
        source_of_truth = PULSE_SOURCE_OF_TRUTH.to_string();
        ledger_sha256 = scan.ledger_sha256.clone();
        event_count = scan.event_count;
        skipped_lines = scan.skipped_lines;
        updated_unix = now_unix();
    }
}

fn meta_matches_scan(meta: &PulseSyncMeta, scan: &JsonlScan) -> bool {
    meta.ledger_sha256 == scan.ledger_sha256
        && meta.event_count == scan.event_count
        && meta.skipped_lines == scan.skipped_lines
}

fn atomic_write_with<T>(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<fs::File>) -> IoResult<T>,
) -> IoResult<T> {
    let file = NamedTempFile::new_in(path.parent().unwrap_or_else(|| Path::new(".")))?;
    let mut writer = BufWriter::new(file.reopen()?);
    let result = write(&mut writer)?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    drop(writer);
    file.persist(path)
        .map_err(|err| IoError::new(err.error.kind(), err.error))?;
    sync_parent(path)?;
    Ok(result)
}

fn ensure_parent(path: &Path) -> IoResult<()> {
    path.parent().map_or(Ok(()), fs::create_dir_all)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> IoResult<()> {
    ensure_parent(path)?;
    atomic_write_with(path, |writer| writer.write_all(bytes))
}

fn pulse_event_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PulseEvent> {
    Ok(data_row! { PulseEvent;
        schema_version = row.get(0)?;
        event = row.get(1)?;
        timestamp_unix = i64_u64(row.get(2)?);
        tool = row.get(3)?;
        mode = row.get(4)?;
        raw_tokens = i64_usize(row.get(5)?);
        visible_tokens = i64_usize(row.get(6)?);
        recovery_tokens = i64_usize(row.get(7)?);
        task_lossless = i64_bool(row.get(8)?);
        cache_hit = i64_bool(row.get(9)?);
        retry_count = i64_usize(row.get(10)?);
        failure = i64_bool(row.get(11)?);
        exact_ref_count = i64_usize(row.get(12)?);
        latency_ms = i64_u128(row.get(13)?);
        source_hash = row.get(14)?;
        session_id = row.get(15)?;
        call_id = row.get(16)?;
        ref_ids = ref_ids_from_column(row.get(17)?);
    })
}

fn atomic_export_sqlite_jsonl(sqlite_path: &Path, output: &Path) -> IoResult<()> {
    ensure_parent(output)?;
    let conn = open_sqlite(sqlite_path)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {EVENT_SQL_COLUMNS} FROM events ORDER BY line_no ASC"
        ))
        .into_io()?;
    atomic_write_with(output, |writer| {
        for row in stmt.query_map([], pulse_event_from_row).into_io()? {
            serde_json::to_writer(&mut *writer, &row.into_io()?).into_io()?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    })
}

fn atomic_import_valid_jsonl(
    input: &Path,
    output: &Path,
    expected_scan: &JsonlScan,
) -> IoResult<()> {
    ensure_parent(output)?;
    atomic_write_with(output, |writer| {
        let copied_scan = scan_reader(
            BufReader::new(fs::File::open(input)?),
            |line, _, corrupt| {
                if corrupt {
                    reject!(InvalidData, "import source contains corrupt JSONL line(s)");
                }
                writer.write_all(line)
            },
        )?;
        if &copied_scan != expected_scan {
            reject!(
                InvalidInput,
                "import source changed while it was being copied"
            );
        }
        Ok(())
    })
}

fn sync_parent(path: &Path) -> IoResult<()> {
    path.parent().map_or(Ok(()), |parent| {
        match fs::File::open(parent).and_then(|file| file.sync_all()) {
            Err(err) if !(cfg!(windows) && err.kind() == ErrorKind::PermissionDenied) => Err(err),
            _ => Ok(()),
        }
    })
}

struct PulseLock {
    file: fs::File,
}

impl Drop for PulseLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

fn acquire_pulse_lock(path: &Path) -> IoResult<PulseLock> {
    let lock_path = lock_path_for_ledger(path);
    ensure_parent(&lock_path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    match FileExt::try_lock(&file) {
        Ok(()) => {}
        Err(TryLockError::Error(err)) if err.kind() != ErrorKind::WouldBlock => return Err(err),
        Err(_) => return Err(pulse_lock_held_error(&lock_path)),
    }

    // Keep the lock-file anchor stable across processes (do not unlink on drop).
    file.set_len(0)?;
    let token = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    writeln!(file, "token={token}")?;
    writeln!(file, "pid={}", std::process::id())?;
    writeln!(file, "created_unix={}", now_unix())?;
    Ok(PulseLock { file })
}

fn pulse_lock_held_error(lock_path: &Path) -> IoError {
    IoError::new(
        ErrorKind::WouldBlock,
        format!("pulse sync lock is held at {}", lock_path.display()),
    )
}

// macOS advisory locks can transiently surface EINVAL while another writer is
// cycling the same lock anchor under heavy local contention. Treat it like
// WouldBlock only for bounded wait paths; direct lock acquisition still returns
// the platform error.
fn acquire_pulse_lock_wait(path: &Path, timeout: Duration) -> IoResult<PulseLock> {
    let start = Instant::now();
    let lock_path = lock_path_for_ledger(path);
    loop {
        match acquire_pulse_lock(path) {
            Ok(lock) => return Ok(lock),
            Err(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::InvalidInput) => {
                if start.elapsed() >= timeout {
                    return Err(pulse_lock_held_error(&lock_path));
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err),
        }
    }
}

fn scan_reader<R: BufRead>(
    mut reader: R,
    mut on_line: impl FnMut(&[u8], Option<&PulseEvent>, bool) -> IoResult<()>,
) -> IoResult<JsonlScan> {
    let (mut hasher, mut line) = (Sha256::new(), Vec::new());
    let (mut event_count, mut skipped_lines) = (0, 0);
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            break;
        }
        hasher.update(&line);
        match parse_event_line(&line) {
            Ok(event) => {
                event_count += usize::from(event.is_some());
                on_line(&line, event.as_ref(), false)?;
            }
            Err(()) => {
                skipped_lines += 1;
                on_line(&line, None, true)?;
            }
        }
    }
    Ok(JsonlScan {
        event_count,
        skipped_lines,
        ledger_sha256: hex_encode(hasher.finalize()),
    })
}

fn scan_jsonl<F>(path: &Path, mut on_event: F) -> IoResult<JsonlScan>
where
    F: FnMut(&PulseEvent) -> IoResult<()>,
{
    match fs::File::open(path) {
        Ok(file) => scan_reader(BufReader::new(file), |_, event, _| {
            event.map_or(Ok(()), &mut on_event)
        }),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(JsonlScan {
            event_count: 0,
            skipped_lines: 0,
            ledger_sha256: hex_sha256(&[]),
        }),
        Err(err) => Err(err),
    }
}

fn parse_event_line(line: &[u8]) -> Result<Option<PulseEvent>, ()> {
    let trimmed = line.trim_ascii();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let event = serde_json::from_slice::<PulseEvent>(trimmed).map_err(|_| ())?;
    if event.schema_version != PULSE_SCHEMA_VERSION {
        return Err(());
    }
    Ok(Some(event))
}

pub fn report_for_path(path: &Path) -> IoResult<PulseReport> {
    let mut report = PulseReport {
        schema_version: PULSE_SCHEMA_VERSION.to_string(),
        status: "ok".to_string(),
        ..PulseReport::default()
    };
    let scan = scan_jsonl(path, |event| {
        report.raw_tokens = report.raw_tokens.saturating_add(event.raw_tokens);
        report.visible_tokens = report.visible_tokens.saturating_add(event.visible_tokens);
        report.recovery_tokens = report.recovery_tokens.saturating_add(event.recovery_tokens);
        if event.task_lossless && !event.failure {
            report.task_lossless_tokens = report
                .task_lossless_tokens
                .saturating_add(event.visible_tokens.saturating_add(event.recovery_tokens));
        }
        report.failures += usize::from(event.failure);
        report.cache_hits += usize::from(event.cache_hit);
        report.exact_ref_count = report.exact_ref_count.saturating_add(event.exact_ref_count);
        Ok(())
    })?;
    report.event_count = scan.event_count;
    report.skipped_lines = scan.skipped_lines;
    report.visible_savings = savings_ratio(report.raw_tokens, report.visible_tokens);
    report.recovery_adjusted_savings = savings_ratio(
        report.raw_tokens,
        report.visible_tokens.saturating_add(report.recovery_tokens),
    );
    Ok(report)
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(hasher.finalize())
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

macro_rules! simple_fns {
    ($($name:ident($arg:ident: $arg_ty:ty) -> $out:ty $body:block)*) => {
        $(
            fn $name($arg: $arg_ty) -> $out $body
        )*
    };
}

fn ledger_sibling(path: &Path, name: &str) -> PathBuf {
    path.parent().unwrap_or_else(|| Path::new(".")).join(name)
}
simple_fns! {
    sqlite_path_for_ledger(path: &Path) -> PathBuf {
        ledger_sibling(path, "events.sqlite")
    }
    meta_path_for_ledger(path: &Path) -> PathBuf {
        ledger_sibling(path, "events.meta.json")
    }
    export_meta_path(path: &Path) -> PathBuf {
        path.with_extension("meta.json")
    }
    lock_path_for_ledger(path: &Path) -> PathBuf {
        ledger_sibling(path, "sync.lock")
    }
    clamp_i64(value: usize) -> i64 {
        value.min(i64::MAX as usize) as i64
    }
    clamp_u128_i64(value: u128) -> i64 {
        value.min(i64::MAX as u128) as i64
    }
    bool_i64(value: bool) -> i64 {
        i64::from(value)
    }
    i64_bool(value: i64) -> bool {
        value != 0
    }
    i64_usize(value: i64) -> usize {
        value.max(0) as usize
    }
    i64_u64(value: i64) -> u64 {
        value.max(0) as u64
    }
    i64_u128(value: i64) -> u128 {
        value.max(0) as u128
    }
}

fn hash_hint(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex_encode(&hasher.finalize()[..8])
}

// Session Ledger (bfu): per-session mass × turns accounting.
pub const SESSION_LEDGER_SCHEMA_VERSION: &str = "session-ledger-v1";

pulse_structs! {
    #[derive(Default)]
    SessionLedgerEntry {
        session_id String;
        turns usize;
        raw_tokens usize;
        visible_tokens usize;
        recovery_tokens usize;
        exact_ref_count usize;
        failures usize;
        cache_hits usize;
        tools BTreeMap<String, usize>;
        source_hash Option<String>;
    }
    SessionLedgerReport {
        schema_version String;
        total_sessions usize;
        total_turns usize;
        total_raw_tokens usize;
        total_visible_tokens usize;
        total_recovery_tokens usize;
        total_exact_refs usize;
        total_failures usize;
        total_cache_hits usize;
        sessions Vec<SessionLedgerEntry>;
    }
}

impl SessionLedgerReport {
    pub fn from_ledger(path: &Path) -> IoResult<Self> {
        let mut sessions: BTreeMap<String, SessionLedgerEntry> = BTreeMap::new();
        scan_jsonl(path, |event| {
            let sid = event
                .session_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            let entry = sessions
                .entry(sid.clone())
                .or_insert_with(|| SessionLedgerEntry {
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
        serde_json::from_str("{\"schema_version\":\"session-ledger-v1\",\"description\":\"Per-session cost ledger: mass × turns accounting per session, per repo, per agent\",\"entry\":{\"session_id\":\"string — stable session identifier (MCP session id or 'unknown')\",\"turns\":\"usize — number of tool calls in this session\",\"raw_tokens\":\"usize — total raw (uncompressed) tokens across all turns\",\"visible_tokens\":\"usize — total visible (compressed) tokens across all turns\",\"recovery_tokens\":\"usize — tokens recovered via expand (charged back to original serve)\",\"exact_ref_count\":\"usize — total exact refs emitted across all turns\",\"failures\":\"usize — number of failed tool calls\",\"cache_hits\":\"usize — number of cache-hit serves\",\"tools\":\"BTreeMap<String, usize> — per-tool call counts\",\"source_hash\":\"Option<String> — repo source hash if available\"},\"report\":{\"schema_version\":\"string — session-ledger-v1\",\"total_sessions\":\"usize — number of distinct sessions\",\"total_turns\":\"usize — total tool calls across all sessions\",\"total_raw_tokens\":\"usize\",\"total_visible_tokens\":\"usize\",\"total_recovery_tokens\":\"usize\",\"total_exact_refs\":\"usize\",\"total_failures\":\"usize\",\"total_cache_hits\":\"usize\",\"sessions\":\"Vec<SessionLedgerEntry>\"},\"cli\":{\"stats\":\"tokenzero session-ledger stats [--json] [--root PATH]\",\"export\":\"tokenzero session-ledger export [--json] [--root PATH]\",\"schema\":\"tokenzero session-ledger schema\"}}")
        .expect("static session ledger schema is valid JSON")
    }

    pub fn render_text(&self) -> String {
        let mut out = String::from(
            "Session Cost Ledger (session-ledger-v1)\n═══════════════════════════════════════\n\n",
        );
        writeln!(
            out,
            "Sessions: {}  Turns: {}  Raw: {}  Visible: {}  Refs: {}  Failures: {}\n",
            self.total_sessions,
            self.total_turns,
            self.total_raw_tokens,
            self.total_visible_tokens,
            self.total_exact_refs,
            self.total_failures,
        )
        .unwrap();
        out.push_str("Per-session breakdown:\n───────────────────────────────────────\n");
        for s in &self.sessions {
            let savings = if s.raw_tokens > 0 {
                ((s.raw_tokens - s.visible_tokens) as f64 / s.raw_tokens as f64) * 100.0
            } else {
                0.0
            };
            writeln!(
                out,
                "  {} — turns={} raw={} visible={} (savings {:.1}%) refs={} failures={}",
                s.session_id,
                s.turns,
                s.raw_tokens,
                s.visible_tokens,
                savings,
                s.exact_ref_count,
                s.failures,
            )
            .unwrap();
            out.push_str("    tools: ");
            for (index, (tool, count)) in s.tools.iter().enumerate() {
                write!(out, "{}{tool}:{count}", if index == 0 { "" } else { ", " }).unwrap();
            }
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests;
