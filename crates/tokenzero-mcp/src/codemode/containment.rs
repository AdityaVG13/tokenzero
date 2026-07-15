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
const DEFAULT_MACHINE_PERMIT: &str = "/tmp/zerostack-codemode-heavy.permit";
const PERMIT_POLL: Duration = Duration::from_millis(20);
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
    permit_path: PathBuf,
}
impl Default for Config {
    fn default() -> Self {
        let [max_active, max_queue_depth, cost_threshold] =
            CONFIG_LIMITS.map(|(name, default, minimum)| env_usize(name, default).max(minimum));
        Self {
            max_active,
            max_queue_depth,
            cost_threshold,
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
        let result = if matches!(
            class,
            ExecutionClass::HeavyShell | ExecutionClass::HeavyEstimatedCost
        ) {
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
    fn acquire(path: &Path, deadline: Instant) -> Result<Self, String> {
        loop {
            match fs::create_dir(path) {
                Ok(()) => {
                    let owner = format!(
                        "{}-{}-{:?}",
                        std::process::id(),
                        epoch_millis(),
                        std::thread::current().id()
                    );
                    if let Err(e) = write_metadata(path, &owner) {
                        cleanup_owned(path, &owner);
                        return Err(format!("write heavy permit metadata: {e}"));
                    }
                    return Ok(Self(path.to_path_buf(), owner));
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
        cleanup_owned(&self.0, &self.1)
    }
}
fn write_metadata(path: &Path, owner: &str) -> std::io::Result<()> {
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
    fs::write(path.join("command"), "tokenzero-codemode-heavy")?;
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
    let Some(pid) = fs::read_to_string(path.join("pid"))
        .ok()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    else {
        return false;
    };
    !process_alive(pid) && remove_permit(path)
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
fn busy_result(code: &str, message: &str) -> CodeModeResult {
    let mut r = CodeModeResult::error_with_kind("busy", format!("{code}: {message}"), 0, true);
    r.telemetry.extra = Some(
        json!({"backpressure":{"class":"busy","code":code,"retryable":true,"retry_strategy":"exponential_backoff"}}),
    );
    r
}
const STATUS_PREFIXES: &[&str] = &["search:", "describe:"];
const STATUS_MARKERS: &[&str] = &[
    "codemode.limits",
    "journaldoctor",
    "journal_doctor",
    "metrics",
    "status",
];
const SHELL_MARKERS: &[&str] = &[".shell(", "tz_shell", "\"shell\"", "'shell'"];

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
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
fn default_machine_permit() -> PathBuf {
    if cfg!(test) {
        let pid = std::process::id();
        std::env::temp_dir().join(format!("zerostack-codemode-heavy-test-{pid}.permit"))
    } else {
        PathBuf::from(DEFAULT_MACHINE_PERMIT)
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
