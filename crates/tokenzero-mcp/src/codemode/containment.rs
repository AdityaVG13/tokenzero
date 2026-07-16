//! Hard containment for expensive CodeMode execution.
//!
//! # ZeroStack machine-wide permit contract (v1)
//!
//! Sibling engines (TokenZero, FSZero, GraphZero) and the ZeroStack hub must
//! share these paths so concurrent CodeMode processes cannot stack CPU:
//!
//! - Status/health/describe/expand-only recovery: ungated
//! - Analysis (light find/search/plans): `/tmp/zerostack-codemode-analysis.permit`
//! - Index (rebuild / watch.drain / `.index(`): `/tmp/zerostack-codemode-index.permit`
//! - Heavy (shell / high-cost): `/tmp/zerostack-codemode-heavy.permit`
//!
//! Contention waits then returns retryable `busy` / `machine_permit_busy` —
//! never a silent ok while a permit is held. Fatal permit I/O (EACCES, etc.)
//! maps to non-retryable `substrate` / `machine_permit_io`.
//!
//! ## Multi-tenant default (100 sessions)
//!
//! Analysis concurrency defaults to `max(1, cores/4)` soft-capped at 8.
//! Index concurrency defaults to `max(1, cores/8)` soft-capped at 2.
//! Hundreds of sessions share those slot pools; each active holder is
//! expected to use about one core because search backends are thread-capped
//! (`rg --threads 1`). Override with the matching
//! `TOKENZERO_CODEMODE_*_CONCURRENCY` / `*_CONCURRENCY_CAP` env vars.
//!
//! Canonical doc: `CODEMODE_MACHINE_PERMITS.md` (`tokenzero-npia`,
//! `tokenzero-qisj`, `fszero-gzw`, `graphzero-01vw`).

use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::{CodeModeOptions, CodeModeResult};

const DEFAULT_MAX_ACTIVE: usize = 1;
const DEFAULT_MAX_QUEUE_DEPTH: usize = 8;
const DEFAULT_COST_THRESHOLD: usize = 32;
const DEFAULT_ANALYSIS_CONCURRENCY_CAP: usize = 8;
const DEFAULT_INDEX_CONCURRENCY_CAP: usize = 2;
const DEFAULT_MACHINE_PERMIT: &str = "/tmp/zerostack-codemode-heavy.permit";
const DEFAULT_ANALYSIS_PERMIT: &str = "/tmp/zerostack-codemode-analysis.permit";
const DEFAULT_INDEX_PERMIT: &str = "/tmp/zerostack-codemode-index.permit";
const PERMIT_POLL: Duration = Duration::from_millis(20);
const PERMIT_POLL_MAX: Duration = Duration::from_millis(200);
const INCOMPLETE_PERMIT_GRACE: Duration = Duration::from_millis(250);
const SNAPSHOT_PLANS: &[&str] = &["status", "codemode.status", "containment.status"];
const CONFIG_LIMITS: [(&str, usize, usize); 3] = [
    (
        "TOKENZERO_CODEMODE_HEAVY_CONCURRENCY",
        DEFAULT_MAX_ACTIVE,
        1,
    ),
    (
        "TOKENZERO_CODEMODE_HEAVY_QUEUE_DEPTH",
        DEFAULT_MAX_QUEUE_DEPTH,
        0,
    ),
    (
        "TOKENZERO_CODEMODE_HEAVY_COST_THRESHOLD",
        DEFAULT_COST_THRESHOLD,
        1,
    ),
];

thread_local! {
    static HEAVY_EXECUTION_ID: Cell<Option<u64>> = const { Cell::new(None) };
}

static NEXT_EXECUTION_ID: AtomicU64 = AtomicU64::new(1);

struct HeavyExecutionGuard(u64);

impl HeavyExecutionGuard {
    fn enter() -> Self {
        let id = NEXT_EXECUTION_ID.fetch_add(1, Ordering::Relaxed);
        HEAVY_EXECUTION_ID.with(|cell| cell.set(Some(id)));
        HeavyExecutionGuard(id)
    }
}

impl Drop for HeavyExecutionGuard {
    fn drop(&mut self) {
        HEAVY_EXECUTION_ID.with(|cell| cell.set(None));
    }
}

fn heavy_execution_id() -> Option<u64> {
    HEAVY_EXECUTION_ID.with(Cell::get)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionClass {
    Status,
    Light,
    Index,
    HeavyShell,
    HeavyEstimatedCost,
}
#[derive(Debug, Clone, Copy)]
struct BackgroundChild(u64, Option<u32>, bool);

#[derive(Debug, Default)]
struct State {
    active_heavy: usize,
    queue_depth: usize,
    rejected_count: u64,
    active_started: Option<Instant>,
    operation_class: Option<ExecutionClass>,
    child_pid: Option<u32>,
    child_pgid: Option<u32>,
    cancellation_state: Option<&'static str>,
    background_jobs: HashMap<String, BackgroundChild>,
    flights: HashMap<String, Arc<Flight>>,
}
#[derive(Debug, Default)]
struct Flight {
    result: Mutex<Option<CodeModeResult>>,
    ready: Condvar,
    followers: AtomicUsize,
}

#[derive(Debug, Clone)]
struct Config {
    max_active: usize,
    max_queue_depth: usize,
    cost_threshold: usize,
    analysis_max_active: usize,
    index_max_active: usize,
}
impl Default for Config {
    fn default() -> Self {
        let [max_active, max_queue_depth, cost_threshold] =
            CONFIG_LIMITS.map(|(name, default, minimum)| env_usize(name, default).max(minimum));
        let analysis_cap = env_usize(
            "TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP",
            DEFAULT_ANALYSIS_CONCURRENCY_CAP,
        )
        .max(1);
        let index_cap = env_usize(
            "TOKENZERO_CODEMODE_INDEX_CONCURRENCY_CAP",
            DEFAULT_INDEX_CONCURRENCY_CAP,
        )
        .max(1);
        Self {
            max_active,
            max_queue_depth,
            cost_threshold,
            analysis_max_active: env_usize_or_else(
                "TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY",
                default_analysis_concurrency,
            )
            .max(1)
            .min(analysis_cap),
            index_max_active: env_usize_or_else(
                "TOKENZERO_CODEMODE_INDEX_CONCURRENCY",
                default_index_concurrency,
            )
            .max(1)
            .min(index_cap),
        }
    }
}
#[derive(Debug)]
struct Controller {
    config: Config,
    state: Mutex<State>,
    capacity: Condvar,
}
static CONTROLLER: OnceLock<Controller> = OnceLock::new();
fn controller() -> &'static Controller {
    CONTROLLER.get_or_init(|| Controller::new(Config::default()))
}

pub(crate) fn execute<F>(plan: &str, options: &CodeModeOptions, run: F) -> CodeModeResult
where
    F: FnOnce() -> CodeModeResult,
{
    controller().execute(plan, options, run)
}
pub(crate) fn snapshot() -> Value {
    controller().snapshot()
}

/// Engine-shell seam. The process runner remains the sole kill authority; this
/// only publishes bounded identity/state and is also used by synthetic tests.
pub(crate) fn note_child(pid: Option<u32>, pgid: Option<u32>, cancellation_state: &'static str) {
    if heavy_execution_id().is_some() {
        controller().note_child(pid, pgid, cancellation_state);
    }
}

pub(crate) fn reserve_background_job(id: &str) {
    if let Some(execution_id) = heavy_execution_id() {
        controller().reserve_background_job(id, execution_id);
    }
}

pub(crate) fn note_background_child(id: &str, _pid: Option<u32>, pgid: Option<u32>) {
    controller().note_background_child(id, pgid);
}

pub(crate) fn finish_background_job(id: &str) {
    controller().lock().background_jobs.remove(id);
}

impl Controller {
    fn new(config: Config) -> Self {
        Self {
            config,
            state: Mutex::new(State::default()),
            capacity: Condvar::new(),
        }
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn note_child(&self, pid: Option<u32>, pgid: Option<u32>, cancellation_state: &'static str) {
        let mut state = self.lock();
        if state.active_heavy > 0 {
            state.child_pid = pid;
            state.child_pgid = pgid;
            state.cancellation_state = Some(cancellation_state);
        }
    }
    fn reserve_background_job(&self, id: &str, execution_id: u64) {
        self.lock()
            .background_jobs
            .insert(id.to_string(), BackgroundChild(execution_id, None, false));
    }
    fn note_background_child(&self, id: &str, pgid: Option<u32>) {
        let cancelled = {
            let mut state = self.lock();
            match state.background_jobs.get_mut(id) {
                Some(job) if job.2 => {
                    state.background_jobs.remove(id);
                    pgid
                }
                Some(job) => {
                    job.1 = pgid;
                    None
                }
                None => None,
            }
        };
        if let Some(pgid) = cancelled {
            terminate_owned_process_group(pgid);
        }
    }
    fn cancel_background_jobs(&self, execution_id: u64) {
        let pgids = {
            let mut state = self.lock();
            let mut pgids = Vec::new();
            state.background_jobs.retain(|_, job| {
                if job.0 != execution_id {
                    return true;
                }
                job.2 = true;
                match job.1 {
                    Some(pgid) => {
                        pgids.push(pgid);
                        false
                    }
                    None => true,
                }
            });
            pgids
        };
        for pgid in pgids {
            terminate_owned_process_group(pgid);
        }
    }

    fn execute<F>(&self, plan: &str, options: &CodeModeOptions, run: F) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        let plan_trimmed = plan.trim();
        let normalized = plan_trimmed.to_ascii_lowercase();
        if SNAPSHOT_PLANS.contains(&normalized.as_str()) {
            return CodeModeResult::completed(self.snapshot(), Vec::new(), 0, 0, 0);
        }
        let class = classify(plan_trimmed, self.config.cost_threshold);
        if class == ExecutionClass::Status {
            return run();
        }
        let key = dedup_key(plan, options);
        let (flight, leader) = {
            let mut s = self.lock();
            if let Some(f) = s.flights.get(&key) {
                let followers = f.followers.fetch_add(1, Ordering::AcqRel);
                if followers >= self.config.max_queue_depth {
                    f.followers.fetch_sub(1, Ordering::AcqRel);
                    s.rejected_count = s.rejected_count.saturating_add(1);
                    return busy_result(
                        "dedup_followers_full",
                        "bounded identical-execution follower set is full; retry with backoff",
                    );
                }
                (Arc::clone(f), false)
            } else {
                let f = Arc::new(Flight::default());
                s.flights.insert(key.clone(), Arc::clone(&f));
                (f, true)
            }
        };
        if !leader {
            let mut result = flight.result.lock().unwrap_or_else(|p| p.into_inner());
            while result.is_none() {
                result = flight.ready.wait(result).unwrap_or_else(|p| p.into_inner());
            }
            let result = result.as_ref().expect("completed flight").clone();
            flight.followers.fetch_sub(1, Ordering::AcqRel);
            return result;
        }
        let result = match class {
            ExecutionClass::Status => catch_worker_panic(run),
            ExecutionClass::Light => self.run_analysis(class, options, run),
            ExecutionClass::Index => self.run_index(class, options, run),
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost => {
                self.run_heavy(class, options, run)
            }
        };
        *flight.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(result.clone());
        flight.ready.notify_all();
        self.lock().flights.remove(&key);
        result
    }

    fn run_analysis<F>(
        &self,
        class: ExecutionClass,
        options: &CodeModeOptions,
        run: F,
    ) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        // Analysis uses the shared machine permit so N concurrent TokenZero
        // (and later FSZero/GraphZero) processes cannot each burn a core on
        // find/search at once. Share the in-process heavy slot so one process
        // also cannot overlap analysis with shell work.
        let slot = match self.acquire_slot(class) {
            Ok(v) => v,
            Err(v) => return v,
        };
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(options.hard_max_wall_ms.max(1)))
            .unwrap_or_else(Instant::now);
        let permit = match MachinePermit::acquire_slots(
            &analysis_permit_path(),
            self.config.analysis_max_active,
            deadline,
            "tokenzero-codemode-analysis",
        ) {
            Ok(v) => v,
            Err(e) => {
                drop(slot);
                return map_acquire_error(e);
            }
        };
        let result = catch_worker_panic(run);
        drop(slot);
        drop(permit);
        result
    }

    fn run_index<F>(
        &self,
        class: ExecutionClass,
        options: &CodeModeOptions,
        run: F,
    ) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        // Index rebuild / drain work shares a tighter family-wide slot pool
        // than analysis so concurrent engines cannot stack index CPU.
        let slot = match self.acquire_slot(class) {
            Ok(v) => v,
            Err(v) => return v,
        };
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(options.hard_max_wall_ms.max(1)))
            .unwrap_or_else(Instant::now);
        let permit = match MachinePermit::acquire_slots(
            &index_permit_path(),
            self.config.index_max_active,
            deadline,
            "tokenzero-codemode-index",
        ) {
            Ok(v) => v,
            Err(e) => {
                drop(slot);
                return map_acquire_error(e);
            }
        };
        let result = catch_worker_panic(run);
        drop(slot);
        drop(permit);
        result
    }

    fn run_heavy<F>(
        &self,
        class: ExecutionClass,
        options: &CodeModeOptions,
        run: F,
    ) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        let slot = match self.acquire_slot(class) {
            Ok(v) => v,
            Err(v) => return v,
        };
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(options.hard_max_wall_ms.max(1)))
            .unwrap_or_else(Instant::now);
        let permit = match MachinePermit::acquire_slots(
            &heavy_permit_path(),
            self.config.max_active,
            deadline,
            "tokenzero-codemode-heavy",
        ) {
            Ok(v) => v,
            Err(e) => {
                drop(slot);
                return map_acquire_error(e);
            }
        };
        let result = {
            let guard = HeavyExecutionGuard::enter();
            let result = catch_worker_panic(run);
            if result.error.is_some() {
                self.cancel_background_jobs(guard.0);
            }
            result
        };
        drop(slot);
        drop(permit);
        result
    }
    #[allow(clippy::result_large_err)]
    fn acquire_slot(&self, class: ExecutionClass) -> Result<HeavySlot<'_>, CodeModeResult> {
        let mut s = self.lock();
        if s.active_heavy >= self.config.max_active {
            if s.queue_depth >= self.config.max_queue_depth {
                s.rejected_count = s.rejected_count.saturating_add(1);
                return Err(busy_result(
                    "heavy_queue_full",
                    "bounded CodeMode heavy queue is full; retry with backoff",
                ));
            }
            s.queue_depth += 1;
            while s.active_heavy >= self.config.max_active {
                s = self.capacity.wait(s).unwrap_or_else(|p| p.into_inner());
            }
            s.queue_depth -= 1;
        }
        s.active_heavy += 1;
        s.active_started = Some(Instant::now());
        s.operation_class = Some(class);
        s.child_pid = None;
        s.child_pgid = None;
        s.cancellation_state = Some("not_cancelled");
        Ok(HeavySlot(self))
    }
    fn snapshot(&self) -> Value {
        let s = self.lock();
        json!({
            "queue_depth": s.queue_depth,
            "active_heavy": s.active_heavy,
            "worker_count": s.active_heavy,
            "analysis_max_active": self.config.analysis_max_active,
            "index_max_active": self.config.index_max_active,
            "heavy_max_active": self.config.max_active,
            "child_pid": s.child_pid,
            "child_pgid": s.child_pgid,
            "operation_class": s.operation_class,
            "elapsed_ms": s.active_started.map(|v| u64::try_from(v.elapsed().as_millis()).unwrap_or(u64::MAX)),
            "cancellation_state": s.cancellation_state.unwrap_or("none"),
            "background_jobs": s.background_jobs.len(),
            "rejected_count": s.rejected_count,
        })
    }
}
struct HeavySlot<'a>(&'a Controller);
impl Drop for HeavySlot<'_> {
    fn drop(&mut self) {
        let mut s = self.0.lock();
        let owned_pgid = (s.cancellation_state == Some("running"))
            .then_some(s.child_pgid)
            .flatten();
        s.active_heavy = s.active_heavy.saturating_sub(1);
        if s.active_heavy == 0 {
            s.active_started = None;
            s.operation_class = None;
            s.child_pid = None;
            s.child_pgid = None;
            s.cancellation_state = None;
        }
        drop(s);
        if let Some(pgid) = owned_pgid {
            terminate_owned_process_group(pgid);
        }
        self.0.capacity.notify_one();
    }
}

#[cfg(unix)]
fn terminate_owned_process_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    let target = format!("-{pgid}");
    for signal in ["-TERM", "-KILL"] {
        let _ = std::process::Command::new("kill")
            .args([signal, "--", &target])
            .status();
        if signal == "-TERM" {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
#[cfg(not(unix))]
fn terminate_owned_process_group(_: u32) {}

#[derive(Debug)]
struct MachinePermit(PathBuf, String);
impl MachinePermit {
    fn acquire_slots(
        base: &Path,
        slots: usize,
        deadline: Instant,
        command: &str,
    ) -> Result<Self, AcquireError> {
        // Always use base/slot-N — even when slots==1 — so mixed concurrency
        // envs cannot stack an exclusive base lock with slot children.
        // Pool size is the caller's requested budget (from env); do not freeze
        // capacity to the first asker — that would let CONCURRENCY=1 starve the
        // family-wide cores/4 analysis budget.
        let slots = slots.max(1);
        let mut attempt = 0u32;
        loop {
            if !legacy_exclusive_busy(base) {
                let _ = fs::create_dir_all(base);
                for idx in 0..slots {
                    let path = base.join(format!("slot-{idx}"));
                    match Self::try_create(&path, command) {
                        Ok(permit) => return Ok(permit),
                        Err(TryPermit::Busy) => {}
                        Err(TryPermit::Fatal(e)) => return Err(AcquireError::Fatal(e)),
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(AcquireError::Busy(format!(
                    "codemode permit {} is held by live process(es) across {slots} slots",
                    base.display()
                )));
            }
            // Back off under multi-waiter pressure so 100 idle sessions do not
            // wake-storm the slot directory every 20ms.
            let sleep_for = permit_backoff(attempt)
                .min(deadline.saturating_duration_since(Instant::now()));
            attempt = attempt.saturating_add(1);
            std::thread::sleep(sleep_for);
        }
    }

    fn acquire(path: &Path, deadline: Instant, command: &str) -> Result<Self, AcquireError> {
        let mut attempt = 0u32;
        loop {
            match Self::try_create(path, command) {
                Ok(permit) => return Ok(permit),
                Err(TryPermit::Busy) => {
                    if Instant::now() >= deadline {
                        return Err(AcquireError::Busy(format!(
                            "codemode permit {} is held by a live process",
                            path.display()
                        )));
                    }
                    let sleep_for = permit_backoff(attempt)
                        .min(deadline.saturating_duration_since(Instant::now()));
                    attempt = attempt.saturating_add(1);
                    std::thread::sleep(sleep_for);
                }
                Err(TryPermit::Fatal(e)) => return Err(AcquireError::Fatal(e)),
            }
        }
    }

    fn try_create(path: &Path, command: &str) -> Result<Self, TryPermit> {
        match fs::create_dir(path) {
            Ok(()) => {
                let owner = format!(
                    "{}-{}-{:?}",
                    std::process::id(),
                    epoch_millis(),
                    std::thread::current().id()
                );
                if let Err(e) = write_metadata(path, &owner, command) {
                    cleanup_owned(path, &owner);
                    return Err(TryPermit::Fatal(format!(
                        "write codemode permit metadata: {e}"
                    )));
                }
                Ok(Self(path.to_path_buf(), owner))
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if reclaim_dead(path) {
                    return Self::try_create(path, command);
                }
                Err(TryPermit::Busy)
            }
            Err(e) => Err(TryPermit::Fatal(format!(
                "create codemode permit {}: {e}",
                path.display()
            ))),
        }
    }
}
enum TryPermit {
    Busy,
    Fatal(String),
}

#[derive(Debug)]
enum AcquireError {
    /// Live holder(s) still hold the permit after the wall deadline.
    Busy(String),
    /// Non-retryable I/O / policy failure creating the permit (EACCES, etc.).
    Fatal(String),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(message) | Self::Fatal(message) => f.write_str(message),
        }
    }
}

impl Drop for MachinePermit {
    fn drop(&mut self) {
        cleanup_owned(&self.0, &self.1)
    }
}
fn write_metadata(path: &Path, owner: &str, command: &str) -> std::io::Result<()> {
    // Write ownership first so an error in any later metadata write remains
    // removable by the acquiring RAII guard.
    fs::write(path.join("owner"), owner)?;
    fs::write(path.join("pid"), std::process::id().to_string())?;
    fs::write(
        path.join("repository"),
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .chars()
            .take(1024)
            .collect::<String>(),
    )?;
    fs::write(path.join("command"), command)?;
    fs::write(path.join("started_at"), epoch_millis().to_string())
}
const PERMIT_METADATA: &[&str] = &["pid", "repository", "command", "started_at", "owner"];
fn remove_permit(path: &Path) -> bool {
    for name in PERMIT_METADATA {
        let _ = fs::remove_file(path.join(name));
    }
    fs::remove_dir(path).is_ok()
}
fn cleanup_owned(path: &Path, owner: &str) {
    if fs::read_to_string(path.join("owner")).ok().as_deref() == Some(owner) {
        remove_permit(path);
    }
}
fn reclaim_dead(path: &Path) -> bool {
    let pid = fs::read_to_string(path.join("pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok());
    if let Some(pid) = pid {
        return !process_alive(pid) && remove_permit(path);
    }

    // A process can die after create_dir() but before writing pid. Without a
    // bounded incomplete-state recovery, that empty permit blocks every
    // CodeMode client forever. The grace period avoids racing a live writer.
    let stale = fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= INCOMPLETE_PERMIT_GRACE);
    stale && remove_permit(path)
}

/// Legacy exclusive layout put `pid`/`owner` directly under `base`. Slot layout
/// keeps metadata only under `base/slot-N`.
fn looks_like_legacy_exclusive_permit(base: &Path) -> bool {
    base.is_dir() && (base.join("pid").is_file() || base.join("owner").is_file())
}

/// If `base` is a live legacy exclusive permit, reclaim dead holders; otherwise
/// treat every slot as Busy so peers cannot create `slot-N` children underneath.
fn legacy_exclusive_busy(base: &Path) -> bool {
    if !looks_like_legacy_exclusive_permit(base) {
        return false;
    }
    let _ = reclaim_dead(base);
    looks_like_legacy_exclusive_permit(base)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|s| s.success())
}
#[cfg(not(unix))]
fn process_alive(_: u32) -> bool {
    true
}
fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_millis())
}
fn catch_worker_panic(run: impl FnOnce() -> CodeModeResult) -> CodeModeResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
        CodeModeResult::error_with_kind(
            "containment_panic",
            "contained CodeMode worker panicked; permits and queue slots were released",
            0,
            false,
        )
    })
}
/// Live-holder timeout → retryable busy; Fatal I/O (EACCES etc.) → substrate.
fn map_acquire_error(err: AcquireError) -> CodeModeResult {
    match err {
        AcquireError::Busy(message) => busy_result("machine_permit_busy", &message),
        AcquireError::Fatal(message) => substrate_result("machine_permit_io", &message),
    }
}

fn busy_result(code: &str, message: &str) -> CodeModeResult {
    let mut r = CodeModeResult::error_with_kind("busy", format!("{code}: {message}"), 0, true);
    r.telemetry.extra = Some(
        json!({"backpressure":{"class":"busy","code":code,"retryable":true,"retry_strategy":"exponential_backoff"}}),
    );
    r
}

fn substrate_result(code: &str, message: &str) -> CodeModeResult {
    CodeModeResult::error_with_kind("substrate", format!("{code}: {message}"), 0, false)
}
const STATUS_PREFIXES: &[&str] = &["search:", "describe:"];
/// API-shaped catalog markers only — bare "status"/"metrics" escape the gate.
const STATUS_MARKERS: &[&str] = &[
    "codemode.limits",
    "codemode.status",
    "containment.status",
    "journaldoctor",
    "journal_doctor",
];
const SHELL_MARKERS: &[&str] = &[".shell(", "tz_shell", "\"shell\"", "'shell'"];
/// API-shaped index markers only — bare "rebuild" must not steal the scarce pool.
const INDEX_MARKERS: &[&str] = &[".index(", "watch.drain"];
/// API-shaped expand / expandMany markers — ref materialization, not analysis.
const EXPAND_MARKERS: &[&str] = &[
    "zero.token.expand",
    "zero.expand",
    ".expand(",
    "zero.token.expandmany",
    "zero.expandmany",
    ".expandmany(",
    "expand_many(",
];
/// When present alongside expand, keep the plan analysis-gated (find/search/FS).
const ANALYSIS_WORK_MARKERS: &[&str] = &[
    ".find(",
    ".grep(",
    ".glob(",
    ".tree(",
    ".read(",
    ".compound(",
    ".blast(",
    ".orient(",
    ".callers(",
    ".delta(",
    "zero.token.find",
    "zero.find",
    "zero.token.grep",
    "zero.grep",
];

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

/// Expand-only recovery: materialize `tz://` / `fz://` / `gz://` refs without
/// holding the machine-wide analysis permit (tokenzero-wawf). Mixed expand+find
/// plans stay Light so search still shares the analysis slot pool.
fn is_expand_recovery_plan(p: &str) -> bool {
    contains_any(p, EXPAND_MARKERS) && !contains_any(p, ANALYSIS_WORK_MARKERS)
}

fn classify(plan: &str, cost_threshold: usize) -> ExecutionClass {
    let p = plan.trim().to_ascii_lowercase();
    if STATUS_PREFIXES.iter().any(|prefix| p.starts_with(prefix))
        || contains_any(&p, STATUS_MARKERS)
    {
        return ExecutionClass::Status;
    }
    if contains_any(&p, SHELL_MARKERS) {
        return ExecutionClass::HeavyShell;
    }
    if contains_any(&p, INDEX_MARKERS) {
        return ExecutionClass::Index;
    }
    if is_expand_recovery_plan(&p) {
        return ExecutionClass::Status;
    }
    let cost = p.matches("zero.").count() + p.matches("tz_").count() + p.matches("method").count();
    if cost > cost_threshold {
        ExecutionClass::HeavyEstimatedCost
    } else {
        ExecutionClass::Light
    }
}
fn dedup_key(plan: &str, options: &CodeModeOptions) -> String {
    let root = options.root.as_deref().unwrap_or_else(|| Path::new("."));
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut h = Sha256::new();
    h.update(root.to_string_lossy().as_bytes());
    h.update([0]);
    h.update(plan.trim().as_bytes());
    h.finalize().iter().map(|v| format!("{v:02x}")).collect()
}
fn heavy_permit_path() -> PathBuf {
    std::env::var_os("TOKENZERO_CODEMODE_HEAVY_PERMIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                let pid = std::process::id();
                std::env::temp_dir().join(format!("zerostack-codemode-heavy-test-{pid}.permit"))
            } else {
                PathBuf::from(DEFAULT_MACHINE_PERMIT)
            }
        })
}
fn analysis_permit_path() -> PathBuf {
    std::env::var_os("TOKENZERO_CODEMODE_ANALYSIS_PERMIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                let pid = std::process::id();
                std::env::temp_dir().join(format!("zerostack-codemode-analysis-test-{pid}.permit"))
            } else {
                PathBuf::from(DEFAULT_ANALYSIS_PERMIT)
            }
        })
}

fn index_permit_path() -> PathBuf {
    std::env::var_os("TOKENZERO_CODEMODE_INDEX_PERMIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                let pid = std::process::id();
                std::env::temp_dir().join(format!("zerostack-codemode-index-test-{pid}.permit"))
            } else {
                PathBuf::from(DEFAULT_INDEX_PERMIT)
            }
        })
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_usize_or_else(name: &str, default: impl FnOnce() -> usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(default)
}

/// Machine-wide analysis slot budget for multi-tenant hosts.
///
/// Default: `max(1, cores/4)` soft-capped by
/// `TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP` (default 8). One hundred
/// sessions share these slots; each active search is thread-capped so the
/// aggregate stays near the budgeted core count instead of `sessions * cores`.
pub(crate) fn default_analysis_concurrency() -> usize {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4);
    let budget = (cores / 4).max(1);
    let cap = env_usize(
        "TOKENZERO_CODEMODE_ANALYSIS_CONCURRENCY_CAP",
        DEFAULT_ANALYSIS_CONCURRENCY_CAP,
    )
    .max(1);
    budget.min(cap)
}

/// Machine-wide index slot budget for multi-tenant hosts.
///
/// Default: `max(1, cores/8)` soft-capped by
/// `TOKENZERO_CODEMODE_INDEX_CONCURRENCY_CAP` (default 2). Tighter than
/// analysis so concurrent rebuild/drain work cannot saturate the host.
pub(crate) fn default_index_concurrency() -> usize {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4);
    let budget = (cores / 8).max(1);
    let cap = env_usize(
        "TOKENZERO_CODEMODE_INDEX_CONCURRENCY_CAP",
        DEFAULT_INDEX_CONCURRENCY_CAP,
    )
    .max(1);
    budget.min(cap)
}

fn permit_backoff(attempt: u32) -> Duration {
    // 20, 40, 80, 160, 200, 200, ...
    let shift = attempt.min(4);
    let millis = (PERMIT_POLL.as_millis() as u64)
        .saturating_mul(1u64 << shift)
        .min(PERMIT_POLL_MAX.as_millis() as u64)
        .max(PERMIT_POLL.as_millis() as u64);
    Duration::from_millis(millis)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;

    #[test]
    fn reclaims_incomplete_machine_permit_after_grace() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-incomplete-permit-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("owner"), "").unwrap();
        std::thread::sleep(INCOMPLETE_PERMIT_GRACE + Duration::from_millis(20));

        assert!(reclaim_dead(&path));
        assert!(!path.exists());
    }

    #[test]
    fn analysis_permit_is_exclusive_across_threads() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-analysis-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        let barrier = Arc::new(Barrier::new(2));
        let path_holder = path.clone();
        let barrier_holder = Arc::clone(&barrier);
        let holder = thread::spawn(move || {
            let permit = MachinePermit::acquire(
                &path_holder,
                Instant::now() + Duration::from_secs(5),
                "test-analysis-holder",
            )
            .expect("holder acquires analysis permit");
            barrier_holder.wait();
            thread::sleep(Duration::from_millis(300));
            drop(permit);
        });

        barrier.wait();
        let contested = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_millis(80),
            "test-analysis-contender",
        );
        assert!(
            contested.is_err(),
            "second acquirer must not stack while holder is live: {contested:?}"
        );
        holder.join().unwrap();
        let after = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_secs(2),
            "test-analysis-after",
        );
        assert!(after.is_ok(), "permit must release for the next waiter");
    }

    #[test]
    fn light_execute_returns_busy_when_analysis_permit_held() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    deadline,
                    &format!("test-analysis-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold analysis slot {idx}/{slots}: {e}"))
            })
            .collect();

        let opts = CodeModeOptions {
            hard_max_wall_ms: 120,
            ..CodeModeOptions::default()
        };
        let result = execute(
            "return {ok:true}",
            &opts,
            || CodeModeResult::completed(json!({"ok": true}), Vec::new(), 0, 0, 0),
        );
        let err = result
            .error
            .as_ref()
            .expect("expected busy error from held analysis permit");
        assert!(
            err.retryable,
            "analysis permit contention must be retryable: {err:?}"
        );
        assert!(
            err.kind == "busy" || err.message.contains("machine_permit_busy"),
            "unexpected error: {err:?}"
        );
        drop(holders);
    }

    #[test]
    fn status_plans_bypass_analysis_permit() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let holder = MachinePermit::acquire_slots(
            &path,
            slots,
            Instant::now() + Duration::from_secs(5),
            "test-analysis-holder",
        )
        .expect("pre-hold analysis permit");

        let result = execute(
            "search: containment",
            &CodeModeOptions::default(),
            || CodeModeResult::completed(json!({"status": "ok"}), Vec::new(), 0, 0, 0),
        );
        assert!(
            result.error.is_none(),
            "status/search catalog plans must stay ungated: {result:?}"
        );
        drop(holder);
    }

    #[test]
    fn expand_only_plans_bypass_analysis_permit() {
        let path = analysis_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_analysis_concurrency().max(1);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    Instant::now() + Duration::from_secs(5),
                    &format!("test-expand-bypass-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold analysis slot {idx}/{slots}: {e}"))
            })
            .collect();

        let result = execute(
            r#"return await zero.expand("tz://blob/deadbeef")"#,
            &CodeModeOptions::default(),
            || CodeModeResult::completed(json!({"text": "ok"}), Vec::new(), 0, 0, 0),
        );
        assert!(
            result.error.is_none(),
            "expand-only recovery must not wait on analysis permit: {result:?}"
        );
        drop(holders);
    }

    #[test]
    fn classify_expand_only_as_status_keeps_mixed_light() {
        assert_eq!(
            classify(r#"return await zero.expand("tz://blob/abc")"#, 32),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(r#"return await zero.token.expand(ref)"#, 32),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(
                r#"return await zero.token.expandMany(["tz://blob/a","tz://blob/b"])"#,
                32
            ),
            ExecutionClass::Status
        );
        assert_eq!(
            classify(
                r#"const e = await zero.expand(c.ref); return e;"#,
                32
            ),
            ExecutionClass::Status
        );
        // Expand + find/search must stay analysis-gated.
        assert_eq!(
            classify(
                r#"const hits = await zero.find("x"); return await zero.expand(hits.ref);"#,
                32
            ),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(
                r#"await zero.fs.compound("read", {path: "src/expand.rs"})"#,
                32
            ),
            ExecutionClass::Light
        );
    }

    #[test]
    fn default_analysis_concurrency_is_core_budgeted() {
        let cores = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4);
        let got = default_analysis_concurrency();
        let expect = (cores / 4).max(1).min(DEFAULT_ANALYSIS_CONCURRENCY_CAP);
        assert_eq!(got, expect);
        assert!(got >= 1);
    }

    #[test]
    fn default_index_concurrency_is_core_budgeted() {
        let cores = std::thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(4);
        let got = default_index_concurrency();
        let expect = (cores / 8).max(1).min(DEFAULT_INDEX_CONCURRENCY_CAP);
        assert_eq!(got, expect);
        assert!(got >= 1);
        assert!(got <= DEFAULT_INDEX_CONCURRENCY_CAP);
    }

    #[test]
    fn multi_slot_analysis_permit_allows_parallel_holders() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-analysis-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let a = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "slot-a",
        )
        .expect("first slot");
        let b = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "slot-b",
        )
        .expect("second slot");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "slot-c",
        );
        assert!(
            contested.is_err(),
            "third holder must wait when only two slots exist"
        );
        drop(a);
        drop(b);
    }

    #[test]
    fn index_permit_is_exclusive_across_threads() {
        let path = std::env::temp_dir().join(format!(
            "tokenzero-index-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&path);
        let barrier = Arc::new(Barrier::new(2));
        let path_holder = path.clone();
        let barrier_holder = Arc::clone(&barrier);
        let holder = thread::spawn(move || {
            let permit = MachinePermit::acquire(
                &path_holder,
                Instant::now() + Duration::from_secs(5),
                "test-index-holder",
            )
            .expect("holder acquires index permit");
            barrier_holder.wait();
            thread::sleep(Duration::from_millis(300));
            drop(permit);
        });

        barrier.wait();
        let contested = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_millis(80),
            "test-index-contender",
        );
        assert!(
            contested.is_err(),
            "second acquirer must not stack while holder is live: {contested:?}"
        );
        holder.join().unwrap();
        let after = MachinePermit::acquire(
            &path,
            Instant::now() + Duration::from_secs(2),
            "test-index-after",
        );
        assert!(after.is_ok(), "permit must release for the next waiter");
    }

    #[test]
    fn multi_slot_index_permit_allows_parallel_holders() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-index-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let a = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "index-slot-a",
        )
        .expect("first index slot");
        let b = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "index-slot-b",
        )
        .expect("second index slot");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "index-slot-c",
        );
        assert!(
            contested.is_err(),
            "third index holder must wait when only two slots exist"
        );
        drop(a);
        drop(b);
    }

    #[test]
    fn index_execute_returns_busy_when_index_permit_held() {
        let path = index_permit_path();
        let _ = fs::remove_dir_all(&path);
        let slots = default_index_concurrency().max(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        let holders: Vec<_> = (0..slots)
            .map(|idx| {
                MachinePermit::acquire_slots(
                    &path,
                    slots,
                    deadline,
                    &format!("test-index-holder-{idx}"),
                )
                .unwrap_or_else(|e| panic!("pre-hold index slot {idx}/{slots}: {e}"))
            })
            .collect();

        let opts = CodeModeOptions {
            hard_max_wall_ms: 120,
            ..CodeModeOptions::default()
        };
        let result = execute(
            "await zero.token.index({rebuild:true})",
            &opts,
            || CodeModeResult::completed(json!({"ok": true}), Vec::new(), 0, 0, 0),
        );
        let err = result
            .error
            .as_ref()
            .expect("expected busy error from held index permit");
        assert!(
            err.retryable,
            "index permit contention must be retryable: {err:?}"
        );
        assert!(
            err.kind == "busy" || err.message.contains("machine_permit_busy"),
            "unexpected error: {err:?}"
        );
        drop(holders);
    }

    #[test]
    fn classify_routes_index_markers_before_light() {
        assert_eq!(
            classify("await zero.fs.index({path:'.'})", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("await watch.drain()", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("await zero.token.index({rebuild:true})", 32),
            ExecutionClass::Index
        );
        assert_eq!(
            classify("return {ok:true}", 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify("await zero.token.shell('ls')", 32),
            ExecutionClass::HeavyShell
        );
    }

    #[test]
    fn classify_rejects_bare_status_metrics_rebuild_false_positives() {
        // Paths / return shapes / prose must not ungate Status or steal Index.
        assert_eq!(
            classify(
                r#"await zero.fs.compound("read", {path: "docs/status.md"})"#,
                32
            ),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(r#"return {status: "ok", metrics: 1}"#, 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify(r#"await zero.token.shell("echo status metrics")"#, 32),
            ExecutionClass::HeavyShell
        );
        assert_eq!(
            classify(r#"return {note: "rebuild later"}"#, 32),
            ExecutionClass::Light
        );
        assert_eq!(
            classify("await rebuild_index()", 32),
            ExecutionClass::Light
        );
        // API-shaped markers still classify correctly.
        assert_eq!(classify("codemode.status", 32), ExecutionClass::Status);
        assert_eq!(classify("search: containment", 32), ExecutionClass::Status);
        assert_eq!(classify("describe:zero.read", 32), ExecutionClass::Status);
        assert_eq!(
            classify("await zero.token.index({rebuild:true})", 32),
            ExecutionClass::Index
        );
    }

    #[test]
    fn snapshot_exposes_index_max_active() {
        let snap = snapshot();
        assert!(
            snap.get("index_max_active")
                .and_then(|v| v.as_u64())
                .is_some_and(|v| v >= 1),
            "snapshot must expose index_max_active: {snap}"
        );
    }

    #[test]
    fn permit_backoff_grows_then_caps() {
        assert_eq!(permit_backoff(0), PERMIT_POLL);
        assert!(permit_backoff(3) > permit_backoff(0));
        assert_eq!(permit_backoff(10), PERMIT_POLL_MAX);
    }

    #[test]
    fn slots_one_uses_slot_zero_not_base() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-slot0-layout-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let permit = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_secs(2),
            "slot-one",
        )
        .expect("slots=1 must acquire");
        assert!(
            base.join("slot-0").join("pid").is_file(),
            "slots=1 must lock base/slot-0, not base itself"
        );
        assert!(
            !base.join("pid").is_file(),
            "slots=1 must not write pid directly under base"
        );
        drop(permit);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn mixed_concurrency_layouts_share_slot_namespace() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-mixed-slots-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);

        // slots=1 holds slot-0; slots>1 peer must take another slot child (shared
        // namespace), not invent a nested lock under an exclusive base.
        let holder = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-1",
        )
        .expect("slots=1 holder");
        let peer = MachinePermit::acquire_slots(
            &base,
            3,
            Instant::now() + Duration::from_secs(2),
            "peer-slots-3",
        )
        .expect("slots>1 peer must share slot namespace with slots=1 holder");
        assert_eq!(peer.0.parent(), Some(base.as_path()));
        let peer_name = peer.0.file_name().and_then(|n| n.to_str()).unwrap_or("");
        assert!(
            peer_name.starts_with("slot-") && peer_name != "slot-0",
            "peer must occupy a free slot child, got {}",
            peer.0.display()
        );
        drop(peer);
        drop(holder);

        // Saturated multi-slot pool must reject a slots=1 peer (no stacking past budget).
        let holder = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-2",
        )
        .expect("slots=2 holder");
        let holder2 = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(5),
            "holder-slots-2b",
        )
        .expect("second slots=2 holder");
        let contested = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_millis(80),
            "peer-slots-1",
        );
        assert!(
            contested.is_err(),
            "slots=1 peer must not stack when multi-slot pool is full: {contested:?}"
        );
        drop(holder);
        drop(holder2);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn live_legacy_exclusive_base_blocks_all_slots() {
        let base = std::env::temp_dir().join(format!(
            "tokenzero-legacy-excl-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_dir_all(&base);
        let legacy = MachinePermit::acquire(
            &base,
            Instant::now() + Duration::from_secs(5),
            "legacy-exclusive",
        )
        .expect("legacy exclusive at base");
        let contested = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_millis(80),
            "slot-peer",
        );
        assert!(
            contested.is_err(),
            "live legacy exclusive base must block slot children: {contested:?}"
        );
        drop(legacy);
        let after = MachinePermit::acquire_slots(
            &base,
            2,
            Instant::now() + Duration::from_secs(2),
            "after-legacy",
        );
        assert!(after.is_ok(), "slots acquire after legacy release: {after:?}");
        drop(after);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn map_acquire_error_distinguishes_busy_vs_fatal() {
        let busy = map_acquire_error(AcquireError::Busy(
            "codemode permit /tmp/x is held by live process(es) across 1 slots".into(),
        ));
        let busy_err = busy
            .error
            .as_ref()
            .expect("busy mapping must produce an error");
        assert!(
            busy_err.retryable,
            "live-holder timeout must stay retryable: {busy_err:?}"
        );
        assert_eq!(busy_err.kind, "busy");
        assert!(
            busy_err.message.contains("machine_permit_busy"),
            "unexpected busy mapping: {busy_err:?}"
        );

        let fatal = map_acquire_error(AcquireError::Fatal(
            "create codemode permit /tmp/x/slot-0: Permission denied (os error 13)".into(),
        ));
        let fatal_err = fatal
            .error
            .as_ref()
            .expect("fatal mapping must produce an error");
        assert!(
            !fatal_err.retryable,
            "Fatal permit I/O must not be retryable: {fatal_err:?}"
        );
        assert_eq!(fatal_err.kind, "substrate");
        assert!(
            fatal_err.message.contains("machine_permit_io"),
            "unexpected fatal mapping: {fatal_err:?}"
        );
        assert!(
            !fatal_err.message.contains("machine_permit_busy"),
            "Fatal must not be labeled busy: {fatal_err:?}"
        );
    }

    #[test]
    fn acquire_slots_returns_fatal_when_parent_is_not_a_directory() {
        // Parent path is a file → create_dir for slot children fails as Fatal (not Busy).
        let blocker = std::env::temp_dir().join(format!(
            "tokenzero-permit-fatal-blocker-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        let _ = fs::remove_file(&blocker);
        let _ = fs::remove_dir_all(&blocker);
        fs::write(&blocker, b"not-a-directory").expect("write blocker file");
        let base = blocker.join("nested-permit");

        let err = MachinePermit::acquire_slots(
            &base,
            1,
            Instant::now() + Duration::from_millis(80),
            "test-fatal",
        )
        .expect_err("expected Fatal when permit parent is a file");
        let _ = fs::remove_file(&blocker);

        match err {
            AcquireError::Fatal(message) => {
                assert!(
                    message.contains("create codemode permit"),
                    "unexpected Fatal message: {message}"
                );
            }
            AcquireError::Busy(message) => {
                panic!("I/O failure must be Fatal, not Busy: {message}")
            }
        }
    }

}
