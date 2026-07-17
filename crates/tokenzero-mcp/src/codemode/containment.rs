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
//! In-process slot waits are wall-bounded (same `hard_max_wall_ms` as machine
//! permits) and return retryable busy on deadline — never hang forever.
//! Analysis / index / heavy use separate in-process active counters so machine
//! `analysis_max_active` is reachable inside one multiplexed process
//! (`tokenzero-jn1i`).
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
use std::time::{Duration, Instant};

use super::{CodeModeOptions, CodeModeResult};

use zerostack_machine_permit::{AcquireError, MachinePermit};

const DEFAULT_MAX_ACTIVE: usize = 1;
const DEFAULT_MAX_QUEUE_DEPTH: usize = 8;
const DEFAULT_COST_THRESHOLD: usize = 32;
const DEFAULT_ANALYSIS_CONCURRENCY_CAP: usize = 8;
const DEFAULT_INDEX_CONCURRENCY_CAP: usize = 2;
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
    /// In-process heavy (shell / high-cost) holders — budgeted by `max_active`.
    active_heavy: usize,
    /// In-process analysis (light) holders — budgeted by `analysis_max_active`
    /// so machine analysis concurrency is reachable inside one multiplexed process.
    active_analysis: usize,
    /// In-process index holders — budgeted by `index_max_active`.
    active_index: usize,
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
            ExecutionClass::Light
            | ExecutionClass::Index
            | ExecutionClass::HeavyShell
            | ExecutionClass::HeavyEstimatedCost => self.run_gated(class, options, run),
        };
        *flight.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(result.clone());
        flight.ready.notify_all();
        self.lock().flights.remove(&key);
        result
    }

    /// Acquire in-process class slot + matching machine permit, then run.
    ///
    /// Analysis / index / heavy share this path; only heavy enters
    /// `HeavyExecutionGuard` and cancels background jobs on error.
    /// In-process waits are wall-bounded (tokenzero-jn1i); machine permits
    /// use family-wide slot pools so sibling engines cannot stack CPU.
    fn run_gated<F>(
        &self,
        class: ExecutionClass,
        options: &CodeModeOptions,
        run: F,
    ) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        let deadline = wall_deadline(options);
        let slot = match self.acquire_slot(class, deadline) {
            Ok(v) => v,
            Err(v) => return v,
        };
        let (path, slots, command) = match class {
            ExecutionClass::Light => (
                analysis_permit_path(),
                self.config.analysis_max_active,
                "tokenzero-codemode-analysis",
            ),
            ExecutionClass::Index => (
                index_permit_path(),
                self.config.index_max_active,
                "tokenzero-codemode-index",
            ),
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost => (
                heavy_permit_path(),
                self.config.max_active,
                "tokenzero-codemode-heavy",
            ),
            ExecutionClass::Status => {
                drop(slot);
                return catch_worker_panic(run);
            }
        };
        let permit = match MachinePermit::acquire_slots(&path, slots, deadline, command) {
            Ok(v) => v,
            Err(e) => {
                drop(slot);
                return map_acquire_error(e);
            }
        };
        let heavy = matches!(
            class,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost
        );
        let result = if heavy {
            let guard = HeavyExecutionGuard::enter();
            let result = catch_worker_panic(run);
            if result.error.is_some() {
                self.cancel_background_jobs(guard.0);
            }
            result
        } else {
            catch_worker_panic(run)
        };
        drop(slot);
        drop(permit);
        result
    }

    fn class_limit(&self, class: ExecutionClass) -> usize {
        match class {
            ExecutionClass::Status => 0,
            ExecutionClass::Light => self.config.analysis_max_active,
            ExecutionClass::Index => self.config.index_max_active,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost => {
                self.config.max_active
            }
        }
    }

    fn active_for(state: &State, class: ExecutionClass) -> usize {
        match class {
            ExecutionClass::Status => 0,
            ExecutionClass::Light => state.active_analysis,
            ExecutionClass::Index => state.active_index,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost => state.active_heavy,
        }
    }

    fn bump_active(state: &mut State, class: ExecutionClass, delta: isize) {
        let counter = match class {
            ExecutionClass::Status => return,
            ExecutionClass::Light => &mut state.active_analysis,
            ExecutionClass::Index => &mut state.active_index,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost => {
                &mut state.active_heavy
            }
        };
        if delta >= 0 {
            *counter = counter.saturating_add(delta as usize);
        } else {
            *counter = counter.saturating_sub((-delta) as usize);
        }
    }

    fn queue_busy_code(class: ExecutionClass, full: bool) -> &'static str {
        match (class, full) {
            (ExecutionClass::Light, true) => "analysis_queue_full",
            (ExecutionClass::Light, false) => "analysis_queue_busy",
            (ExecutionClass::Index, true) => "index_queue_full",
            (ExecutionClass::Index, false) => "index_queue_busy",
            (ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost, true) => {
                "heavy_queue_full"
            }
            (ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost, false) => {
                "heavy_queue_busy"
            }
            (ExecutionClass::Status, _) => "heavy_queue_busy",
        }
    }

    /// Acquire an in-process class slot, waiting at most until `deadline`.
    ///
    /// Analysis / index / heavy use separate active counters so light work is not
    /// serialized behind `heavy max_active` (default 1). On wall deadline return
    /// retryable busy — never hang forever ahead of the machine permit wait
    /// (tokenzero-jn1i).
    #[allow(clippy::result_large_err)]
    fn acquire_slot(
        &self,
        class: ExecutionClass,
        deadline: Instant,
    ) -> Result<HeavySlot<'_>, CodeModeResult> {
        let limit = self.class_limit(class).max(1);
        let mut s = self.lock();
        if Self::active_for(&s, class) >= limit {
            if s.queue_depth >= self.config.max_queue_depth {
                s.rejected_count = s.rejected_count.saturating_add(1);
                return Err(busy_result(
                    Self::queue_busy_code(class, true),
                    "bounded CodeMode in-process queue is full; retry with backoff",
                ));
            }
            s.queue_depth += 1;
            loop {
                if Self::active_for(&s, class) < limit {
                    break;
                }
                let now = Instant::now();
                if now >= deadline {
                    s.queue_depth -= 1;
                    s.rejected_count = s.rejected_count.saturating_add(1);
                    return Err(busy_result(
                        Self::queue_busy_code(class, false),
                        "in-process CodeMode slot wait hit wall deadline; retry with backoff",
                    ));
                }
                let wait = deadline.saturating_duration_since(now);
                let (guard, wait_result) = self
                    .capacity
                    .wait_timeout(s, wait)
                    .unwrap_or_else(|p| p.into_inner());
                s = guard;
                if wait_result.timed_out() && Self::active_for(&s, class) >= limit {
                    s.queue_depth -= 1;
                    s.rejected_count = s.rejected_count.saturating_add(1);
                    return Err(busy_result(
                        Self::queue_busy_code(class, false),
                        "in-process CodeMode slot wait hit wall deadline; retry with backoff",
                    ));
                }
            }
            s.queue_depth -= 1;
        }
        Self::bump_active(&mut s, class, 1);
        s.active_started = Some(Instant::now());
        s.operation_class = Some(class);
        if matches!(
            class,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost
        ) {
            s.child_pid = None;
            s.child_pgid = None;
            s.cancellation_state = Some("not_cancelled");
        }
        Ok(HeavySlot {
            controller: self,
            class,
        })
    }
    fn snapshot(&self) -> Value {
        let s = self.lock();
        let worker_count = s
            .active_heavy
            .saturating_add(s.active_analysis)
            .saturating_add(s.active_index);
        json!({
            "queue_depth": s.queue_depth,
            "active_heavy": s.active_heavy,
            "active_analysis": s.active_analysis,
            "active_index": s.active_index,
            "worker_count": worker_count,
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
#[derive(Debug)]
struct HeavySlot<'a> {
    controller: &'a Controller,
    class: ExecutionClass,
}
impl Drop for HeavySlot<'_> {
    fn drop(&mut self) {
        let mut s = self.controller.lock();
        let owned_pgid = matches!(
            self.class,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost
        )
        .then(|| {
            (s.cancellation_state == Some("running"))
                .then_some(s.child_pgid)
                .flatten()
        })
        .flatten();
        Controller::bump_active(&mut s, self.class, -1);
        let idle = s.active_heavy == 0 && s.active_analysis == 0 && s.active_index == 0;
        if idle {
            s.active_started = None;
            s.operation_class = None;
            s.child_pid = None;
            s.child_pgid = None;
            s.cancellation_state = None;
        } else if matches!(
            self.class,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost
        ) && s.active_heavy == 0
        {
            s.child_pid = None;
            s.child_pgid = None;
            s.cancellation_state = None;
        }
        drop(s);
        if let Some(pgid) = owned_pgid {
            terminate_owned_process_group(pgid);
        }
        self.controller.capacity.notify_all();
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

fn wall_deadline(options: &CodeModeOptions) -> Instant {
    Instant::now()
        .checked_add(Duration::from_millis(options.hard_max_wall_ms.max(1)))
        .unwrap_or_else(Instant::now)
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
                // Unique per test THREAD: parallel tests must not race on one
                // permit dir (remove_dir_all vs acquire flake, RUST_TEST_THREADS=2).
                let pid = format!("{}-{:?}", std::process::id(), std::thread::current().id());
                std::env::temp_dir().join(format!("zerostack-codemode-heavy-test-{pid}.permit"))
            } else {
                zerostack_machine_permit::scoped_permit_base("heavy")
            }
        })
}
fn analysis_permit_path() -> PathBuf {
    std::env::var_os("TOKENZERO_CODEMODE_ANALYSIS_PERMIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                // Unique per test THREAD: parallel tests must not race on one
                // permit dir (remove_dir_all vs acquire flake, RUST_TEST_THREADS=2).
                let pid = format!("{}-{:?}", std::process::id(), std::thread::current().id());
                std::env::temp_dir().join(format!("zerostack-codemode-analysis-test-{pid}.permit"))
            } else {
                zerostack_machine_permit::scoped_permit_base("analysis")
            }
        })
}

fn index_permit_path() -> PathBuf {
    std::env::var_os("TOKENZERO_CODEMODE_INDEX_PERMIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            if cfg!(test) {
                // Unique per test THREAD: parallel tests must not race on one
                // permit dir (remove_dir_all vs acquire flake, RUST_TEST_THREADS=2).
                let pid = format!("{}-{:?}", std::process::id(), std::thread::current().id());
                std::env::temp_dir().join(format!("zerostack-codemode-index-test-{pid}.permit"))
            } else {
                zerostack_machine_permit::scoped_permit_base("index")
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

#[cfg(test)]
#[path = "containment_tests.rs"]
mod tests;
