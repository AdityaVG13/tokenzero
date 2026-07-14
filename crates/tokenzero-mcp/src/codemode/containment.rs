//! Hard containment for expensive CodeMode execution.

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
#[cfg(not(test))]
const DEFAULT_MACHINE_PERMIT: &str = "/tmp/zerostack-codemode-heavy.permit";
const PERMIT_POLL: Duration = Duration::from_millis(20);

thread_local! {
    static HEAVY_EXECUTION_ID: Cell<Option<u64>> = const { Cell::new(None) };
}

static NEXT_EXECUTION_ID: AtomicU64 = AtomicU64::new(1);

struct HeavyExecutionGuard {
    id: u64,
}

impl HeavyExecutionGuard {
    fn enter() -> Self {
        let id = NEXT_EXECUTION_ID.fetch_add(1, Ordering::Relaxed);
        HEAVY_EXECUTION_ID.with(|cell| cell.set(Some(id)));
        HeavyExecutionGuard { id }
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

fn heavy_execution_active() -> bool {
    heavy_execution_id().is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionClass {
    Status,
    Light,
    HeavyShell,
    HeavyEstimatedCost,
}
impl ExecutionClass {
    fn is_heavy(self) -> bool {
        matches!(self, Self::HeavyShell | Self::HeavyEstimatedCost)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ContainmentSnapshot {
    pub queue_depth: usize,
    pub active_heavy: usize,
    pub worker_count: usize,
    pub child_pid: Option<u32>,
    pub child_pgid: Option<u32>,
    pub operation_class: Option<ExecutionClass>,
    pub elapsed_ms: Option<u64>,
    pub cancellation_state: &'static str,
    pub background_jobs: usize,
    pub rejected_count: u64,
}

#[derive(Debug, Clone, Copy)]
struct BackgroundChild {
    execution_id: u64,
    pid: Option<u32>,
    pgid: Option<u32>,
    cancelled: bool,
}

#[derive(Debug)]
struct State {
    active_heavy: usize,
    queue_depth: usize,
    rejected_count: u64,
    active_started: Option<Instant>,
    operation_class: Option<ExecutionClass>,
    child_pid: Option<u32>,
    child_pgid: Option<u32>,
    cancellation_state: &'static str,
    background_jobs: HashMap<String, BackgroundChild>,
    flights: HashMap<String, Arc<Flight>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            active_heavy: 0,
            queue_depth: 0,
            rejected_count: 0,
            active_started: None,
            operation_class: None,
            child_pid: None,
            child_pgid: None,
            cancellation_state: "none",
            background_jobs: HashMap::new(),
            flights: HashMap::new(),
        }
    }
}
#[derive(Debug, Default)]
struct Flight {
    result: Mutex<Option<CodeModeResult>>,
    ready: Condvar,
    followers: AtomicUsize,
}

struct FollowerGuard<'a> {
    flight: &'a Flight,
}
impl Drop for FollowerGuard<'_> {
    fn drop(&mut self) {
        self.flight.followers.fetch_sub(1, Ordering::AcqRel);
    }
}
#[derive(Debug, Clone)]
struct Config {
    max_active: usize,
    max_queue_depth: usize,
    cost_threshold: usize,
    permit_path: PathBuf,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_active: env_usize("TOKENZERO_CODEMODE_HEAVY_CONCURRENCY", DEFAULT_MAX_ACTIVE)
                .max(1),
            max_queue_depth: env_usize(
                "TOKENZERO_CODEMODE_HEAVY_QUEUE_DEPTH",
                DEFAULT_MAX_QUEUE_DEPTH,
            ),
            cost_threshold: env_usize(
                "TOKENZERO_CODEMODE_HEAVY_COST_THRESHOLD",
                DEFAULT_COST_THRESHOLD,
            )
            .max(1),
            permit_path: std::env::var_os("TOKENZERO_CODEMODE_HEAVY_PERMIT")
                .map(PathBuf::from)
                .unwrap_or_else(default_machine_permit),
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
    serde_json::to_value(controller().snapshot()).unwrap_or_else(|_| json!({}))
}

/// Engine-shell seam. The process runner remains the sole kill authority; this
/// only publishes bounded identity/state and is also used by synthetic tests.
pub(crate) fn note_child(pid: Option<u32>, pgid: Option<u32>, cancellation_state: &'static str) {
    if !heavy_execution_active() {
        return;
    }
    controller().note_child(pid, pgid, cancellation_state);
}

pub(crate) fn reserve_background_job(id: &str) {
    let Some(execution_id) = heavy_execution_id() else {
        return;
    };
    controller().reserve_background_job(id, execution_id);
}

pub(crate) fn note_background_child(id: &str, pid: Option<u32>, pgid: Option<u32>) {
    controller().note_background_child(id, pid, pgid);
}

pub(crate) fn finish_background_job(id: &str) {
    controller().finish_background_job(id);
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
            state.cancellation_state = cancellation_state;
        }
    }

    fn reserve_background_job(&self, id: &str, execution_id: u64) {
        self.lock().background_jobs.insert(
            id.to_string(),
            BackgroundChild {
                execution_id,
                pid: None,
                pgid: None,
                cancelled: false,
            },
        );
    }

    fn note_background_child(&self, id: &str, pid: Option<u32>, pgid: Option<u32>) {
        let cancelled_pgid = {
            let mut state = self.lock();
            match state.background_jobs.get_mut(id) {
                Some(job) if job.cancelled => {
                    let cancelled_pgid = pgid;
                    state.background_jobs.remove(id);
                    cancelled_pgid
                }
                Some(job) => {
                    job.pid = pid;
                    job.pgid = pgid;
                    None
                }
                None => None,
            }
        };
        if let Some(pgid) = cancelled_pgid {
            terminate_owned_process_group(pgid);
        }
    }

    fn finish_background_job(&self, id: &str) {
        self.lock().background_jobs.remove(id);
    }

    fn cancel_background_jobs(&self, execution_id: u64) {
        let pgids = {
            let mut state = self.lock();
            let mut pgids = Vec::new();
            let ids = state
                .background_jobs
                .iter()
                .filter_map(|(id, job)| (job.execution_id == execution_id).then(|| id.clone()))
                .collect::<Vec<_>>();
            for id in ids {
                if let Some(job) = state.background_jobs.get_mut(&id) {
                    job.cancelled = true;
                    if let Some(pgid) = job.pgid {
                        pgids.push(pgid);
                    }
                }
                if state
                    .background_jobs
                    .get(&id)
                    .is_some_and(|job| job.pgid.is_some())
                {
                    state.background_jobs.remove(&id);
                }
            }
            pgids
        };
        for pgid in pgids {
            terminate_owned_process_group(pgid);
        }
    }

    fn status_snapshot_result(&self) -> CodeModeResult {
        let snapshot = serde_json::to_value(self.snapshot()).unwrap_or_else(|_| json!({}));
        CodeModeResult::completed(snapshot, Vec::new(), 0, 0, 0)
    }

    fn execute<F>(&self, plan: &str, options: &CodeModeOptions, run: F) -> CodeModeResult
    where
        F: FnOnce() -> CodeModeResult,
    {
        let plan_trimmed = plan.trim();
        if is_exact_status_plan(plan_trimmed) {
            return self.status_snapshot_result();
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
            let _follower = FollowerGuard { flight: &flight };
            let mut result = flight.result.lock().unwrap_or_else(|p| p.into_inner());
            while result.is_none() {
                result = flight.ready.wait(result).unwrap_or_else(|p| p.into_inner());
            }
            return result.as_ref().expect("completed flight").clone();
        }
        let result = if class.is_heavy() {
            self.run_heavy(class, options, run)
        } else {
            catch_worker_panic(run)
        };
        *flight.result.lock().unwrap_or_else(|p| p.into_inner()) = Some(result.clone());
        flight.ready.notify_all();
        self.lock().flights.remove(&key);
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
        let permit = match MachinePermit::acquire(&self.config.permit_path, deadline) {
            Ok(v) => v,
            Err(e) => {
                drop(slot);
                return busy_result("machine_permit_busy", &e);
            }
        };
        let result = {
            let guard = HeavyExecutionGuard::enter();
            let result = catch_worker_panic(run);
            if result.error.is_some() {
                self.cancel_background_jobs(guard.id);
            }
            result
        };
        drop(slot);
        drop(permit);
        result
    }
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
        s.cancellation_state = "not_cancelled";
        Ok(HeavySlot { controller: self })
    }
    fn snapshot(&self) -> ContainmentSnapshot {
        let s = self.lock();
        ContainmentSnapshot {
            queue_depth: s.queue_depth,
            active_heavy: s.active_heavy,
            worker_count: s.active_heavy,
            child_pid: s.child_pid,
            child_pgid: s.child_pgid,
            operation_class: s.operation_class,
            elapsed_ms: s
                .active_started
                .map(|v| u64::try_from(v.elapsed().as_millis()).unwrap_or(u64::MAX)),
            cancellation_state: s.cancellation_state,
            background_jobs: s.background_jobs.len(),
            rejected_count: s.rejected_count,
        }
    }
}
struct HeavySlot<'a> {
    controller: &'a Controller,
}
impl Drop for HeavySlot<'_> {
    fn drop(&mut self) {
        let mut s = self.controller.lock();
        let owned_pgid = (s.cancellation_state == "running")
            .then_some(s.child_pgid)
            .flatten();
        s.active_heavy = s.active_heavy.saturating_sub(1);
        if s.active_heavy == 0 {
            s.active_started = None;
            s.operation_class = None;
            s.child_pid = None;
            s.child_pgid = None;
            s.cancellation_state = "none";
        }
        drop(s);
        if let Some(pgid) = owned_pgid {
            terminate_owned_process_group(pgid);
        }
        self.controller.capacity.notify_one();
    }
}

#[cfg(unix)]
fn terminate_owned_process_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    let target = format!("-{pgid}");
    let _ = std::process::Command::new("kill")
        .args(["-TERM", "--", &target])
        .status();
    std::thread::sleep(Duration::from_millis(50));
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &target])
        .status();
}
#[cfg(not(unix))]
fn terminate_owned_process_group(_: u32) {}

#[derive(Debug)]
struct MachinePermit {
    path: PathBuf,
    owner: String,
}
impl MachinePermit {
    fn acquire(path: &Path, deadline: Instant) -> Result<Self, String> {
        loop {
            match fs::create_dir(path) {
                Ok(()) => {
                    let owner = permit_owner();
                    if let Err(e) = write_metadata(path, &owner) {
                        cleanup_owned(path, &owner);
                        return Err(format!("write heavy permit metadata: {e}"));
                    }
                    return Ok(Self {
                        path: path.to_path_buf(),
                        owner,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if reclaim_dead(path) {
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err(format!(
                            "heavy permit {} is held by a live process",
                            path.display()
                        ));
                    }
                    std::thread::sleep(
                        PERMIT_POLL.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(e) => return Err(format!("create heavy permit {}: {e}", path.display())),
            }
        }
    }
}
impl Drop for MachinePermit {
    fn drop(&mut self) {
        cleanup_owned(&self.path, &self.owner)
    }
}
fn write_metadata(path: &Path, owner: &str) -> std::io::Result<()> {
    // Write ownership first so an error in any later metadata write remains
    // removable by the acquiring RAII guard.
    fs::write(path.join("owner"), owner)?;
    fs::write(path.join("pid"), std::process::id().to_string())?;
    fs::write(path.join("repository"), current_repository())?;
    fs::write(path.join("command"), "tokenzero-codemode-heavy")?;
    fs::write(path.join("started_at"), epoch_millis().to_string())
}
fn cleanup_owned(path: &Path, owner: &str) {
    if fs::read_to_string(path.join("owner")).ok().as_deref() != Some(owner) {
        return;
    }
    for n in ["pid", "repository", "command", "started_at", "owner"] {
        let _ = fs::remove_file(path.join(n));
    }
    let _ = fs::remove_dir(path);
}
fn reclaim_dead(path: &Path) -> bool {
    let Ok(pid) = fs::read_to_string(path.join("pid")) else {
        return false;
    };
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return false;
    };
    if process_alive(pid) {
        return false;
    }
    for n in ["pid", "repository", "command", "started_at", "owner"] {
        let _ = fs::remove_file(path.join(n));
    }
    fs::remove_dir(path).is_ok()
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
fn current_repository() -> String {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .to_string_lossy()
        .chars()
        .take(1024)
        .collect()
}
fn permit_owner() -> String {
    format!(
        "{}-{}-{:?}",
        std::process::id(),
        epoch_millis(),
        std::thread::current().id()
    )
}
fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |v| v.as_millis())
}
fn catch_worker_panic<F>(run: F) -> CodeModeResult
where
    F: FnOnce() -> CodeModeResult,
{
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).unwrap_or_else(|_| {
        CodeModeResult::error_with_kind(
            "containment_panic",
            "contained CodeMode worker panicked; permits and queue slots were released",
            0,
            false,
        )
    })
}
fn busy_result(code: &str, message: &str) -> CodeModeResult {
    let mut r = CodeModeResult::error_with_kind("busy", format!("{code}: {message}"), 0, true);
    r.telemetry.extra = Some(
        json!({"backpressure":{"class":"busy","code":code,"retryable":true,"retry_strategy":"exponential_backoff"}}),
    );
    r
}
fn classify(plan: &str, cost_threshold: usize) -> ExecutionClass {
    let p = plan.trim().to_ascii_lowercase();
    if is_status(&p) {
        return ExecutionClass::Status;
    }
    if p.contains(".shell(")
        || p.contains("tz_shell")
        || p.contains("\"shell\"")
        || p.contains("'shell'")
    {
        return ExecutionClass::HeavyShell;
    }
    let cost = p.matches("zero.").count() + p.matches("tz_").count() + p.matches("method").count();
    if cost > cost_threshold {
        ExecutionClass::HeavyEstimatedCost
    } else {
        ExecutionClass::Light
    }
}
fn is_status(p: &str) -> bool {
    p.starts_with("search:")
        || p.starts_with("describe:")
        || [
            "codemode.limits",
            "journaldoctor",
            "journal_doctor",
            "metrics",
            "status",
        ]
        .iter()
        .any(|v| p.contains(v))
}
fn is_exact_status_plan(plan: &str) -> bool {
    matches!(
        plan.to_ascii_lowercase().as_str(),
        "status" | "codemode.status" | "containment.status"
    )
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
#[cfg(not(test))]
fn default_machine_permit() -> PathBuf {
    PathBuf::from(DEFAULT_MACHINE_PERMIT)
}

#[cfg(test)]
fn default_machine_permit() -> PathBuf {
    std::env::temp_dir().join(format!(
        "zerostack-codemode-heavy-test-{}.permit",
        std::process::id()
    ))
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;
    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tokenzero-containment-{name}-{}-{}",
            std::process::id(),
            epoch_millis()
        ))
    }
    fn ctl(path: PathBuf, queue: usize) -> Arc<Controller> {
        Arc::new(Controller::new(Config {
            max_active: 1,
            max_queue_depth: queue,
            cost_threshold: 1,
            permit_path: path,
        }))
    }
    fn heavy(id: usize) -> String {
        format!("zero.read({id}); zero.read({id})")
    }
    struct BlockedExecution {
        gate: Arc<(Mutex<bool>, Condvar)>,
        join: thread::JoinHandle<CodeModeResult>,
    }
    impl BlockedExecution {
        fn start(controller: Arc<Controller>, id: usize) -> Self {
            let gate = Arc::new((Mutex::new(false), Condvar::new()));
            let entered = Arc::new((Mutex::new(false), Condvar::new()));
            let (worker_gate, worker_entered) = (Arc::clone(&gate), Arc::clone(&entered));
            let join = thread::spawn(move || controller.execute(
                &heavy(id), &CodeModeOptions::default(), || {
                    *worker_entered.0.lock().unwrap() = true;
                    worker_entered.1.notify_all();
                    let mut open = worker_gate.0.lock().unwrap();
                    while !*open { open = worker_gate.1.wait(open).unwrap(); }
                    CodeModeResult::completed(json!(id), vec![], 1, 1, 1)
                },
            ));
            let mut ready = entered.0.lock().unwrap();
            while !*ready { ready = entered.1.wait(ready).unwrap(); }
            drop(ready);
            Self { gate, join }
        }
        fn finish(self) -> CodeModeResult {
            *self.gate.0.lock().unwrap() = true;
            self.gate.1.notify_all();
            self.join.join().unwrap()
        }
    }
    #[test]
    fn containment_eight_submissions_never_exceed_max_concurrency() {
        let c = ctl(temp("parallel"), 8);
        let active = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let mut joins = vec![];
        for id in 0..8 {
            let c = Arc::clone(&c);
            let a = Arc::clone(&active);
            let m = Arc::clone(&max);
            joins.push(thread::spawn(move || {
                c.execute(&heavy(id), &CodeModeOptions::default(), || {
                    let n = a.fetch_add(1, Ordering::SeqCst) + 1;
                    m.fetch_max(n, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(20));
                    a.fetch_sub(1, Ordering::SeqCst);
                    CodeModeResult::completed(json!(id), vec![], 1, 1, 1)
                })
            }));
        }
        for j in joins {
            assert!(j.join().unwrap().error.is_none())
        }
        assert_eq!(max.load(Ordering::SeqCst), 1);
        assert_eq!(c.snapshot().active_heavy, 0)
    }
    #[test]
    fn containment_duplicates_coalesce_one_side_effect() {
        let c = ctl(temp("dedup"), 8);
        let calls = Arc::new(AtomicUsize::new(0));
        let mut joins = vec![];
        for _ in 0..8 {
            let c = Arc::clone(&c);
            let calls = Arc::clone(&calls);
            joins.push(thread::spawn(move || {
                c.execute(&heavy(9), &CodeModeOptions::default(), || {
                    calls.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(40));
                    CodeModeResult::completed(json!(9), vec![], 1, 1, 1)
                })
            }));
        }
        for j in joins {
            assert_eq!(j.join().unwrap().value, Some(json!(9)))
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1)
    }
    #[test]
    fn containment_dedup_followers_are_bounded() {
        let c = ctl(temp("dedup-bounded"), 1);
        let blocked = BlockedExecution::start(Arc::clone(&c), 19);
        let follower = {
            let c = Arc::clone(&c);
            thread::spawn(move || c.execute(&heavy(19), &CodeModeOptions::default(), || panic!("follower ran")))
        };
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline
            && !c.lock().flights.values().any(|f| f.followers.load(Ordering::Acquire) == 1)
        {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(c.lock().flights.values().any(|f| f.followers.load(Ordering::Acquire) == 1));
        let rejected = c.execute(&heavy(19), &CodeModeOptions::default(), || panic!("clone ran"));
        assert_eq!(rejected.error.as_ref().map(|e| e.kind.as_str()), Some("busy"));
        assert!(rejected.error.unwrap().message.contains("dedup_followers_full"));
        assert!(blocked.finish().error.is_none());
        assert!(follower.join().unwrap().error.is_none());
    }

    #[test]
    fn containment_queue_full_is_structured_busy() {
        let c = ctl(temp("busy"), 0);
        let blocked = BlockedExecution::start(Arc::clone(&c), 1);
        let result = c.execute(&heavy(2), &CodeModeOptions::default(), || panic!("spawned"));
        assert_eq!(result.error.as_ref().map(|e| e.kind.as_str()), Some("busy"));
        assert!(result.error.unwrap().retryable);
        blocked.finish();
        assert_eq!(c.snapshot().rejected_count, 1)
    }

    #[test]
    fn containment_error_and_panic_release_permit() {
        let p = temp("release");
        let c = ctl(p.clone(), 1);
        assert!(
            c.execute(&heavy(1), &CodeModeOptions::default(), || {
                CodeModeResult::error("injected", 0)
            })
            .error
            .is_some()
        );
        assert!(!p.exists());
        assert_eq!(
            c.execute(&heavy(2), &CodeModeOptions::default(), || panic!(
                "injected"
            ))
            .error
            .unwrap()
            .kind,
            "containment_panic"
        );
        assert!(!p.exists())
    }
    #[test]
    fn containment_two_clients_share_machine_permit() {
        let p = temp("clients");
        let a = ctl(p.clone(), 2);
        let b = ctl(p, 2);
        let active = Arc::new(AtomicUsize::new(0));
        let max = Arc::new(AtomicUsize::new(0));
        let spawn = |c: Arc<Controller>, id| {
            let active = Arc::clone(&active);
            let max = Arc::clone(&max);
            thread::spawn(move || {
                c.execute(&heavy(id), &CodeModeOptions::default(), || {
                    let n = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(n, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(40));
                    active.fetch_sub(1, Ordering::SeqCst);
                    CodeModeResult::completed(json!(id), vec![], 1, 1, 1)
                })
            })
        };
        let x = spawn(a, 1);
        let y = spawn(b, 2);
        x.join().unwrap();
        y.join().unwrap();
        assert_eq!(max.load(Ordering::SeqCst), 1)
    }
    #[test]
    fn containment_status_never_queues() {
        let c = ctl(temp("status"), 0);
        let blocked = BlockedExecution::start(Arc::clone(&c), 1);
        let start = Instant::now();
        assert!(c.execute("status", &CodeModeOptions::default(), || {
            CodeModeResult::completed(json!(true), vec![], 0, 1, 1)
        }).error.is_none());
        assert!(start.elapsed() < Duration::from_millis(20));
        blocked.finish();
    }

    #[test]
    fn containment_dead_pid_lock_is_reclaimed() {
        let p = temp("dead");
        fs::create_dir(&p).unwrap();
        fs::write(p.join("pid"), "4294967294").unwrap();
        let permit = MachinePermit::acquire(&p, Instant::now() + Duration::from_secs(1)).unwrap();
        drop(permit);
        assert!(!p.exists())
    }
    #[test]
    fn containment_exact_status_short_circuits_closure() {
        let c = ctl(temp("exact-status"), 0);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_clone = Arc::clone(&calls);
        let result = c.execute("status", &CodeModeOptions::default(), || {
            calls_clone.fetch_add(1, Ordering::SeqCst);
            CodeModeResult::completed(json!(true), vec![], 0, 0, 0)
        });
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let expected = serde_json::to_value(c.snapshot()).unwrap();
        assert_eq!(result.value, Some(expected));
    }
    #[cfg(unix)]
    fn spawn_owned_test_child(c: &Controller) -> std::process::Child {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("sh");
        command.args(["-c", "sleep 5"]).process_group(0);
        let child = command.spawn().unwrap();
        let id = child.id();
        c.note_child(Some(id), Some(id), "running");
        child
    }

    #[cfg(unix)]
    fn assert_process_group_dead(id: u32) {
        assert!(
            !process_alive(id),
            "synthetic child {id} survived containment cleanup"
        );
    }

    #[cfg(unix)]
    #[test]
    fn containment_error_kills_registered_background_child() {
        use std::os::unix::process::CommandExt;

        let p = temp("background-cancel");
        let c = ctl(p.clone(), 1);
        let worker = Arc::clone(&c);
        let child_id = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&child_id);
        let child = Arc::new(Mutex::new(None));
        let child_slot = Arc::clone(&child);
        let result = c.execute(&heavy(30), &CodeModeOptions::default(), move || {
            let execution_id = heavy_execution_id().expect("heavy execution id");
            worker.reserve_background_job("synthetic-background", execution_id);
            let mut command = std::process::Command::new("sh");
            command.args(["-c", "sleep 30"]).process_group(0);
            let owned = command.spawn().unwrap();
            let id = owned.id();
            seen.store(id as usize, Ordering::SeqCst);
            worker.note_background_child("synthetic-background", Some(id), Some(id));
            *child_slot.lock().unwrap() = Some(owned);
            CodeModeResult::error_with_kind("cancelled", "synthetic cancellation", 0, true)
        });
        assert_eq!(result.error.unwrap().kind, "cancelled");
        let id = child_id.load(Ordering::SeqCst) as u32;
        child.lock().unwrap().take().unwrap().wait().unwrap();
        assert_process_group_dead(id);
        assert_eq!(c.snapshot().background_jobs, 0);
        assert!(!p.exists());
    }

    #[cfg(unix)]
    #[test]
    fn containment_cancel_kills_owned_child_and_frees_permit() {
        let p = temp("cancel");
        let c = ctl(p.clone(), 1);
        let worker = Arc::clone(&c);
        let child_id = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&child_id);
        let child = Arc::new(Mutex::new(None));
        let child_slot = Arc::clone(&child);
        let r = c.execute(&heavy(3), &CodeModeOptions::default(), move || {
            let owned = spawn_owned_test_child(&worker);
            let id = owned.id();
            seen.store(id as usize, Ordering::SeqCst);
            *child_slot.lock().unwrap() = Some(owned);
            panic!("synthetic cancellation")
        });
        assert_eq!(r.error.unwrap().kind, "containment_panic");
        assert!(!p.exists());
        let id = child_id.load(Ordering::SeqCst) as u32;
        assert!(id > 0);
        child.lock().unwrap().take().unwrap().wait().unwrap();
        assert_process_group_dead(id);
        assert_eq!(c.snapshot().active_heavy, 0)
    }

    #[cfg(unix)]
    #[test]
    fn containment_no_orphans_at_suite_end() {
        let p = temp("no-orphans");
        let c = ctl(p.clone(), 1);
        let worker = Arc::clone(&c);
        let child_id = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&child_id);
        let child = Arc::new(Mutex::new(None));
        let child_slot = Arc::clone(&child);
        let r = c.execute(&heavy(4), &CodeModeOptions::default(), move || {
            let owned = spawn_owned_test_child(&worker);
            let id = owned.id();
            seen.store(id as usize, Ordering::SeqCst);
            *child_slot.lock().unwrap() = Some(owned);
            CodeModeResult::error_with_kind("cancelled", "synthetic cancellation", 0, true)
        });
        assert_eq!(r.error.unwrap().kind, "cancelled");
        assert!(!p.exists());
        let id = child_id.load(Ordering::SeqCst) as u32;
        assert!(id > 0);
        child.lock().unwrap().take().unwrap().wait().unwrap();
        assert_process_group_dead(id);
        assert_eq!(c.snapshot().active_heavy, 0)
    }
}
