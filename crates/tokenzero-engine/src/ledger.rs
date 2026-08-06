//! Queryable, append-only accounting for served TokenZero responses.
//!
//! Every JSONL line is a tokenzero.ledger.v2 LedgerRecord (migration note:
//! v2 is a strict superset of v1 — it adds the `recovery_costs` block with
//! recovery-adjusted fields and the RATC identity, ratc = visible + expand +
//! rho_fail*retries + lambda_fail*fails. v1 lines remain readable: the
//! recovery_costs field defaults to the honest zero/unknown state, and the
//! reader accepts both schema tags). prevented_tokens is derived only from
//! existing per-response dedup.visible_tokens_saved and
//! diff.visible_tokens_saved telemetry. It is not a prevented-read estimate.
//! saved_bytes separately preserves session_delta.saved_bytes. Recovery-cost
//! fields are telemetry-only counters; no payload bytes ever enter a record.
//!
//! Telemetry pointer contract (produced by the expand surface, bead h470.3):
//!   /expand/visible_tokens     -> recovery_costs.expand_tokens
//!   /expand/count              -> recovery_costs.expand_count
//!   /expand/retry_count        -> recovery_costs.retry_count
//!   /expand/fail_count         -> recovery_costs.fail_count
//!   /expand/dangling_ref_count -> recovery_costs.dangling_ref_count
//! task_success and anchor_recall_ok are per-task facts (E1.2 grouping is out
//! of scope); they serialize as null until a per-task roll-up sets them.
//!
//! The first record writes synchronously through a retained O_APPEND handle.
//! Later records batch for at most 250 ms under normal scheduler operation.
//! Drop and explicit flush drain without per-turn fsync. Before a write would
//! exceed DEFAULT_MAX_LEDGER_BYTES, the active file rotates to .jsonl.1.
//! Queries scan both generations and ignore malformed lines, including a torn
//! final line.

use fs4::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokenzero_core::ToolResponse;

pub const LEDGER_SCHEMA: &str = "tokenzero.ledger.v1";
pub const LEDGER_SCHEMA_V2: &str = "tokenzero.ledger.v2";
/// Weight applied to retry counts in the RATC identity. The config surface
/// (bead radc-e3-9ax3.3) will make these operator-settable; until then the
/// honest default is 0 so no unmeasured cost is fabricated.
pub const DEFAULT_RHO_FAIL: f64 = 0.0;
/// Weight applied to fail counts in the RATC identity.
pub const DEFAULT_LAMBDA_FAIL: f64 = 0.0;
pub const TOKENZERO_AGENT_ENV: &str = "TOKENZERO_AGENT";
pub const DEFAULT_MAX_LEDGER_BYTES: u64 = 8 * 1024 * 1024;
pub const DEFAULT_MAX_LEDGER_GENERATIONS: usize = 4;
pub const DEFAULT_MAX_LEDGER_TOTAL_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionIdentity {
    #[serde(rename = "crate")]
    pub crate_version: String,
    pub git_describe: Option<String>,
}

/// Recovery-adjusted cost block added by tokenzero.ledger.v2. All fields are
/// telemetry-only counters/bools; the block is self-contained so the RATC
/// identity can be audited arithmetically from one record line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryCosts {
    /// Mirrors token_mass.visible_tokens so `ratc` is self-contained.
    pub visible_tokens: u64,
    /// Billed tokens spent on ref expansion (recovery), 0 until h470.3 counts them.
    pub expand_tokens: u64,
    pub expand_count: u64,
    pub retry_count: u64,
    pub fail_count: u64,
    /// Weight per retry in the RATC identity (DEFAULT_RHO_FAIL until E3.3 config).
    pub rho_fail: f64,
    /// Weight per failure in the RATC identity (DEFAULT_LAMBDA_FAIL until E3.3 config).
    pub lambda_fail: f64,
    /// Per-task outcome; null = unknown (E1.2 per-task grouping out of scope).
    pub task_success: Option<bool>,
    /// Whether anchor recall succeeded for the task; null = unknown.
    pub anchor_recall_ok: Option<bool>,
    pub dangling_ref_count: u64,
    /// Recovery-adjusted token cost: visible + expand + rho_fail*retries +
    /// lambda_fail*fails. Stored on the record so auditors read one field.
    pub ratc: f64,
}

impl Default for RecoveryCosts {
    fn default() -> Self {
        Self {
            visible_tokens: 0,
            expand_tokens: 0,
            expand_count: 0,
            retry_count: 0,
            fail_count: 0,
            rho_fail: DEFAULT_RHO_FAIL,
            lambda_fail: DEFAULT_LAMBDA_FAIL,
            task_success: None,
            anchor_recall_ok: None,
            dangling_ref_count: 0,
            ratc: 0.0,
        }
    }
}

impl RecoveryCosts {
    /// The RATC identity, computed from this block's own fields.
    pub fn compute_ratc(&self) -> f64 {
        (self.visible_tokens + self.expand_tokens) as f64
            + self.rho_fail * self.retry_count as f64
            + self.lambda_fail * self.fail_count as f64
    }

    /// Return a copy whose ratc field satisfies the identity exactly.
    pub fn with_ratc(mut self) -> Self {
        self.ratc = self.compute_ratc();
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenMass {
    pub visible_tokens: u64,
    pub raw_tokens: u64,
    /// Existing dedup/diff token savings only; never a prevented-read estimate.
    pub prevented_tokens: u64,
    pub saved_bytes: u64,
}

/// One served response in the versioned tokenzero.ledger JSONL schema
/// (v2 since 1.4.x; v1 lines remain readable, see module docs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRecord {
    pub schema: String,
    pub timestamp_ms: u64,
    pub session_id: String,
    pub repo: String,
    pub agent: Option<String>,
    pub version: VersionIdentity,
    pub tool: String,
    pub token_mass: TokenMass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eviction_amortization: Option<Value>,
    pub cumulative_session_cost_tokens: u64,
    pub optimization_tags: Vec<String>,
    /// tokenzero.ledger.v2 recovery-adjusted block. serde(default) keeps
    /// tokenzero.ledger.v1 lines readable with the honest zero/unknown state.
    #[serde(default)]
    pub recovery_costs: RecoveryCosts,
}

#[derive(Debug)]
pub(crate) struct LedgerWriter {
    session_id: String,
    repo: String,
    agent: Option<String>,
    version: VersionIdentity,
    optimization_tags: Vec<String>,
    cumulative_visible_tokens: Mutex<u64>,
    path: PathBuf,
    max_bytes: u64,
    io: Mutex<LedgerMode>,
}

#[derive(Debug)]
enum LedgerMode {
    Direct {
        open_file: Option<File>,
        accepted_record: bool,
    },
    Buffered(Arc<LedgerIo>),
}

#[derive(Debug)]
struct LedgerIo {
    path: PathBuf,
    max_bytes: u64,
    state: Mutex<LedgerIoState>,
}

#[derive(Debug)]
struct FlushScheduler {
    registry: Mutex<FlushRegistry>,
    wake: Condvar,
}

#[derive(Debug)]
struct FlushRegistry {
    targets: Vec<Weak<LedgerIo>>,
    generation: u64,
}

#[derive(Debug)]
struct LedgerIoState {
    /// Kept-open append handle so warm MCP paths avoid open/close per call.
    open_file: Option<File>,
    /// Lazily allocated write-behind buffer: warm MCP batches records into one write(2).
    write_buf: Vec<u8>,
    buffered_at: Option<Instant>,
}

const LEDGER_FLUSH_BYTES: usize = 4 * 1024;
const LEDGER_FLUSH_WINDOW: Duration = Duration::from_millis(250);

static FLUSH_SCHEDULER: LazyLock<FlushScheduler> = LazyLock::new(|| FlushScheduler {
    registry: Mutex::new(FlushRegistry {
        targets: Vec::new(),
        generation: 0,
    }),
    wake: Condvar::new(),
});
static FLUSH_THREAD: LazyLock<io::Result<std::thread::JoinHandle<()>>> = LazyLock::new(|| {
    std::thread::Builder::new()
        .name("tokenzero-ledger-flush".to_owned())
        .spawn(run_flush_scheduler)
});

impl LedgerWriter {
    pub(crate) fn new(
        cache_path: &Path,
        session_id: String,
        repo: String,
        optimization_tags: Vec<String>,
    ) -> Self {
        Self::with_max_bytes(
            cache_path,
            session_id,
            repo,
            optimization_tags,
            DEFAULT_MAX_LEDGER_BYTES,
        )
    }

    fn with_max_bytes(
        cache_path: &Path,
        session_id: String,
        repo: String,
        optimization_tags: Vec<String>,
        max_bytes: u64,
    ) -> Self {
        Self {
            session_id,
            repo,
            agent: std::env::var(TOKENZERO_AGENT_ENV)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
            version: VersionIdentity {
                crate_version: env!("CARGO_PKG_VERSION").to_string(),
                git_describe: None,
            },
            optimization_tags,
            cumulative_visible_tokens: Mutex::new(0),
            path: ledger_path_for_cache(cache_path),
            max_bytes,
            io: Mutex::new(LedgerMode::Direct {
                open_file: None,
                accepted_record: false,
            }),
        }
    }

    /// Snapshot existing response accounting and append one record. Fail-open.
    pub(crate) fn record_response(&self, tool: &str, response: &ToolResponse) {
        let accounting = response.accounting.as_ref();
        let telemetry = response.telemetry.as_ref();
        let typed_expand_miss = telemetry
            .and_then(|value| value.pointer("/expand/fail_count"))
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0);
        if accounting.is_none() && !typed_expand_miss {
            return;
        }
        let get = |pointer: &str| {
            telemetry
                .and_then(|value| value.pointer(pointer))
                .and_then(Value::as_u64)
                .unwrap_or(0)
        };
        let visible_tokens = accounting
            .map(|accounting| u64::try_from(accounting.visible_tokens).unwrap_or(u64::MAX))
            .unwrap_or(0);
        let Ok(mut cumulative) = self.cumulative_visible_tokens.lock() else {
            return;
        };
        *cumulative = cumulative.saturating_add(visible_tokens);
        let get_bool = |pointer: &str| {
            telemetry
                .and_then(|value| value.pointer(pointer))
                .and_then(Value::as_bool)
        };
        let recovery_costs = RecoveryCosts {
            visible_tokens,
            expand_tokens: get("/expand/visible_tokens"),
            expand_count: get("/expand/count"),
            retry_count: get("/expand/retry_count"),
            fail_count: get("/expand/fail_count"),
            rho_fail: DEFAULT_RHO_FAIL,
            lambda_fail: DEFAULT_LAMBDA_FAIL,
            task_success: get_bool("/task/success"),
            anchor_recall_ok: get_bool("/task/anchor_recall_ok"),
            dangling_ref_count: get("/expand/dangling_ref_count"),
            ratc: 0.0,
        }
        .with_ratc();
        let record = LedgerRecord {
            schema: LEDGER_SCHEMA_V2.to_string(),
            timestamp_ms: now_ms(),
            session_id: self.session_id.clone(),
            repo: self.repo.clone(),
            agent: self.agent.clone(),
            version: self.version.clone(),
            tool: tool.to_string(),
            token_mass: TokenMass {
                visible_tokens,
                raw_tokens: accounting
                    .map(|accounting| u64::try_from(accounting.raw_tokens).unwrap_or(u64::MAX))
                    .unwrap_or(0),
                prevented_tokens: get("/dedup/visible_tokens_saved")
                    .saturating_add(get("/diff/visible_tokens_saved")),
                saved_bytes: get("/session_delta/saved_bytes"),
            },
            eviction_amortization: telemetry
                .and_then(|value| value.pointer("/working_set_eviction/amortized"))
                .cloned(),
            cumulative_session_cost_tokens: *cumulative,
            optimization_tags: self.optimization_tags.clone(),
            recovery_costs,
        };
        let _ = self.append_record(&record);
    }

    fn append_record(&self, record: &LedgerRecord) -> io::Result<()> {
        let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
        line.push(b'\n');
        let mut mode = self
            .io
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let LedgerMode::Buffered(io) = &*mode {
            return io.append(line);
        }
        let LedgerMode::Direct {
            open_file,
            accepted_record,
        } = &mut *mode
        else {
            unreachable!()
        };
        if !*accepted_record {
            *accepted_record = true;
            if write_bytes_locked(&self.path, self.max_bytes, open_file, &line).is_ok() {
                return Ok(());
            }
            // A failed first write is retained and retried on the bounded timer.
        } else if line.len() >= LEDGER_FLUSH_BYTES {
            return write_bytes_locked(&self.path, self.max_bytes, open_file, &line);
        }
        let io = Arc::new(LedgerIo {
            path: self.path.clone(),
            max_bytes: self.max_bytes,
            state: Mutex::new(LedgerIoState {
                open_file: open_file.take(),
                write_buf: line,
                buffered_at: Some(Instant::now()),
            }),
        });
        *mode = LedgerMode::Buffered(Arc::clone(&io));
        drop(mode);
        match register_flush_target(&io) {
            Ok(()) => Ok(()),
            Err(error) => {
                io.flush()?;
                Err(error)
            }
        }
    }

    /// Drain buffered records during an orderly lifecycle shutdown. Fail-open.
    pub(crate) fn flush(&self) {
        let mode = self
            .io
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let LedgerMode::Buffered(io) = &*mode {
            let _ = io.flush();
        }
    }
}

impl LedgerIo {
    fn append(&self, line: Vec<u8>) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if line.len() >= LEDGER_FLUSH_BYTES {
            self.flush_locked(&mut state)?;
            return write_bytes_locked(&self.path, self.max_bytes, &mut state.open_file, &line);
        }
        if state.write_buf.len().saturating_add(line.len()) > LEDGER_FLUSH_BYTES {
            self.flush_locked(&mut state)?;
        }
        let starts_flush_window = state.write_buf.is_empty();
        if starts_flush_window {
            state.buffered_at = Some(Instant::now());
        }
        if starts_flush_window && state.write_buf.capacity() == 0 {
            state.write_buf = line;
        } else {
            state.write_buf.extend_from_slice(&line);
        }
        drop(state);
        if starts_flush_window {
            if let Err(error) = wake_flush_scheduler() {
                self.flush()?;
                return Err(error);
            }
        }
        Ok(())
    }

    fn flush(&self) -> io::Result<()> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.flush_locked(&mut state)
    }

    fn flush_if_due(&self, now: Instant) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let due = state
            .buffered_at
            .is_some_and(|buffered_at| now.duration_since(buffered_at) >= LEDGER_FLUSH_WINDOW);
        if due && self.flush_locked(&mut state).is_err() {
            // Retain the bytes and retry on the next bounded window without
            // spinning if the filesystem is temporarily unavailable.
            state.buffered_at = Some(now);
        }
    }

    fn flush_deadline(&self) -> Option<Instant> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .buffered_at
            .map(|buffered_at| buffered_at + LEDGER_FLUSH_WINDOW)
    }

    fn flush_locked(&self, state: &mut LedgerIoState) -> io::Result<()> {
        if state.write_buf.is_empty() {
            state.buffered_at = None;
            return Ok(());
        }
        let LedgerIoState {
            open_file,
            write_buf,
            buffered_at,
            ..
        } = state;
        write_bytes_locked(&self.path, self.max_bytes, open_file, write_buf)?;
        write_buf.clear();
        *buffered_at = None;
        Ok(())
    }
}

fn write_bytes_locked(
    path: &Path,
    max_bytes: u64,
    open_file: &mut Option<File>,
    bytes: &[u8],
) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = PathBuf::from(format!("{}.rotation.lock", path.display()));
    let rotation_lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    FileExt::lock(&rotation_lock)?;

    if open_file
        .as_ref()
        .is_some_and(|file| !open_file_matches_path(file, path))
    {
        *open_file = None;
    }
    let bytes_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let observed_len = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if observed_len > 0 && observed_len.saturating_add(bytes_len) > max_bytes {
        *open_file = None;
        rotate_ledger(path, max_bytes)?;
    }
    if open_file.is_none() {
        *open_file = Some(OpenOptions::new().create(true).append(true).open(path)?);
    }
    open_file
        .as_mut()
        .expect("ledger file just opened")
        .write_all(bytes)?;
    enforce_ledger_total_bytes(path, max_bytes)
}

#[cfg(unix)]
fn open_file_matches_path(file: &File, path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(open_metadata) = file.metadata() else {
        return false;
    };
    let Ok(path_metadata) = fs::metadata(path) else {
        return false;
    };
    open_metadata.dev() == path_metadata.dev() && open_metadata.ino() == path_metadata.ino()
}

#[cfg(not(unix))]
fn open_file_matches_path(_file: &File, _path: &Path) -> bool {
    false
}

fn flush_thread_result(result: &io::Result<std::thread::JoinHandle<()>>) -> io::Result<()> {
    result.as_ref().map(|_| ()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("failed to start ledger flush scheduler: {error}"),
        )
    })
}

fn ensure_flush_thread_started() -> io::Result<()> {
    flush_thread_result(LazyLock::force(&FLUSH_THREAD))
}

fn wake_flush_scheduler() -> io::Result<()> {
    ensure_flush_thread_started()?;
    let mut registry = FLUSH_SCHEDULER
        .registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.generation = registry.generation.wrapping_add(1);
    drop(registry);
    FLUSH_SCHEDULER.wake.notify_one();
    Ok(())
}

fn register_flush_target(target: &Arc<LedgerIo>) -> io::Result<()> {
    ensure_flush_thread_started()?;
    let mut registry = FLUSH_SCHEDULER
        .registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.targets.push(Arc::downgrade(target));
    registry.generation = registry.generation.wrapping_add(1);
    drop(registry);
    FLUSH_SCHEDULER.wake.notify_one();
    Ok(())
}

fn run_flush_scheduler() {
    let mut active = Vec::<Arc<LedgerIo>>::new();
    let mut registry = FLUSH_SCHEDULER
        .registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    loop {
        let scan_generation = registry.generation;
        active.clear();
        registry.targets.retain(|target| {
            let Some(target) = target.upgrade() else {
                return false;
            };
            active.push(target);
            true
        });
        drop(registry);

        let now = Instant::now();
        let mut next_deadline = None::<Instant>;
        for target in &active {
            target.flush_if_due(now);
            if let Some(deadline) = target.flush_deadline() {
                next_deadline = Some(
                    next_deadline
                        .map(|current| current.min(deadline))
                        .unwrap_or(deadline),
                );
            }
        }
        // Do not let the process-wide worker extend writer or file-handle lifetime.
        active.clear();

        registry = FLUSH_SCHEDULER
            .registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if registry.generation != scan_generation {
            continue;
        }
        registry = if let Some(deadline) = next_deadline {
            let timeout = deadline.saturating_duration_since(Instant::now());
            FLUSH_SCHEDULER
                .wake
                .wait_timeout(registry, timeout)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0
        } else {
            FLUSH_SCHEDULER
                .wake
                .wait(registry)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
    }
}

impl Drop for LedgerWriter {
    fn drop(&mut self) {
        self.flush();
    }
}

pub fn ledger_path_for_cache(cache_path: &Path) -> PathBuf {
    cache_path.with_file_name("ledger.jsonl")
}

fn rotated_path(path: &Path) -> PathBuf {
    rotated_path_at(path, 1)
}

fn rotated_path_at(path: &Path, generation: usize) -> PathBuf {
    path.with_extension(format!("jsonl.{generation}"))
}

fn ledger_rotation_limits(max_bytes: u64) -> (usize, u64) {
    let generations = std::env::var("TOKENZERO_LEDGER_MAX_GENERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_LEDGER_GENERATIONS);
    let total_bytes = std::env::var("TOKENZERO_LEDGER_MAX_TOTAL_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value >= max_bytes)
        .unwrap_or(DEFAULT_MAX_LEDGER_TOTAL_BYTES.max(max_bytes));
    (generations, total_bytes)
}

fn rotate_ledger(path: &Path, max_bytes: u64) -> io::Result<()> {
    let (generations, _) = ledger_rotation_limits(max_bytes);
    let _ = fs::remove_file(rotated_path_at(path, generations));
    for generation in (1..generations).rev() {
        let source = rotated_path_at(path, generation);
        let destination = rotated_path_at(path, generation + 1);
        if let Err(error) = fs::rename(source, destination)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error);
        }
    }
    if let Err(error) = fs::rename(path, rotated_path(path))
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error);
    }
    Ok(())
}

fn enforce_ledger_total_bytes(path: &Path, max_bytes: u64) -> io::Result<()> {
    let (generations, total_limit) = ledger_rotation_limits(max_bytes);
    let mut total = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    for generation in 1..=generations {
        total = total.saturating_add(
            fs::metadata(rotated_path_at(path, generation))
                .map(|metadata| metadata.len())
                .unwrap_or(0),
        );
    }
    for generation in (1..=generations).rev() {
        if total <= total_limit {
            break;
        }
        let candidate = rotated_path_at(path, generation);
        let bytes = fs::metadata(&candidate)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        match fs::remove_file(candidate) {
            Ok(()) => total = total.saturating_sub(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn append_record(path: &Path, record: &LedgerRecord, max_bytes: u64) -> io::Result<()> {
    let mut line = serde_json::to_vec(record).map_err(io::Error::other)?;
    line.push(b'\n');
    let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
    if line_bytes > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "ledger record exceeds rotation limit",
        ));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let observed_len = fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if observed_len > 0 && observed_len.saturating_add(line_bytes) > max_bytes {
        let lock_path = PathBuf::from(format!("{}.rotation.lock", path.display()));
        let rotation_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        FileExt::lock(&rotation_lock)?;
        let locked_len = fs::metadata(path)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if locked_len == observed_len
            && locked_len > 0
            && locked_len.saturating_add(line_bytes) > max_bytes
        {
            rotate_ledger(path, max_bytes)?;
        }
        let _ = FileExt::unlock(&rotation_lock);
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    enforce_ledger_total_bytes(path, max_bytes)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCostSummary {
    pub task_id: String,
    pub success: bool,
    pub visible: u64,
    pub expand: u64,
    pub retries: u64,
    pub fails: u64,
    pub ratc: f64,
    pub expand_count: u64,
    pub dangling_refs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskCostReport {
    pub schema: String,
    pub task_count: u64,
    pub successful_tasks: u64,
    pub success_rate: f64,
    pub tasks: Vec<TaskCostSummary>,
}

#[derive(Default)]
struct TaskCostAccumulator {
    visible: u64,
    expand: u64,
    retries: u64,
    fails: u64,
    ratc: f64,
    expand_count: u64,
    dangling_refs: u64,
    saw_success: bool,
    saw_failure: bool,
}

/// Group ledger v2 records by their stable session/task identity.
///
/// Until the task surface emits a narrower task id, LedgerRecord.session_id is
/// the canonical task grouping key. Unknown outcomes are conservatively not
/// successful. Every observed task is retained in the denominator, including
/// tasks with fail_count > 0.
pub fn task_cost_report(path: &Path) -> io::Result<TaskCostReport> {
    let records = read_records(path)?;
    let mut grouped = BTreeMap::<String, TaskCostAccumulator>::new();
    for record in records {
        let costs = &record.recovery_costs;
        let task = grouped.entry(record.session_id.clone()).or_default();
        let visible = if record.schema == LEDGER_SCHEMA {
            record.token_mass.visible_tokens
        } else {
            costs.visible_tokens
        };
        task.visible = task.visible.saturating_add(visible);
        task.expand = task.expand.saturating_add(costs.expand_tokens);
        task.retries = task.retries.saturating_add(costs.retry_count);
        task.fails = task.fails.saturating_add(costs.fail_count);
        task.ratc += if record.schema == LEDGER_SCHEMA {
            visible as f64
        } else {
            costs.ratc
        };
        task.expand_count = task.expand_count.saturating_add(costs.expand_count);
        task.dangling_refs = task.dangling_refs.saturating_add(costs.dangling_ref_count);
        task.saw_success |= costs.task_success == Some(true);
        task.saw_failure |= costs.task_success == Some(false) || costs.fail_count > 0;
    }

    let tasks = grouped
        .into_iter()
        .map(|(task_id, task)| TaskCostSummary {
            task_id,
            success: task.saw_success && !task.saw_failure,
            visible: task.visible,
            expand: task.expand,
            retries: task.retries,
            fails: task.fails,
            ratc: task.ratc,
            expand_count: task.expand_count,
            dangling_refs: task.dangling_refs,
        })
        .collect::<Vec<_>>();
    let task_count = u64::try_from(tasks.len()).unwrap_or(u64::MAX);
    let successful_tasks =
        u64::try_from(tasks.iter().filter(|task| task.success).count()).unwrap_or(u64::MAX);
    let success_rate = if task_count == 0 {
        0.0
    } else {
        successful_tasks as f64 / task_count as f64
    };
    Ok(TaskCostReport {
        schema: "tokenzero.task-cost-report.v1".to_owned(),
        task_count,
        successful_tasks,
        success_rate,
        tasks,
    })
}

pub fn render_task_cost_csv(report: &TaskCostReport) -> String {
    fn field(value: &str) -> String {
        if value
            .chars()
            .any(|character| matches!(character, ',' | '"' | '\n' | '\r'))
        {
            format!(r#""{}""#, value.replace('"', r#""""#))
        } else {
            value.to_owned()
        }
    }

    let mut out = String::from(
        "task_id,success,visible,expand,retries,fails,ratc,expand_count,dangling_refs\n",
    );
    for task in &report.tasks {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            field(&task.task_id),
            task.success,
            task.visible,
            task.expand,
            task.retries,
            task.fails,
            task.ratc,
            task.expand_count,
            task.dangling_refs,
        ));
    }
    out
}

pub fn write_task_cost_report(
    ledger_path: &Path,
    json_path: &Path,
    csv_path: &Path,
) -> io::Result<TaskCostReport> {
    let report = task_cost_report(ledger_path)?;
    for output in [json_path, csv_path] {
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = serde_json::to_vec_pretty(&report).map_err(io::Error::other)?;
    fs::write(json_path, json)?;
    fs::write(csv_path, render_task_cost_csv(&report))?;
    Ok(report)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerQuery {
    RepoCost {
        repo: String,
        since_ms: u64,
    },
    VersionDelta {
        baseline: String,
        candidate: String,
        since_ms: u64,
    },
    AgentSpend {
        since_ms: u64,
    },
}

/// Scan and aggregate the bounded JSONL ledger. Malformed/torn lines are ignored.
pub fn query_ledger(path: &Path, query: &LedgerQuery) -> io::Result<Value> {
    let records = read_records(path)?;
    let since = |ms: u64| records.iter().filter(move |r| r.timestamp_ms >= ms);
    match query {
        LedgerQuery::RepoCost { repo, since_ms } => {
            let (turns, visible, raw, prevented) = since(*since_ms)
                .filter(|r| r.repo == *repo)
                .fold((0_u64, 0_u64, 0_u64, 0_u64), |(t, v, raw, p), r| {
                    (
                        t + 1,
                        v.saturating_add(r.token_mass.visible_tokens),
                        raw.saturating_add(r.token_mass.raw_tokens),
                        p.saturating_add(r.token_mass.prevented_tokens),
                    )
                });
            Ok(json!({
                "schema": LEDGER_SCHEMA_V2,
                "query": "cost_per_repo",
                "repo": repo,
                "since_ms": since_ms,
                "turns": turns,
                "visible_cost_tokens": visible,
                "raw_tokens": raw,
                "prevented_tokens": prevented
            }))
        }
        LedgerQuery::VersionDelta {
            baseline,
            candidate,
            since_ms,
        } => {
            let mut totals = BTreeMap::<&str, u64>::new();
            for r in since(*since_ms) {
                let total = totals.entry(r.version.crate_version.as_str()).or_default();
                *total = total.saturating_add(r.token_mass.visible_tokens);
            }
            let baseline_cost = totals.get(baseline.as_str()).copied().unwrap_or(0);
            let candidate_cost = totals.get(candidate.as_str()).copied().unwrap_or(0);
            Ok(json!({
                "schema": LEDGER_SCHEMA_V2,
                "query": "version_delta",
                "since_ms": since_ms,
                "baseline": {"version": baseline, "visible_cost_tokens": baseline_cost},
                "candidate": {"version": candidate, "visible_cost_tokens": candidate_cost},
                "delta_visible_cost_tokens": i128::from(candidate_cost) - i128::from(baseline_cost)
            }))
        }
        LedgerQuery::AgentSpend { since_ms } => {
            let mut totals = BTreeMap::<&str, (u64, u64)>::new();
            for r in since(*since_ms) {
                let total = totals
                    .entry(r.agent.as_deref().unwrap_or("<unknown>"))
                    .or_default();
                total.0 = total.0.saturating_add(1);
                total.1 = total.1.saturating_add(r.token_mass.visible_tokens);
            }
            let agents = totals
                .into_iter()
                .map(|(agent, (turns, visible_cost_tokens))| {
                    json!({
                        "agent": agent,
                        "turns": turns,
                        "visible_cost_tokens": visible_cost_tokens
                    })
                })
                .collect::<Vec<_>>();
            Ok(json!({
                "schema": LEDGER_SCHEMA_V2,
                "query": "per_agent_spend",
                "since_ms": since_ms,
                "agents": agents
            }))
        }
    }
}

fn read_records(path: &Path) -> io::Result<Vec<LedgerRecord>> {
    let mut records = Vec::new();
    let (generations, _) = ledger_rotation_limits(DEFAULT_MAX_LEDGER_BYTES);
    let mut candidates = (1..=generations)
        .rev()
        .map(|generation| rotated_path_at(path, generation))
        .collect::<Vec<_>>();
    candidates.push(path.to_path_buf());
    for candidate in candidates {
        let file = match fs::File::open(candidate) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        for line in BufReader::new(file).lines() {
            let Ok(line) = line else { continue };
            let Ok(record) = serde_json::from_str::<LedgerRecord>(&line) else {
                continue;
            };
            // v1 lines parse via serde(default) recovery_costs; both schema
            // tags are accepted so mixed-generation ledgers stay queryable.
            if record.schema == LEDGER_SCHEMA || record.schema == LEDGER_SCHEMA_V2 {
                records.push(record);
            }
        }
    }
    Ok(records)
}

// Shareable usage telemetry lives in `usage_telemetry` (opt-in, three-field only).
pub use crate::config::{TELEMETRY_ENV, resolve_telemetry, telemetry_env_enabled};
pub use crate::usage_telemetry::{
    ExecutionPath, TelemetryInspection, UsageRecord, inspect_usage_telemetry as inspect_telemetry,
    usage_telemetry_path_for_cache,
};

/// Summarize every ledger entry's raw and prevented token mass.
pub fn aggregate_token_mass(path: &Path) -> io::Result<(u64, u64)> {
    let records = read_records(path)?;
    let (raw, saved) = records.iter().fold((0_u64, 0_u64), |(raw, saved), record| {
        (
            raw.saturating_add(record.token_mass.raw_tokens),
            saved.saturating_add(record.token_mass.prevented_tokens),
        )
    });
    Ok((raw, saved))
}

pub fn schema_example() -> Value {
    json!({
        "schema": LEDGER_SCHEMA_V2,
        "timestamp_ms": 1_700_000_000_000_u64,
        "session_id": "session-123",
        "repo": "/workspace/repo",
        "agent": null,
        "version": {"crate": env!("CARGO_PKG_VERSION"), "git_describe": null},
        "tool": "read",
        "token_mass": {
            "visible_tokens": 120,
            "raw_tokens": 400,
            "prevented_tokens": 80,
            "saved_bytes": 1024
        },
        "eviction_amortization": {
            "p_fault": 0.25,
            "expected_rehydration_tokens": 80.0,
            "amortized_tokens_per_access": 20.0,
            "actual_rehydration_tokens": 80,
            "thrash_worst_case_tokens": 120,
            "alarm": false
        },
        "cumulative_session_cost_tokens": 120,
        "optimization_tags": ["session_dedup:on", "diff_reads:on", "tool_surface:mcp"],
        "recovery_costs": {
            "visible_tokens": 120,
            "expand_tokens": 40,
            "expand_count": 2,
            "retry_count": 1,
            "fail_count": 0,
            "rho_fail": 0.0,
            "lambda_fail": 0.0,
            "task_success": null,
            "anchor_recall_ok": null,
            "dangling_ref_count": 0,
            "ratc": 160.0
        }
    })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use tempfile::tempdir;

    fn test_writer(cache_path: &Path) -> LedgerWriter {
        LedgerWriter::with_max_bytes(
            cache_path,
            "session-test".to_owned(),
            "/workspace/repo".to_owned(),
            vec!["session_dedup:on".to_owned()],
            DEFAULT_MAX_LEDGER_BYTES,
        )
    }

    fn test_record() -> LedgerRecord {
        serde_json::from_value(schema_example()).unwrap()
    }

    fn buffered_io(writer: &LedgerWriter) -> Arc<LedgerIo> {
        let mode = writer.io.lock().unwrap();
        let LedgerMode::Buffered(io) = &*mode else {
            panic!("writer has not entered buffered mode");
        };
        Arc::clone(io)
    }

    #[test]
    fn flush_thread_spawn_failure_is_a_typed_io_error() {
        let spawn: io::Result<std::thread::JoinHandle<()>> = Err(io::Error::new(
            io::ErrorKind::OutOfMemory,
            "synthetic spawn failure",
        ));
        let error = flush_thread_result(&spawn).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::OutOfMemory);
        assert!(
            error
                .to_string()
                .contains("failed to start ledger flush scheduler")
        );
    }

    /// Golden v2 fixture: the stored ratc must equal the RATC identity
    /// computed arithmetically from the record's own fields.
    const GOLDEN_V2: &str = r#"{
        "schema": "tokenzero.ledger.v2",
        "timestamp_ms": 1700000000000,
        "session_id": "session-golden",
        "repo": "/workspace/repo",
        "agent": "claude-code",
        "version": {"crate": "1.4.0", "git_describe": null},
        "tool": "expand",
        "token_mass": {"visible_tokens": 120, "raw_tokens": 400, "prevented_tokens": 80, "saved_bytes": 1024},
        "cumulative_session_cost_tokens": 120,
        "optimization_tags": ["session_dedup:on"],
        "recovery_costs": {
            "visible_tokens": 120,
            "expand_tokens": 40,
            "expand_count": 2,
            "retry_count": 3,
            "fail_count": 1,
            "rho_fail": 2.5,
            "lambda_fail": 10.0,
            "task_success": true,
            "anchor_recall_ok": false,
            "dangling_ref_count": 1,
            "ratc": 177.5
        }
    }"#;

    #[test]
    fn golden_v2_ratc_identity_holds_arithmetically() {
        let record: LedgerRecord = serde_json::from_str(GOLDEN_V2).unwrap();
        let rc = &record.recovery_costs;
        let expected = (rc.visible_tokens + rc.expand_tokens) as f64
            + rc.rho_fail * rc.retry_count as f64
            + rc.lambda_fail * rc.fail_count as f64;
        // 120 + 40 + 2.5*3 + 10.0*1 = 177.5
        assert_eq!(expected, 177.5);
        assert_eq!(rc.ratc, expected, "stored ratc must equal the identity");
        assert_eq!(rc.compute_ratc(), rc.ratc);
        assert_eq!(record.schema, LEDGER_SCHEMA_V2);
    }

    #[test]
    fn golden_v2_round_trips_without_payload_keys() {
        let record: LedgerRecord = serde_json::from_str(GOLDEN_V2).unwrap();
        let text = serde_json::to_string(&record).unwrap();
        // Telemetry-only rule: no payload bytes in ledger records.
        for key in ["payload", "content", "bytes_b64", "text"] {
            assert!(
                !text.contains(&format!("\"{key}\"")),
                "ledger record must not carry a '{key}' key: {text}"
            );
        }
        let back: LedgerRecord = serde_json::from_str(&text).unwrap();
        assert_eq!(back, record);
    }

    #[test]
    fn v1_lines_remain_readable_with_defaulted_recovery_costs() {
        let v1_line = r#"{
            "schema": "tokenzero.ledger.v1",
            "timestamp_ms": 1700000000000,
            "session_id": "session-old",
            "repo": "/workspace/repo",
            "agent": null,
            "version": {"crate": "1.3.0", "git_describe": null},
            "tool": "read",
            "token_mass": {"visible_tokens": 10, "raw_tokens": 20, "prevented_tokens": 5, "saved_bytes": 64},
            "cumulative_session_cost_tokens": 10,
            "optimization_tags": []
        }"#;
        let record: LedgerRecord = serde_json::from_str(v1_line).unwrap();
        assert_eq!(record.schema, LEDGER_SCHEMA);
        let rc = &record.recovery_costs;
        assert_eq!(rc.expand_tokens, 0);
        assert_eq!(rc.rho_fail, DEFAULT_RHO_FAIL);
        assert_eq!(rc.task_success, None);
        assert_eq!(rc.ratc, 0.0);
        // And read_records accepts the v1 tag on disk (JSONL: one compact
        // record per line).
        let directory = tempdir().unwrap();
        let ledger = directory.path().join("ledger.jsonl");
        let compact = serde_json::to_string(&record).unwrap();
        let compact = compact.replace(LEDGER_SCHEMA_V2, LEDGER_SCHEMA);
        fs::write(&ledger, format!("{compact}\n")).unwrap();
        let records = read_records(&ledger).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].schema, LEDGER_SCHEMA);
    }

    fn task_record(
        task_id: &str,
        visible: u64,
        expand: u64,
        retries: u64,
        fails: u64,
        success: Option<bool>,
        expand_count: u64,
        dangling_refs: u64,
    ) -> LedgerRecord {
        let mut record = test_record();
        record.session_id = task_id.to_owned();
        record.token_mass.visible_tokens = visible;
        record.recovery_costs = RecoveryCosts {
            visible_tokens: visible,
            expand_tokens: expand,
            expand_count,
            retry_count: retries,
            fail_count: fails,
            rho_fail: 2.0,
            lambda_fail: 10.0,
            task_success: success,
            anchor_recall_ok: None,
            dangling_ref_count: dangling_refs,
            ratc: 0.0,
        }
        .with_ratc();
        record
    }

    #[test]
    fn task_cost_report_matches_hand_computed_json_and_csv_golden() {
        let directory = tempdir().unwrap();
        let ledger = directory.path().join("ledger.jsonl");
        let json_output = directory.path().join("reports/tasks.json");
        let csv_output = directory.path().join("reports/tasks.csv");
        let records = [
            task_record("task,a", 10, 4, 1, 0, Some(true), 1, 1),
            task_record("task,a", 6, 1, 0, 0, None, 1, 0),
            task_record("task-b", 8, 2, 1, 1, Some(true), 1, 2),
        ];
        let fixture = records
            .iter()
            .map(|record| serde_json::to_string(record).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&ledger, format!("{fixture}\n")).unwrap();
        let report = write_task_cost_report(&ledger, &json_output, &csv_output).unwrap();
        assert_eq!(
            report,
            TaskCostReport {
                schema: "tokenzero.task-cost-report.v1".to_owned(),
                task_count: 2,
                successful_tasks: 1,
                success_rate: 0.5,
                tasks: vec![
                    TaskCostSummary {
                        task_id: "task,a".to_owned(),
                        success: true,
                        visible: 16,
                        expand: 5,
                        retries: 1,
                        fails: 0,
                        ratc: 23.0,
                        expand_count: 2,
                        dangling_refs: 1
                    },
                    TaskCostSummary {
                        task_id: "task-b".to_owned(),
                        success: false,
                        visible: 8,
                        expand: 2,
                        retries: 1,
                        fails: 1,
                        ratc: 22.0,
                        expand_count: 1,
                        dangling_refs: 2
                    },
                ],
            }
        );
        let json_back: TaskCostReport =
            serde_json::from_slice(&fs::read(json_output).unwrap()).unwrap();
        assert_eq!(json_back, report);
        let expected_csv = concat!(
            "task_id,success,visible,expand,retries,fails,ratc,expand_count,dangling_refs\n",
            "\"task,a\",true,16,5,1,0,23,2,1\n",
            "task-b,false,8,2,1,1,22,1,2\n",
        );
        assert_eq!(fs::read_to_string(csv_output).unwrap(), expected_csv);
    }

    #[test]
    fn tz_evict_amortized_charge_round_trips_through_ledger() {
        let charge = test_record()
            .eviction_amortization
            .expect("eviction charge");
        assert_eq!(charge["amortized_tokens_per_access"], 20.0);
        assert_eq!(charge["thrash_worst_case_tokens"], 120);
        assert_eq!(charge["alarm"], false);
    }

    #[test]
    fn first_record_is_persisted_without_scheduler_registration() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);

        writer.append_record(&test_record()).unwrap();

        let mode = writer.io.lock().unwrap();
        let LedgerMode::Direct {
            open_file,
            accepted_record,
        } = &*mode
        else {
            panic!("one record must not allocate buffered mode");
        };
        assert!(*accepted_record);
        assert!(open_file.is_some());
        drop(mode);
        assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
    }

    #[test]
    fn typed_expand_miss_is_not_dropped_without_accounting() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);
        let mut response = ToolResponse::error(
            "expand",
            "dangling_ref",
            "dangling ref",
            Some("re-run producer".to_owned()),
        );
        response.telemetry = Some(json!({
            "expand": {
                "fail_count": 1,
                "dangling_ref_count": 1,
                "miss_kind": "dangling_ref",
            }
        }));

        writer.record_response("expand", &response);
        writer.flush();

        let records = read_records(&ledger_path).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].token_mass.visible_tokens, 0);
        assert_eq!(records[0].token_mass.raw_tokens, 0);
        assert_eq!(records[0].recovery_costs.fail_count, 1);
        assert_eq!(records[0].recovery_costs.dangling_ref_count, 1);
    }

    #[test]
    fn flush_window_boundary_is_deterministic() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);
        writer.append_record(&test_record()).unwrap();
        writer.append_record(&test_record()).unwrap();
        let io = buffered_io(&writer);
        let buffered_at = io
            .state
            .lock()
            .unwrap()
            .buffered_at
            .expect("second record is buffered");

        io.flush_if_due(buffered_at + LEDGER_FLUSH_WINDOW - Duration::from_nanos(1));
        assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
        io.flush_if_due(buffered_at + LEDGER_FLUSH_WINDOW);

        assert_eq!(
            read_records(&ledger_path).unwrap(),
            vec![test_record(), test_record()]
        );
    }

    #[test]
    fn low_volume_record_flushes_after_bounded_window() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);

        writer.append_record(&test_record()).unwrap();
        writer.append_record(&test_record()).unwrap();
        assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);

        let deadline = Instant::now() + LEDGER_FLUSH_WINDOW + Duration::from_secs(2);
        while read_records(&ledger_path).unwrap().len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            read_records(&ledger_path).unwrap(),
            vec![test_record(), test_record()]
        );
    }

    #[test]
    fn explicit_flush_drains_low_volume_record() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);

        writer.append_record(&test_record()).unwrap();
        writer.append_record(&test_record()).unwrap();
        writer.flush();

        assert_eq!(
            read_records(&ledger_path).unwrap(),
            vec![test_record(), test_record()]
        );
    }

    #[test]
    fn failed_timed_flush_is_retained_for_explicit_retry() {
        let directory = tempdir().unwrap();
        let blocked_parent = directory.path().join("blocked");
        fs::write(&blocked_parent, b"not a directory").unwrap();
        let cache_path = blocked_parent.join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);
        writer.append_record(&test_record()).unwrap();
        let io = buffered_io(&writer);
        let buffered_at = io
            .state
            .lock()
            .unwrap()
            .buffered_at
            .expect("record is buffered");

        let failed_at = buffered_at + LEDGER_FLUSH_WINDOW;
        io.flush_if_due(failed_at);
        assert_eq!(io.state.lock().unwrap().buffered_at, Some(failed_at));

        fs::remove_file(&blocked_parent).unwrap();
        fs::create_dir(&blocked_parent).unwrap();
        writer.flush();
        assert_eq!(read_records(&ledger_path).unwrap(), vec![test_record()]);
    }

    #[test]
    fn drop_flushes_low_volume_record() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let ledger_path = ledger_path_for_cache(&cache_path);
        let writer = test_writer(&cache_path);

        writer.append_record(&test_record()).unwrap();
        writer.append_record(&test_record()).unwrap();
        drop(writer);

        assert_eq!(
            read_records(&ledger_path).unwrap(),
            vec![test_record(), test_record()]
        );
    }

    #[test]
    fn retained_handle_reopens_after_external_rotation() {
        let directory = tempdir().unwrap();
        let cache_path = directory.path().join("cache.json");
        let path = ledger_path_for_cache(&cache_path);
        let rotated = rotated_path(&path);
        let first_writer = test_writer(&cache_path);

        first_writer.append_record(&test_record()).unwrap();
        fs::rename(&path, &rotated).unwrap();

        let second_writer = test_writer(&cache_path);
        second_writer.append_record(&test_record()).unwrap();
        first_writer.append_record(&test_record()).unwrap();
        first_writer.flush();

        assert_eq!(fs::read_to_string(&rotated).unwrap().lines().count(), 1);
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
    }

    #[test]
    fn concurrent_rotation_does_not_rotate_twice() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        let record: LedgerRecord = serde_json::from_value(schema_example()).unwrap();
        let line_len = serde_json::to_vec(&record).unwrap().len() as u64 + 1;
        let max_bytes = line_len + 8;
        let original = vec![b'x'; 9];
        fs::write(&path, &original).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let path = path.clone();
            let record = record.clone();
            let barrier = barrier.clone();
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                append_record(&path, &record, max_bytes).unwrap();
            }));
        }
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(fs::read(rotated_path(&path)).unwrap(), original);
        assert_eq!(read_records(&path).unwrap().len(), 2);
    }

    #[test]
    fn ledger_rotation_caps_generations_and_total_bytes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ledger.jsonl");
        for generation in 0..(DEFAULT_MAX_LEDGER_GENERATIONS + 2) {
            fs::write(&path, [generation as u8]).unwrap();
            rotate_ledger(&path, DEFAULT_MAX_LEDGER_BYTES).unwrap();
        }
        for generation in 1..=DEFAULT_MAX_LEDGER_GENERATIONS {
            assert!(rotated_path_at(&path, generation).is_file());
        }
        assert!(!rotated_path_at(&path, DEFAULT_MAX_LEDGER_GENERATIONS + 1).exists());

        fs::write(&path, [0]).unwrap();
        for generation in 1..=DEFAULT_MAX_LEDGER_GENERATIONS {
            OpenOptions::new()
                .write(true)
                .open(rotated_path_at(&path, generation))
                .unwrap()
                .set_len(10 * 1024 * 1024)
                .unwrap();
        }
        enforce_ledger_total_bytes(&path, DEFAULT_MAX_LEDGER_BYTES).unwrap();
        let total = std::iter::once(path.clone())
            .chain(
                (1..=DEFAULT_MAX_LEDGER_GENERATIONS)
                    .map(|generation| rotated_path_at(&path, generation)),
            )
            .filter_map(|candidate| fs::metadata(candidate).ok())
            .map(|metadata| metadata.len())
            .sum::<u64>();
        assert!(total <= DEFAULT_MAX_LEDGER_TOTAL_BYTES);
    }
}
