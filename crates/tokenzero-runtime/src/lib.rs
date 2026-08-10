#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokenzero_core::shell_display_command_from_argv_for_platform;
use wait_timeout::ChildExt;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

#[cfg(windows)]
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub const DEFAULT_SHELL_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_SHELL_SPILL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("empty command")]
    EmptyCommand,
    #[error("spawned command {0} pipe is unavailable")]
    MissingPipe(&'static str),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Argv,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimePlan {
    pub execution_mode: ExecutionMode,
    pub argv: Vec<String>,
    pub shell: Option<String>,
    pub shell_arg: Option<String>,
    pub cwd: Option<String>,
    pub platform: String,
    pub explicit_binary: bool,
    pub alias_dependency: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AllocatorPressureRelief {
    pub attempted: bool,
    pub reclaimed_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCapture {
    pub bytes_seen: usize,
    pub captured_bytes: usize,
    pub truncated: bool,
    /// Whether the in-memory captured bytes were valid UTF-8 without replacement.
    /// Defaults false for older records, so absence never authorizes exact text recovery.
    #[serde(default)]
    pub captured_utf8_lossless: bool,
    /// SHA-256 over every byte read from the stream, including bytes beyond
    /// the in-memory preview. Absence never authorizes exact recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_stream_sha256: Option<String>,
    pub spill_path: Option<String>,
    pub spill_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunOutputPolicy {
    pub per_stream_capture_bytes: usize,
    pub spill_threshold_bytes: usize,
    pub spill_dir: Option<PathBuf>,
}

impl Default for RunOutputPolicy {
    fn default() -> Self {
        let env_usize = |key, default| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(default)
        };
        Self {
            per_stream_capture_bytes: env_usize(
                "TOKENZERO_SHELL_CAPTURE_BYTES",
                DEFAULT_SHELL_CAPTURE_BYTES,
            ),
            spill_threshold_bytes: env_usize(
                "TOKENZERO_SHELL_SPILL_BYTES",
                DEFAULT_SHELL_SPILL_BYTES,
            ),
            spill_dir: std::env::var_os("TOKENZERO_SHELL_SPILL_DIR").map(PathBuf::from),
        }
        .normalized()
    }
}

impl RunOutputPolicy {
    pub fn normalized(mut self) -> Self {
        if self.per_stream_capture_bytes == 0 {
            self.per_stream_capture_bytes = DEFAULT_SHELL_CAPTURE_BYTES;
        }
        if self.spill_threshold_bytes == 0 {
            self.spill_threshold_bytes = DEFAULT_SHELL_SPILL_BYTES;
        }
        self.spill_threshold_bytes = self
            .spill_threshold_bytes
            .min(self.per_stream_capture_bytes);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub ok: bool,
    pub command: String,
    pub argv: Vec<String>,
    pub execution_mode: ExecutionMode,
    pub alias_dependency: bool,
    pub cwd: Option<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_capture: StreamCapture,
    pub stderr_capture: StreamCapture,
    pub capture_limit_bytes: usize,
    pub spill_threshold_bytes: usize,
    pub allocator_pressure_relief: AllocatorPressureRelief,
    pub timed_out: bool,
    /// Main child exited; group terminated after IO grace (not a timeout).
    #[serde(default)]
    pub io_grace_expired: bool,
    pub duration_ms: u128,
}

pub fn current_platform() -> &'static str {
    if cfg!(windows) { "windows" } else { "posix" }
}

pub fn plan_command(
    argv: &[String],
    cwd: Option<&Path>,
    explicit_shell: bool,
) -> Result<RuntimePlan, RuntimeError> {
    plan_command_for_platform(argv, cwd, explicit_shell, current_platform())
}

pub fn plan_command_for_platform(
    argv: &[String],
    cwd: Option<&Path>,
    explicit_shell: bool,
    platform: &str,
) -> Result<RuntimePlan, RuntimeError> {
    if argv.is_empty() || argv.iter().all(String::is_empty) {
        return Err(RuntimeError::EmptyCommand);
    }
    let make = |execution_mode, argv, shell, shell_arg, explicit_binary| RuntimePlan {
        execution_mode,
        argv,
        shell,
        shell_arg,
        cwd: cwd.map(|p| p.display().to_string()),
        platform: platform.into(),
        explicit_binary,
        alias_dependency: false,
    };
    let windows = matches!(platform, "windows" | "cmd" | "powershell" | "pwsh");
    let first = argv.first();
    let powershell = windows
        && !first.is_some_and(|v| is_windows_shell_host(v))
        && looks_like_powershell_syntax(&argv.join(" "));
    let needs_shell = explicit_shell
        || (argv.len() == 1 && contains_platform_shell_syntax(&argv[0], platform))
        || argv_has_shell_operator_tokens(argv)
        || powershell
        || (windows && first.is_some_and(|v| is_windows_shell_builtin(v)));
    if !needs_shell {
        return Ok(make(ExecutionMode::Argv, argv.to_vec(), None, None, true));
    }
    let (host, arg, syntax, prefix): (&str, &str, &str, &[&str]) = match (windows, powershell) {
        (true, true) => (
            "powershell",
            "-Command",
            "powershell",
            &["powershell", "-NoProfile", "-Command"],
        ),
        (true, false) => ("cmd", "/C", "cmd", &["cmd", "/C"]),
        // Pipefail prevents a successful final stage from masking an earlier failure.
        (false, _) => (
            "/bin/bash",
            "-c",
            "posix",
            &["/bin/bash", "-o", "pipefail", "-c"],
        ),
    };
    let mut shell_argv = prefix.iter().map(|s| (*s).into()).collect::<Vec<_>>();
    shell_argv.push(shell_command_string_from_argv(argv, syntax));
    Ok(make(
        ExecutionMode::Shell,
        shell_argv,
        Some(host.into()),
        Some(arg.into()),
        false,
    ))
}

fn shell_command_string_from_argv(argv: &[String], shell_platform: &str) -> String {
    if argv.len() == 1 {
        return argv[0].clone();
    }
    argv.iter()
        .map(|arg| {
            if is_shell_operator_token(arg) {
                arg.clone()
            } else {
                shell_display_command_from_argv_for_platform(
                    std::slice::from_ref(arg),
                    shell_platform,
                )
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn run_command(
    argv: &[String],
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
    stdin: Option<&str>,
    timeout: Duration,
    explicit_shell: bool,
) -> Result<RunResult, RuntimeError> {
    run_command_with_policy(
        argv,
        cwd,
        env_overrides,
        stdin,
        timeout,
        explicit_shell,
        RunOutputPolicy::default(),
    )
}

pub fn run_command_with_policy(
    argv: &[String],
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
    stdin: Option<&str>,
    timeout: Duration,
    explicit_shell: bool,
    output_policy: RunOutputPolicy,
) -> Result<RunResult, RuntimeError> {
    run_command_with_policy_observer(
        argv,
        cwd,
        env_overrides,
        stdin,
        timeout,
        explicit_shell,
        output_policy,
        |_, _, _| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_command_with_policy_observer<F>(
    argv: &[String],
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
    stdin: Option<&str>,
    timeout: Duration,
    explicit_shell: bool,
    output_policy: RunOutputPolicy,
    observer: F,
) -> Result<RunResult, RuntimeError>
where
    F: FnMut(Option<u32>, Option<u32>, &'static str),
{
    run_command_with_policy_observers(
        argv,
        cwd,
        env_overrides,
        stdin,
        timeout,
        explicit_shell,
        output_policy,
        observer,
        |_, _| {},
    )
}

#[allow(clippy::too_many_arguments)]
pub fn run_command_with_policy_observers<F, G>(
    argv: &[String],
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
    stdin: Option<&str>,
    timeout: Duration,
    explicit_shell: bool,
    output_policy: RunOutputPolicy,
    mut observer: F,
    stream_observer: G,
) -> Result<RunResult, RuntimeError>
where
    F: FnMut(Option<u32>, Option<u32>, &'static str),
    G: Fn(&'static str, &[u8]) + Send + Sync + 'static,
{
    let output_policy = output_policy.normalized();
    let plan = plan_command(argv, cwd, explicit_shell)?;
    let command_display = match plan.execution_mode {
        ExecutionMode::Shell => plan.argv.last().cloned().unwrap_or_else(|| argv.join(" ")),
        ExecutionMode::Argv => {
            shell_display_command_from_argv_for_platform(&plan.argv, &plan.platform)
        }
    };
    let result_command = command_display.clone();
    let result_argv = plan.argv.clone();
    let (result_mode, result_alias_dependency) = (plan.execution_mode, plan.alias_dependency);
    // Always echo the effective cwd. When the caller omits cwd the child inherits
    // process cwd — report that path instead of leaving telemetry null.
    let effective_cwd = cwd
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let result_cwd = effective_cwd
        .as_ref()
        .map(|path| path.display().to_string());
    let capture_limit_bytes = output_policy.per_stream_capture_bytes;
    let spill_threshold_bytes = output_policy.spill_threshold_bytes;
    let start = Instant::now();
    let (program, rest) = plan.argv.split_first().ok_or(RuntimeError::EmptyCommand)?;
    let mut command = match plan.execution_mode {
        ExecutionMode::Argv => {
            command_for_argv(program, rest, effective_cwd.as_deref(), env_overrides)
        }
        ExecutionMode::Shell => {
            let mut cmd = Command::new(program);
            cmd.args(rest);
            cmd
        }
    };
    if let Some(cwd) = effective_cwd.as_deref() {
        command.current_dir(cwd);
    }
    // Caller-selected commands only; explicit env overrides applied after scrub.
    scrub_inherited_orchestration_env(&mut command);
    if let Some(env) = env_overrides {
        command.envs(env);
    }
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let process_group = ProcessGroup::for_child(&child);
    let stdout = match required_child_pipe(child.stdout.take(), "stdout") {
        Ok(stdout) => stdout,
        Err(error) => {
            terminate_child_after_setup_error(&mut child, &process_group);
            return Err(error);
        }
    };
    let stderr = match required_child_pipe(child.stderr.take(), "stderr") {
        Ok(stderr) => stderr,
        Err(error) => {
            terminate_child_after_setup_error(&mut child, &process_group);
            return Err(error);
        }
    };
    let child_stdin = if stdin.is_some() {
        match required_child_pipe(child.stdin.take(), "stdin") {
            Ok(stdin) => Some(stdin),
            Err(error) => {
                terminate_child_after_setup_error(&mut child, &process_group);
                return Err(error);
            }
        }
    } else {
        None
    };
    observer(Some(child.id()), process_group.pgid(), "running");
    let stdout_policy = output_policy.clone();
    let stderr_policy = output_policy.clone();
    let stream_observer = Arc::new(stream_observer);
    let stdout_observer = Arc::clone(&stream_observer);
    let stderr_observer = Arc::clone(&stream_observer);
    let stdout_reader = spawn_io_worker("stdout reader", move || {
        capture_reader_with_observer(stdout, "stdout", stdout_policy, move |chunk| {
            stdout_observer("stdout", chunk);
        })
    });
    let stderr_reader = spawn_io_worker("stderr reader", move || {
        capture_reader_with_observer(stderr, "stderr", stderr_policy, move |chunk| {
            stderr_observer("stderr", chunk);
        })
    });
    // Stdin writes can block; keep them off the wait_timeout path.
    let stdin_writer = spawn_stdin_writer(stdin, child_stdin);
    let mut force_timed_out = false;
    let status = match child.wait_timeout(timeout)? {
        Some(status) => status,
        None => {
            force_timed_out = true;
            process_group.terminate();
            let _ = child.kill();
            child.wait()?
        }
    };
    let process_io = collect_process_io(
        stdin_writer,
        stdout_reader,
        stderr_reader,
        force_timed_out,
        start,
        timeout,
        process_group,
        !force_timed_out,
    )?;
    let timed_out = force_timed_out || process_io.timed_out;
    observer(
        None,
        None,
        if timed_out {
            "timed_out_killed"
        } else {
            "completed"
        },
    );
    let allocator_pressure_relief = allocator_pressure_relief_after_large_capture(
        &process_io.stdout.capture,
        &process_io.stderr.capture,
    );
    Ok(RunResult {
        ok: !timed_out && status.success(),
        command: result_command,
        argv: result_argv,
        execution_mode: result_mode,
        alias_dependency: result_alias_dependency,
        cwd: result_cwd,
        exit_code: status.code(),
        stdout: process_io.stdout.text,
        stderr: process_io.stderr.text,
        stdout_capture: process_io.stdout.capture,
        stderr_capture: process_io.stderr.capture,
        capture_limit_bytes,
        spill_threshold_bytes,
        allocator_pressure_relief,
        timed_out,
        io_grace_expired: process_io.io_grace_expired,
        duration_ms: start.elapsed().as_millis(),
    })
}

fn allocator_pressure_relief_after_large_capture(
    stdout: &StreamCapture,
    stderr: &StreamCapture,
) -> AllocatorPressureRelief {
    if [stdout, stderr]
        .iter()
        .any(|capture| capture.truncated || capture.spill_path.is_some())
    {
        platform_allocator_pressure_relief()
    } else {
        AllocatorPressureRelief::default()
    }
}

#[cfg(target_os = "macos")]
#[allow(
    unsafe_code,
    reason = "macOS allocator pressure relief requires a tiny FFI shim"
)]
fn platform_allocator_pressure_relief() -> AllocatorPressureRelief {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut c_void, goal: usize) -> usize;
    }
    let reclaimed = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    AllocatorPressureRelief {
        attempted: true,
        reclaimed_bytes: Some(reclaimed),
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_allocator_pressure_relief() -> AllocatorPressureRelief {
    AllocatorPressureRelief::default()
}

#[derive(Debug)]
struct CapturedStream {
    text: String,
    capture: StreamCapture,
}

struct ProcessIo {
    stdout: CapturedStream,
    stderr: CapturedStream,
    timed_out: bool,
    io_grace_expired: bool,
}

struct IoWorker<T> {
    name: &'static str,
    receiver: Receiver<std::io::Result<T>>,
    handle: Option<thread::JoinHandle<()>>,
}

fn spawn_io_worker<T: Send + 'static>(
    name: &'static str,
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> IoWorker<T> {
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || {
        let _ = sender.send(work());
    });
    IoWorker {
        name,
        receiver,
        handle: Some(handle),
    }
}

fn required_child_pipe<T>(pipe: Option<T>, name: &'static str) -> Result<T, RuntimeError> {
    pipe.ok_or(RuntimeError::MissingPipe(name))
}

fn terminate_child_after_setup_error(child: &mut std::process::Child, group: &ProcessGroup) {
    group.terminate();
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn_stdin_writer(input: Option<&str>, stdin: Option<ChildStdin>) -> Option<IoWorker<()>> {
    input.zip(stdin).map(|(input, mut stdin)| {
        let input = input.as_bytes().to_vec();
        spawn_io_worker("stdin writer", move || stdin.write_all(&input))
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_process_io(
    mut stdin: Option<IoWorker<()>>,
    mut stdout: IoWorker<CapturedStream>,
    mut stderr: IoWorker<CapturedStream>,
    tolerate_write_error: bool,
    start: Instant,
    timeout: Duration,
    group: ProcessGroup,
    child_exited: bool,
) -> Result<ProcessIo, RuntimeError> {
    let deadline = if child_exited {
        deadline_from(Instant::now(), CHILD_EXITED_IO_GRACE).min(deadline_from(start, timeout))
    } else {
        deadline_from(start, timeout)
    };
    let mut stdin_result = poll_stdin(stdin.as_mut(), deadline)?;
    let mut stdout_result = poll_worker(&mut stdout, deadline)?;
    let mut stderr_result = poll_worker(&mut stderr, deadline)?;
    let incomplete = stdin_result.is_none() || stdout_result.is_none() || stderr_result.is_none();
    let timed_out = incomplete && !child_exited;
    let io_grace_expired = incomplete && child_exited;
    if incomplete {
        // Unix: process-group kill closes inherited fds. Non-Unix: job terminate
        // kills the tree so pipe writers release and blocked readers can finish.
        group.terminate();
        let cleanup = deadline_from(Instant::now(), PROCESS_IO_SHUTDOWN_GRACE);
        stdin_result = stdin_result.or(poll_stdin(stdin.as_mut(), cleanup)?);
        stdout_result = stdout_result.or(poll_worker(&mut stdout, cleanup)?);
        stderr_result = stderr_result.or(poll_worker(&mut stderr, cleanup)?);
    }
    // Never return while leaving live reader JoinHandles detached: if cleanup
    // still left a worker blocked, terminate again and join with a final grace.
    stdin_result = ensure_worker_joined(stdin.as_mut(), stdin_result, &group)?;
    stdout_result = ensure_worker_joined(Some(&mut stdout), stdout_result, &group)?;
    stderr_result = ensure_worker_joined(Some(&mut stderr), stderr_result, &group)?;
    let stdin_result = stdin_result.ok_or_else(|| worker_timeout("shell stdin writer"))?;
    if !tolerate_write_error && !timed_out && !io_grace_expired {
        stdin_result?;
    }
    Ok(ProcessIo {
        stdout: stdout_result.ok_or_else(|| worker_timeout("shell stdout reader"))??,
        stderr: stderr_result.ok_or_else(|| worker_timeout("shell stderr reader"))??,
        timed_out,
        io_grace_expired,
    })
}

/// After process-tree terminate, require the worker to finish and be joined so
/// inherited-pipe readers are never detached on the timeout error path.
fn ensure_worker_joined<T>(
    worker: Option<&mut IoWorker<T>>,
    result: Option<std::io::Result<T>>,
    group: &ProcessGroup,
) -> Result<Option<std::io::Result<T>>, RuntimeError> {
    if result.is_some() {
        return Ok(result);
    }
    let Some(worker) = worker else {
        return Ok(None);
    };
    group.terminate();
    let final_grace = deadline_from(Instant::now(), PROCESS_IO_JOIN_GRACE);
    let recovered = poll_worker(worker, final_grace)?;
    if recovered.is_some() {
        return Ok(recovered);
    }
    // Last resort: hand the JoinHandle to a joiner thread so Drop never detaches
    // a live reader. With Windows job terminate (or Unix process-group kill),
    // this path should be rare; the joiner exits when the pipe finally closes.
    reaper_join_worker(worker);
    Ok(None)
}

fn reaper_join_worker<T>(worker: &mut IoWorker<T>) {
    if let Some(handle) = worker.handle.take() {
        let name = worker.name;
        thread::spawn(move || {
            if handle.join().is_err() {
                eprintln!("tokenzero-runtime: {name} panicked after IO shutdown grace");
            }
        });
    }
}

fn poll_stdin(
    worker: Option<&mut IoWorker<()>>,
    deadline: Instant,
) -> Result<Option<std::io::Result<()>>, RuntimeError> {
    worker.map_or(Ok(Some(Ok(()))), |worker| poll_worker(worker, deadline))
}

fn poll_worker<T>(
    worker: &mut IoWorker<T>,
    deadline: Instant,
) -> Result<Option<std::io::Result<T>>, RuntimeError> {
    match worker
        .receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
    {
        Ok(result) => {
            join_worker(worker)?;
            Ok(Some(result))
        }
        Err(RecvTimeoutError::Timeout) => Ok(None),
        Err(RecvTimeoutError::Disconnected) => {
            join_worker(worker)?;
            Err(RuntimeError::Io(std::io::Error::other(format!(
                "{} exited without reporting a result",
                worker.name
            ))))
        }
    }
}

fn join_worker<T>(worker: &mut IoWorker<T>) -> Result<(), RuntimeError> {
    if let Some(handle) = worker.handle.take() {
        handle.join().map_err(|_| {
            RuntimeError::Io(std::io::Error::other(format!("{} panicked", worker.name)))
        })?;
    }
    Ok(())
}

fn deadline_from(start: Instant, timeout: Duration) -> Instant {
    start.checked_add(timeout).unwrap_or_else(Instant::now)
}

const PROCESS_IO_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
const CHILD_EXITED_IO_GRACE: Duration = Duration::from_millis(250);
/// Final join grace after a second terminate when inherited pipes still block readers.
const PROCESS_IO_JOIN_GRACE: Duration = Duration::from_millis(500);

fn worker_timeout(name: &str) -> RuntimeError {
    RuntimeError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("{name} did not close after process timeout cleanup"),
    ))
}

struct ProcessGroup {
    #[cfg(unix)]
    pgid: Option<u32>,
    #[cfg(windows)]
    job: Option<windows_job::Job>,
    #[cfg(not(any(unix, windows)))]
    _unused: (),
}

impl ProcessGroup {
    fn pgid(&self) -> Option<u32> {
        #[cfg(unix)]
        {
            self.pgid
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    #[cfg(unix)]
    fn for_child(child: &std::process::Child) -> Self {
        Self {
            pgid: Some(child.id()),
        }
    }

    #[cfg(windows)]
    fn for_child(child: &std::process::Child) -> Self {
        Self {
            job: windows_job::Job::attach_child(child),
        }
    }

    #[cfg(not(any(unix, windows)))]
    fn for_child(_: &std::process::Child) -> Self {
        Self { _unused: () }
    }

    fn terminate(&self) {
        #[cfg(unix)]
        if let Some(pgid) = self.pgid {
            terminate_unix_process_group(pgid);
        }
        #[cfg(windows)]
        if let Some(job) = self.job.as_ref() {
            job.terminate();
        }
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    // Allow assigning the child into our job even when the parent already lives
    // inside a job (common under CI / shells). CREATE_SUSPENDED is not required
    // when AssignProcessToJobObject succeeds on a running process.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    command.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
}

#[cfg(not(any(unix, windows)))]
fn configure_process_group(_: &mut Command) {}

#[cfg(unix)]
fn terminate_unix_process_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    let target = format!("-{pgid}");
    for signal in ["-TERM", "-KILL"] {
        let _ = Command::new("kill")
            .args([signal, "--", &target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if signal == "-TERM" {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

/// Windows job-object containment so terminate() can kill descendants that still
/// hold inherited stdout/stderr write ends (unblocking our pipe readers).
#[cfg(windows)]
#[allow(
    unsafe_code,
    reason = "Windows Job Object APIs are not exposed safely in std"
)]
mod windows_job {
    use super::*;
    use std::ffi::c_void;
    use std::ptr;

    type HANDLE = *mut c_void;
    type BOOL = i32;
    type DWORD = u32;

    const JobObjectExtendedLimitInformation: u32 = 9;

    #[repr(C)]
    struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        per_process_user_time_limit: i64,
        per_job_user_time_limit: i64,
        limit_flags: DWORD,
        minimum_working_set_size: usize,
        maximum_working_set_size: usize,
        active_process_limit: DWORD,
        affinity: usize,
        priority_class: DWORD,
        scheduling_class: DWORD,
    }

    #[repr(C)]
    struct IO_COUNTERS {
        read_operation_count: u64,
        write_operation_count: u64,
        other_operation_count: u64,
        read_transfer_count: u64,
        write_transfer_count: u64,
        other_transfer_count: u64,
    }

    #[repr(C)]
    struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        basic_limit_information: JOBOBJECT_BASIC_LIMIT_INFORMATION,
        io_info: IO_COUNTERS,
        process_memory_limit: usize,
        job_memory_limit: usize,
        peak_process_memory_used: usize,
        peak_job_memory_used: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateJobObjectW(attrs: *mut c_void, name: *const u16) -> HANDLE;
        fn SetInformationJobObject(
            job: HANDLE,
            info_class: u32,
            info: *mut c_void,
            info_len: DWORD,
        ) -> BOOL;
        fn AssignProcessToJobObject(job: HANDLE, process: HANDLE) -> BOOL;
        fn TerminateJobObject(job: HANDLE, exit_code: DWORD) -> BOOL;
    }

    pub struct Job {
        handle: OwnedHandle,
    }

    impl Job {
        pub fn attach_child(child: &std::process::Child) -> Option<Self> {
            unsafe {
                let raw = CreateJobObjectW(ptr::null_mut(), ptr::null());
                if raw.is_null() {
                    return None;
                }
                let handle = OwnedHandle::from_raw_handle(raw as RawHandle);
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                    basic_limit_information: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        per_process_user_time_limit: 0,
                        per_job_user_time_limit: 0,
                        // No KILL_ON_JOB_CLOSE: successful captures must not reap
                        // intentional background descendants when the job handle drops.
                        // terminate() uses TerminateJobObject explicitly on cleanup paths.
                        limit_flags: 0,
                        minimum_working_set_size: 0,
                        maximum_working_set_size: 0,
                        active_process_limit: 0,
                        affinity: 0,
                        priority_class: 0,
                        scheduling_class: 0,
                    },
                    io_info: IO_COUNTERS {
                        read_operation_count: 0,
                        write_operation_count: 0,
                        other_operation_count: 0,
                        read_transfer_count: 0,
                        write_transfer_count: 0,
                        other_transfer_count: 0,
                    },
                    process_memory_limit: 0,
                    job_memory_limit: 0,
                    peak_process_memory_used: 0,
                    peak_job_memory_used: 0,
                };
                if SetInformationJobObject(
                    handle.as_raw_handle() as HANDLE,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut c_void,
                    std::mem::size_of_val(&info) as DWORD,
                ) == 0
                {
                    return None;
                }
                if AssignProcessToJobObject(
                    handle.as_raw_handle() as HANDLE,
                    child.as_raw_handle() as HANDLE,
                ) == 0
                {
                    return None;
                }
                Some(Self { handle })
            }
        }

        pub fn terminate(&self) {
            unsafe {
                let _ = TerminateJobObject(self.handle.as_raw_handle() as HANDLE, 1);
            }
        }
    }
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn capture_reader_with_observer<R: Read, F: FnMut(&[u8])>(
    mut reader: R,
    stream_name: &str,
    policy: RunOutputPolicy,
    mut observer: F,
) -> std::io::Result<CapturedStream> {
    let policy = policy.normalized();
    let mut captured = Vec::with_capacity(policy.per_stream_capture_bytes.min(64 * 1024));
    let mut bytes_seen = 0usize;
    let mut full_stream_hasher = Sha256::new();
    let mut spill = SpillWriter::new(stream_name, policy.spill_dir.as_deref());
    let mut buf = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        let chunk = &buf[..read];
        full_stream_hasher.update(chunk);
        observer(chunk);
        bytes_seen = bytes_seen.saturating_add(read);
        let captured_before = captured.len();
        if captured.len() < policy.per_stream_capture_bytes {
            let available = policy.per_stream_capture_bytes - captured.len();
            captured.extend_from_slice(&chunk[..read.min(available)]);
        }
        if bytes_seen > policy.spill_threshold_bytes {
            spill.write(chunk, captured_before, &captured)?;
        }
    }
    Ok(CapturedStream {
        text: String::from_utf8_lossy(&captured).into_owned(),
        capture: StreamCapture {
            bytes_seen,
            captured_bytes: captured.len(),
            truncated: bytes_seen > captured.len(),
            captured_utf8_lossless: std::str::from_utf8(&captured).is_ok(),
            full_stream_sha256: Some(lowercase_hex(&full_stream_hasher.finalize())),
            spill_path: spill.path.as_ref().map(|path| path.display().to_string()),
            spill_bytes: spill.bytes_written,
        },
    })
}

#[derive(Debug)]
struct SpillWriter {
    stream_name: String,
    dir: Option<PathBuf>,
    file: Option<File>,
    path: Option<PathBuf>,
    bytes_written: usize,
}

impl SpillWriter {
    fn new(stream_name: &str, dir: Option<&Path>) -> Self {
        Self {
            stream_name: stream_name.to_string(),
            dir: dir.map(Path::to_path_buf),
            file: None,
            path: None,
            bytes_written: 0,
        }
    }

    fn write(
        &mut self,
        chunk: &[u8],
        captured_before: usize,
        captured: &[u8],
    ) -> std::io::Result<()> {
        if self.file.is_none() {
            let root = self
                .dir
                .clone()
                .unwrap_or_else(|| std::env::temp_dir().join("tokenzero-spills"));
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let path = root.join(format!(
                "tokenzero-{}-{stamp}-{}.log",
                std::process::id(),
                self.stream_name
            ));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&path)?;
            file.write_all(&captured[..captured_before])?;
            self.bytes_written = self.bytes_written.saturating_add(captured_before);
            self.path = Some(path);
            self.file = Some(file);
        }
        self.file
            .as_mut()
            .expect("spill file initialized")
            .write_all(chunk)?;
        self.bytes_written = self.bytes_written.saturating_add(chunk.len());
        Ok(())
    }
}

/// Age after which a spill file is reclaimable (session path pointers expire).
pub const DEFAULT_SPILL_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Post-age-pass byte ceiling; oldest spills reclaimed first.
pub const DEFAULT_SPILL_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
/// Metadata-work ceiling: at most this many directory entries are visited per prune.
pub const DEFAULT_SPILL_MAX_SCAN_ENTRIES: usize = 4096;
/// Wall deadline for a prune pass; mid-scan work aborts once the deadline elapses.
pub const DEFAULT_SPILL_PRUNE_DEADLINE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default, Serialize)]
pub struct SpillPruneReport {
    pub dir: String,
    pub dry_run: bool,
    pub scanned_files: usize,
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub kept_files: usize,
    pub kept_bytes: u64,
    pub failed_removals: usize,
    /// Cap applied to directory enumeration (metadata-work bound).
    pub scan_budget: usize,
    /// True when enumeration stopped because `scan_budget` was exhausted.
    pub scan_truncated: bool,
    /// True when enumeration stopped because the prune deadline elapsed.
    pub deadline_elapsed: bool,
}

/// Prune with default scan budget and wall deadline.
pub fn prune_spill_dir(
    dir: &Path,
    max_age: Duration,
    max_total_bytes: u64,
    dry_run: bool,
) -> SpillPruneReport {
    prune_spill_dir_bounded(
        dir,
        max_age,
        max_total_bytes,
        dry_run,
        DEFAULT_SPILL_MAX_SCAN_ENTRIES,
        Some(Instant::now() + DEFAULT_SPILL_PRUNE_DEADLINE),
    )
}

/// Prune spill files with an explicit scanned-entry budget and optional deadline.
///
/// Storage-byte policy (`max_total_bytes`) is separate from metadata-work policy
/// (`max_scan_entries` / `deadline`): the latter bounds queue size and sort work
/// even when every visited file is a zero-byte fresh spill.
pub fn prune_spill_dir_bounded(
    dir: &Path,
    max_age: Duration,
    max_total_bytes: u64,
    dry_run: bool,
    max_scan_entries: usize,
    deadline: Option<Instant>,
) -> SpillPruneReport {
    let mut report = SpillPruneReport {
        dir: dir.display().to_string(),
        dry_run,
        scan_budget: max_scan_entries,
        ..Default::default()
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return report;
    };
    let now = SystemTime::now();
    let mut fresh = Vec::new();
    let mut visited = 0usize;
    for entry in entries.flatten().take(max_scan_entries) {
        if deadline.is_some_and(|limit| Instant::now() >= limit) {
            report.deadline_elapsed = true;
            break;
        }
        visited += 1;
        let path = entry.path();
        let valid_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("tokenzero-") && name.ends_with(".log"));
        let Ok(meta) = entry.metadata() else { continue };
        if !valid_name || !meta.is_file() {
            continue;
        }
        report.scanned_files += 1;
        let modified = meta.modified().unwrap_or(now);
        if now.duration_since(modified).is_ok_and(|age| age > max_age) {
            remove_spill_file(&path, meta.len(), dry_run, &mut report);
        } else {
            fresh.push((modified, meta.len(), path));
        }
    }
    report.scan_truncated = !report.deadline_elapsed && visited >= max_scan_entries;
    // Queue is already capped by the scan budget; sort work is O(B log B), not O(N log N).
    fresh.sort_by_key(|item| item.0);
    let mut bytes = fresh.iter().map(|item| item.1).sum::<u64>();
    let split = fresh
        .iter()
        .take_while(|item| {
            if bytes <= max_total_bytes {
                return false;
            }
            remove_spill_file(&item.2, item.1, dry_run, &mut report);
            bytes = bytes.saturating_sub(item.1);
            true
        })
        .count();
    for item in &fresh[split..] {
        report.kept_files += 1;
        report.kept_bytes += item.1;
    }
    report
}

fn remove_spill_file(path: &Path, len: u64, dry_run: bool, report: &mut SpillPruneReport) {
    if dry_run || fs::remove_file(path).is_ok() {
        report.removed_files += 1;
        report.removed_bytes += len;
    } else {
        report.failed_removals += 1;
    }
}

const ORCHESTRATION_ENV_PREFIXES: [&str; 4] = ["TOKENZERO_", "ZEROSTACK_", "FSZERO_", "GRAPHZERO_"];

fn scrub_inherited_orchestration_env(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|name| {
            ORCHESTRATION_ENV_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        }) {
            command.env_remove(key);
        }
    }
}

fn command_for_argv(
    program: &str,
    args: &[String],
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
) -> Command {
    #[cfg(windows)]
    {
        let resolved = resolve_windows_program(program, cwd, env_overrides);
        if resolved
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
        {
            let mut cmd = Command::new("cmd");
            cmd.arg("/D").arg("/S").arg("/C").arg(
                std::iter::once("call".to_string())
                    .chain(std::iter::once(quote_windows_cmd(
                        &resolved.display().to_string(),
                    )))
                    .chain(args.iter().map(|arg| quote_windows_cmd(arg)))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            return cmd;
        }
        let mut cmd = Command::new(resolved);
        cmd.args(args);
        cmd
    }
    #[cfg(not(windows))]
    {
        let _ = (cwd, env_overrides);
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd
    }
}

#[cfg(windows)]
fn resolve_windows_program(
    program: &str,
    cwd: Option<&Path>,
    env: Option<&BTreeMap<String, String>>,
) -> PathBuf {
    let raw = Path::new(program);
    let find = |path: &Path| {
        windows_program_candidates(path, env)
            .into_iter()
            .find(|candidate| candidate.exists())
    };
    if program.contains('\\') || program.contains('/') || raw.is_absolute() {
        return find(raw).unwrap_or_else(|| raw.into());
    }
    let mut dirs = cwd.map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
    if dirs.is_empty() {
        dirs.extend(std::env::current_dir());
    }
    if let Some(path) = env_value(env, "PATH").or_else(|| std::env::var("PATH").ok()) {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter()
        .find_map(|dir| find(&dir.join(program)))
        .unwrap_or_else(|| raw.into())
}

#[cfg(windows)]
fn windows_program_candidates(path: &Path, env: Option<&BTreeMap<String, String>>) -> Vec<PathBuf> {
    if path.extension().is_some() {
        return vec![path.into()];
    }
    let mut candidates = env_value(env, "PATHEXT")
        .or_else(|| std::env::var("PATHEXT").ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.starts_with('.') && ext.len() > 1)
        .map(|ext| {
            let mut name = path.as_os_str().to_os_string();
            name.push(ext.to_ascii_lowercase());
            PathBuf::from(name)
        })
        .collect::<Vec<_>>();
    candidates.push(path.into());
    candidates
}

#[cfg(windows)]
fn env_value(env: Option<&BTreeMap<String, String>>, key: &str) -> Option<String> {
    env?.iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.clone())
}

// Shell split/quote/platform helpers live in tokenzero_core (single source of truth).
pub use tokenzero_core::{
    argv_has_shell_operator_tokens, contains_platform_shell_syntax, contains_shell_syntax,
    is_shell_operator_token, is_windows_shell_builtin, is_windows_shell_host,
    looks_like_powershell_syntax, quote_for, quote_posix, quote_powershell, quote_windows_cmd,
    split_command_string, split_command_string_for_platform,
};

pub fn env_map(pairs: &[String]) -> Result<BTreeMap<String, String>, RuntimeError> {
    let mut out = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(RuntimeError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("--env requires KEY=VALUE, got {pair}"),
            )));
        };
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

#[cfg(test)]
mod stdio_error_tests {
    use super::*;

    #[test]
    fn missing_spawn_pipe_is_a_typed_runtime_error() {
        let error = required_child_pipe::<()>(None, "stdout").unwrap_err();
        assert!(matches!(&error, RuntimeError::MissingPipe("stdout")));
        assert_eq!(
            error.to_string(),
            "spawned command stdout pipe is unavailable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_pipes_capture_both_streams_and_write_stdin() {
        let argv = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "cat; printf err >&2".to_string(),
        ];
        let result = run_command(
            &argv,
            None,
            None,
            Some("input"),
            Duration::from_secs(2),
            false,
        )
        .unwrap();
        assert!(result.ok);
        assert_eq!(result.stdout, "input");
        assert_eq!(result.stderr, "err");
        assert!(result.stdout_capture.captured_utf8_lossless);
        assert!(result.stderr_capture.captured_utf8_lossless);
        assert_eq!(
            result.stdout_capture.full_stream_sha256.as_deref(),
            Some(tokenzero_core::sha256_hex("input").as_str())
        );
        assert_eq!(
            result.stderr_capture.full_stream_sha256.as_deref(),
            Some(tokenzero_core::sha256_hex("err").as_str())
        );
    }

    #[cfg(unix)]
    #[test]
    fn invalid_utf8_capture_is_never_labeled_lossless() {
        let argv = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf '\\377'".to_string(),
        ];
        let result = run_command(&argv, None, None, None, Duration::from_secs(2), false).unwrap();
        assert!(result.ok);
        assert!(!result.stdout_capture.captured_utf8_lossless);
        assert_eq!(result.stdout_capture.bytes_seen, 1);
        assert_eq!(
            result.stdout_capture.full_stream_sha256.as_deref(),
            Some(lowercase_hex(&Sha256::digest([0xff])).as_str())
        );
    }
}

#[cfg(test)]
mod inherited_pipe_join_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn unix_descendant_holding_pipes_returns_without_detaching_readers() {
        // Child exits immediately while a descendant keeps both pipes open.
        // Process-group terminate must close those writers so readers join
        // inside the cleanup/join grace instead of being detached on Drop.
        let argv = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 60 >/dev/null 2>&1 & exit 0".to_string(),
        ];
        let started = Instant::now();
        let result = run_command(&argv, None, None, None, Duration::from_secs(2), false)
            .expect("run_command should return");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "cleanup+join must finish well under the descendant sleep, took {:?}",
            started.elapsed()
        );
        assert!(result.ok || result.io_grace_expired || result.timed_out);
    }
}

#[cfg(test)]
mod spill_prune_bounds_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn prune_spill_dir_respects_scan_budget_before_sort() {
        let dir = tempdir().unwrap();
        for i in 0..64 {
            let path = dir.path().join(format!("tokenzero-{i}-stdout.log"));
            fs::write(&path, vec![b'x'; (i % 8) + 1]).unwrap();
        }
        let report = prune_spill_dir_bounded(dir.path(), DEFAULT_SPILL_TTL, 0, true, 16, None);
        assert_eq!(report.scan_budget, 16);
        assert!(report.scan_truncated);
        assert!(!report.deadline_elapsed);
        assert_eq!(report.scanned_files, 16);
        // Non-zero fresh spills with max_total_bytes=0 are all reclaimable.
        assert_eq!(report.removed_files, 16);
        assert_eq!(report.kept_files, 0);
    }

    #[test]
    fn prune_spill_dir_aborts_when_deadline_already_elapsed() {
        let dir = tempdir().unwrap();
        for i in 0..8 {
            fs::write(dir.path().join(format!("tokenzero-{i}-stdout.log")), []).unwrap();
        }
        let report = prune_spill_dir_bounded(
            dir.path(),
            DEFAULT_SPILL_TTL,
            DEFAULT_SPILL_MAX_TOTAL_BYTES,
            true,
            128,
            Some(Instant::now() - Duration::from_millis(1)),
        );
        assert!(report.deadline_elapsed);
        assert_eq!(report.scanned_files, 0);
        assert!(!report.scan_truncated);
    }
}
