#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokenzero_core::shell_display_command_from_argv_for_platform;
use wait_timeout::ChildExt;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

pub const DEFAULT_SHELL_CAPTURE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_SHELL_SPILL_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("empty command")]
    EmptyCommand,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocatorPressureRelief {
    pub attempted: bool,
    pub reclaimed_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamCapture {
    pub bytes_seen: usize,
    pub captured_bytes: usize,
    pub truncated: bool,
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
            std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
        };
        Self {
            per_stream_capture_bytes: env_usize("TOKENZERO_SHELL_CAPTURE_BYTES", DEFAULT_SHELL_CAPTURE_BYTES),
            spill_threshold_bytes: env_usize("TOKENZERO_SHELL_SPILL_BYTES", DEFAULT_SHELL_SPILL_BYTES),
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
        self.spill_threshold_bytes = self.spill_threshold_bytes.min(self.per_stream_capture_bytes);
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

#[derive(Debug)]
struct RunResultBuilder {
    command: String,
    argv: Vec<String>,
    execution_mode: ExecutionMode,
    alias_dependency: bool,
    cwd: Option<String>,
    capture_limit_bytes: usize,
    spill_threshold_bytes: usize,
}

impl RunResultBuilder {
    fn from_plan(
        command: String,
        plan: &RuntimePlan,
        cwd: Option<&Path>,
        output_policy: &RunOutputPolicy,
    ) -> Self {
        Self {
            command,
            argv: plan.argv.clone(),
            execution_mode: plan.execution_mode,
            alias_dependency: plan.alias_dependency,
            cwd: cwd.map(|p| p.display().to_string()),
            capture_limit_bytes: output_policy.per_stream_capture_bytes,
            spill_threshold_bytes: output_policy.spill_threshold_bytes,
        }
    }

    fn finish(
        self,
        ok: bool,
        exit_code: Option<i32>,
        process_io: ProcessIo,
        force_timed_out: bool,
        start: Instant,
    ) -> RunResult {
        let allocator_pressure_relief = allocator_pressure_relief_after_large_capture(
            &process_io.stdout.capture,
            &process_io.stderr.capture,
        );
        RunResult {
            ok, command: self.command, argv: self.argv, execution_mode: self.execution_mode,
            alias_dependency: self.alias_dependency, cwd: self.cwd, exit_code,
            stdout: process_io.stdout.text, stderr: process_io.stderr.text,
            stdout_capture: process_io.stdout.capture, stderr_capture: process_io.stderr.capture,
            capture_limit_bytes: self.capture_limit_bytes,
            spill_threshold_bytes: self.spill_threshold_bytes,
            allocator_pressure_relief,
            timed_out: force_timed_out || process_io.timed_out,
            io_grace_expired: process_io.io_grace_expired,
            duration_ms: start.elapsed().as_millis(),
        }
    }
}

pub fn current_platform() -> &'static str {
    if cfg!(windows) { "windows" } else { "posix" }
}

fn runtime_plan(
    execution_mode: ExecutionMode,
    argv: Vec<String>,
    shell: Option<String>,
    shell_arg: Option<String>,
    cwd: Option<&Path>,
    platform: &str,
    explicit_binary: bool,
) -> RuntimePlan {
    RuntimePlan {
        execution_mode,
        argv,
        shell,
        shell_arg,
        cwd: cwd.map(|p| p.display().to_string()),
        platform: platform.to_string(),
        explicit_binary,
        alias_dependency: false,
    }
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
    if argv.is_empty() || argv.iter().all(|v| v.is_empty()) {
        return Err(RuntimeError::EmptyCommand);
    }
    let joined_command = argv.join(" ");
    let windows = matches!(platform, "windows" | "cmd" | "powershell" | "pwsh");
    let first = argv.first();
    let powershell_script = windows
        && !first.is_some_and(|value| is_windows_shell_host(value))
        && looks_like_powershell_syntax(&joined_command);
    let needs_shell = explicit_shell
        || (argv.len() == 1 && contains_platform_shell_syntax(&argv[0], platform))
        || argv_has_shell_operator_tokens(argv)
        || powershell_script
        || (windows && first.is_some_and(|value| is_windows_shell_builtin(value)));
    if !needs_shell {
        return Ok(runtime_plan(
            ExecutionMode::Argv,
            argv.to_vec(),
            None,
            None,
            cwd,
            platform,
            true,
        ));
    }
    let (shell, shell_arg, shell_platform, prefix): (&str, &str, &str, &[&str]) =
        if windows && powershell_script {
            ("powershell", "-Command", "powershell", &["powershell", "-NoProfile", "-Command"])
        } else if windows {
            ("cmd", "/C", "cmd", &["cmd", "/C"])
        } else {
            ("/bin/sh", "-c", "posix", &["/bin/sh", "-c"])
        };
    let command_string = shell_command_string_from_argv(argv, shell_platform);
    let mut shell_argv = prefix.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    shell_argv.push(command_string);
    Ok(runtime_plan(
        ExecutionMode::Shell,
        shell_argv,
        Some(shell.to_string()),
        Some(shell_arg.to_string()),
        cwd,
        platform,
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
                shell_display_command_from_argv_for_platform(std::slice::from_ref(arg), shell_platform)
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
    run_command_with_policy(argv, cwd, env_overrides, stdin, timeout, explicit_shell, RunOutputPolicy::default())
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
    run_command_with_policy_observer(argv, cwd, env_overrides, stdin, timeout, explicit_shell, output_policy, |_, _, _| {})
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
    mut observer: F,
) -> Result<RunResult, RuntimeError>
where
    F: FnMut(Option<u32>, Option<u32>, &'static str),
{
    let output_policy = output_policy.normalized();
    let plan = plan_command(argv, cwd, explicit_shell)?;
    let command_display = command_display_for_plan(argv, &plan);
    let result_builder =
        RunResultBuilder::from_plan(command_display.clone(), &plan, cwd, &output_policy);
    let start = Instant::now();
    let (program, rest) = plan.argv.split_first().ok_or(RuntimeError::EmptyCommand)?;
    let mut command = match plan.execution_mode {
        ExecutionMode::Argv => command_for_argv(program, rest, cwd, env_overrides),
        ExecutionMode::Shell => {
            let mut cmd = Command::new(program);
            cmd.args(rest);
            cmd
        }
    };
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    // Caller-selected commands only; explicit env overrides applied after scrub.
    scrub_inherited_orchestration_env(&mut command);
    if let Some(env) = env_overrides {
        command.envs(env);
    }
    command.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let process_group = ProcessGroup::for_child(&child);
    observer(Some(child.id()), process_group.pgid(), "running");
    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let stdout_policy = output_policy.clone();
    let stderr_policy = output_policy.clone();
    let stdout_reader = spawn_io_worker("stdout reader", move || {
        capture_reader(stdout, "stdout", stdout_policy)
    });
    let stderr_reader = spawn_io_worker("stderr reader", move || {
        capture_reader(stderr, "stderr", stderr_policy)
    });
    // Stdin writes can block; keep them off the wait_timeout path.
    let stdin_writer = spawn_stdin_writer(stdin, child.stdin.take());
    let status = match child.wait_timeout(timeout)? {
        Some(status) => status,
        None => {
            process_group.terminate();
            let _ = child.kill();
            let status = child.wait()?;
            let process_io = collect_process_io(
                stdin_writer, stdout_reader, stderr_reader, true, start, timeout, process_group, false,
            )?;
            observer(None, None, "timed_out_killed");
            return Ok(result_builder.finish(false, status.code(), process_io, true, start));
        }
    };
    let process_io = collect_process_io(
        stdin_writer, stdout_reader, stderr_reader, false, start, timeout, process_group, true,
    )?;
    observer(
        None,
        None,
        if process_io.timed_out {
            "timed_out_killed"
        } else {
            "completed"
        },
    );
    Ok(result_builder.finish(
        !process_io.timed_out && status.success(),
        status.code(),
        process_io,
        false,
        start,
    ))
}

fn command_display_for_plan(input_argv: &[String], plan: &RuntimePlan) -> String {
    match plan.execution_mode {
        ExecutionMode::Shell => plan.argv.last().cloned().unwrap_or_else(|| input_argv.join(" ")),
        ExecutionMode::Argv => {
            command_display_for_execution_mode(&plan.argv, plan.execution_mode, &plan.platform)
        }
    }
}

fn command_display_for_execution_mode(
    argv: &[String],
    execution_mode: ExecutionMode,
    platform: &str,
) -> String {
    match execution_mode {
        ExecutionMode::Shell => argv.join(" "),
        ExecutionMode::Argv => shell_display_command_from_argv_for_platform(argv, platform),
    }
}

fn allocator_pressure_relief_after_large_capture(
    stdout: &StreamCapture,
    stderr: &StreamCapture,
) -> AllocatorPressureRelief {
    let large_capture = stdout.truncated
        || stderr.truncated
        || stdout.spill_path.is_some()
        || stderr.spill_path.is_some();
    if !large_capture {
        return AllocatorPressureRelief { attempted: false, reclaimed_bytes: None };
    }
    platform_allocator_pressure_relief()
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

    // SAFETY: matches macOS malloc ABI; null zone = all zones, goal 0 = reclaim max.
    // No Rust allocations cross the FFI boundary; return is telemetry only.
    let reclaimed = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    AllocatorPressureRelief { attempted: true, reclaimed_bytes: Some(reclaimed) }
}

#[cfg(not(target_os = "macos"))]
fn platform_allocator_pressure_relief() -> AllocatorPressureRelief {
    AllocatorPressureRelief { attempted: false, reclaimed_bytes: None }
}

#[derive(Debug)]
struct CapturedStream {
    text: String,
    capture: StreamCapture,
}

fn spawn_stdin_writer(
    input: Option<&str>,
    child_stdin: Option<ChildStdin>,
) -> Option<IoWorker<()>> {
    input.map(|input| {
        let input = input.as_bytes().to_vec();
        let mut child_stdin = child_stdin.expect("stdin is piped");
        spawn_io_worker("stdin writer", move || child_stdin.write_all(&input))
    })
}

#[derive(Debug)]
struct ProcessIo {
    stdout: CapturedStream,
    stderr: CapturedStream,
    timed_out: bool,
    io_grace_expired: bool,
}

#[allow(clippy::too_many_arguments)]
fn collect_process_io(
    mut stdin_writer: Option<IoWorker<()>>,
    mut stdout_reader: IoWorker<CapturedStream>,
    mut stderr_reader: IoWorker<CapturedStream>,
    tolerate_write_error: bool,
    start: Instant,
    timeout: Duration,
    process_group: ProcessGroup,
    child_exited: bool,
) -> Result<ProcessIo, RuntimeError> {
    // Exited main child: short grace then group terminate (no false timeout).
    let deadline = if child_exited {
        Instant::now()
            .checked_add(child_exited_io_grace())
            .unwrap_or_else(Instant::now)
            .min(deadline_from(start, timeout))
    } else {
        deadline_from(start, timeout)
    };
    let mut timed_out = false;
    let mut io_grace_expired = false;
    let mut stdin_result = poll_stdin_until(stdin_writer.as_mut(), deadline)?;
    let mut stdout_result = poll_worker_until(&mut stdout_reader, deadline)?;
    let mut stderr_result = poll_worker_until(&mut stderr_reader, deadline)?;

    if stdin_result.is_none() || stdout_result.is_none() || stderr_result.is_none() {
        if child_exited {
            io_grace_expired = true;
        } else {
            timed_out = true;
        }
        process_group.terminate();
        let cleanup_deadline = Instant::now()
            .checked_add(process_io_shutdown_grace())
            .unwrap_or_else(Instant::now);
        if stdin_result.is_none() {
            stdin_result = poll_stdin_until(stdin_writer.as_mut(), cleanup_deadline)?;
        }
        if stdout_result.is_none() {
            stdout_result = poll_worker_until(&mut stdout_reader, cleanup_deadline)?;
        }
        if stderr_result.is_none() {
            stderr_result = poll_worker_until(&mut stderr_reader, cleanup_deadline)?;
        }
    }

    let stdin_result = stdin_result.ok_or_else(|| timed_out_worker_error("shell stdin writer"))?;
    if !tolerate_write_error && !timed_out && !io_grace_expired {
        stdin_result.map_err(RuntimeError::Io)?;
    }
    let stdout = take_stream_result(stdout_result, "shell stdout reader")?;
    let stderr = take_stream_result(stderr_result, "shell stderr reader")?;
    Ok(ProcessIo {
        stdout,
        stderr,
        timed_out,
        io_grace_expired,
    })
}

fn poll_stdin_until(
    writer: Option<&mut IoWorker<()>>,
    deadline: Instant,
) -> Result<Option<std::io::Result<()>>, RuntimeError> {
    match writer {
        Some(writer) => poll_worker_until(writer, deadline),
        None => Ok(Some(Ok(()))),
    }
}

fn take_stream_result(
    result: Option<std::io::Result<CapturedStream>>,
    name: &'static str,
) -> Result<CapturedStream, RuntimeError> {
    result.ok_or_else(|| timed_out_worker_error(name))?.map_err(RuntimeError::Io)
}

struct IoWorker<T> {
    name: &'static str,
    receiver: Receiver<std::io::Result<T>>,
    handle: Option<thread::JoinHandle<()>>,
}

fn spawn_io_worker<T, F>(name: &'static str, work: F) -> IoWorker<T>
where
    T: Send + 'static,
    F: FnOnce() -> std::io::Result<T> + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    let handle = thread::spawn(move || { let _ = sender.send(work()); });
    IoWorker { name, receiver, handle: Some(handle) }
}

fn poll_worker_until<T>(
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

// After group terminate: room for workers to observe pipe close (under 5s test sleeps).
fn process_io_shutdown_grace() -> Duration { Duration::from_secs(2) }

// Main child already exited: open pipes belong to background descendants.
fn child_exited_io_grace() -> Duration { Duration::from_millis(250) }

fn timed_out_worker_error(name: &str) -> RuntimeError {
    RuntimeError::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("{name} did not close after process timeout cleanup"),
    ))
}

#[derive(Clone, Copy)]
struct ProcessGroup {
    #[cfg(unix)]
    pgid: u32,
}

impl ProcessGroup {
    fn pgid(self) -> Option<u32> {
        #[cfg(unix)]
        { Some(self.pgid) }
        #[cfg(not(unix))]
        { None }
    }

    fn for_child(child: &std::process::Child) -> Self {
        #[cfg(not(unix))]
        let _ = child;
        Self {
            #[cfg(unix)]
            pgid: child.id(),
        }
    }

    fn terminate(self) {
        #[cfg(unix)]
        terminate_unix_process_group(self.pgid);
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_unix_process_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    // The "--" separator is load-bearing: Ubuntu's procps kill accepts
    // `kill -TERM -<pgid>` with exit 0 yet signals nothing, so the group
    // kill silently no-ops without it (Debian and macOS tolerate both).
    let target = format!("-{pgid}");
    let kill = |sig: &str| {
        let _ = Command::new("kill")
            .args([sig, "--", &target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    };
    kill("-TERM");
    thread::sleep(Duration::from_millis(50));
    kill("-KILL");
}

fn capture_reader<R: Read>(
    mut reader: R,
    stream_name: &str,
    policy: RunOutputPolicy,
) -> std::io::Result<CapturedStream> {
    let policy = policy.normalized();
    let mut captured = Vec::with_capacity(policy.per_stream_capture_bytes.min(64 * 1024));
    let mut bytes_seen = 0usize;
    let mut spill = SpillWriter::new(stream_name, policy.spill_dir.as_deref());
    let mut buf = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buf)?;
        if read == 0 {
            break;
        }
        let chunk = &buf[..read];
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
            spill_path: spill.path_string(),
            spill_bytes: spill.bytes_written(),
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
            let path = self.create_path()?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut file = File::create(&path)?;
            file.write_all(&captured[..captured_before])?;
            self.bytes_written = self.bytes_written.saturating_add(captured_before);
            self.path = Some(path);
            self.file = Some(file);
        }
        if let Some(file) = self.file.as_mut() {
            file.write_all(chunk)?;
            self.bytes_written = self.bytes_written.saturating_add(chunk.len());
        }
        Ok(())
    }

    fn create_path(&self) -> std::io::Result<PathBuf> {
        let root = self.dir.clone().unwrap_or_else(|| std::env::temp_dir().join("tokenzero-spills"));
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        Ok(root.join(format!("tokenzero-{}-{stamp}-{}.log", std::process::id(), self.stream_name)))
    }

    fn path_string(&self) -> Option<String> {
        self.path.as_ref().map(|path| path.display().to_string())
    }

    fn bytes_written(&self) -> usize { self.bytes_written }
}

/// Age after which a spill file is reclaimable (session path pointers expire).
pub const DEFAULT_SPILL_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Post-age-pass byte ceiling; oldest spills reclaimed first.
pub const DEFAULT_SPILL_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

/// Spill-directory prune outcome (`removed_*` is prospective under `dry_run`).
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
}

/// Reclaim `tokenzero-*.log` spills older than `max_age`, then oldest-first
/// until `max_total_bytes`. Failures are counted; missing dir → empty report.
pub fn prune_spill_dir(
    dir: &Path,
    max_age: Duration,
    max_total_bytes: u64,
    dry_run: bool,
) -> SpillPruneReport {
    let mut report = SpillPruneReport {
        dir: dir.display().to_string(),
        dry_run,
        ..SpillPruneReport::default()
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return report;
    };
    let now = SystemTime::now();
    let mut fresh: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        if !name.starts_with("tokenzero-") || !name.ends_with(".log") { continue; }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() { continue; }
        report.scanned_files += 1;
        let modified = meta.modified().unwrap_or(now);
        let expired = now.duration_since(modified).map(|age| age > max_age).unwrap_or(false);
        if expired {
            remove_spill_file(&path, meta.len(), dry_run, &mut report);
        } else {
            fresh.push((modified, meta.len(), path));
        }
    }
    fresh.sort_by_key(|(modified, _, _)| *modified);
    let mut fresh_bytes: u64 = fresh.iter().map(|(_, len, _)| *len).sum();
    let mut evict_until = 0;
    while fresh_bytes > max_total_bytes && evict_until < fresh.len() {
        let (_, len, path) = &fresh[evict_until];
        remove_spill_file(path, *len, dry_run, &mut report);
        fresh_bytes = fresh_bytes.saturating_sub(*len);
        evict_until += 1;
    }
    for (_, len, _) in &fresh[evict_until..] {
        report.kept_files += 1;
        report.kept_bytes += *len;
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
            ORCHESTRATION_ENV_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
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
        if is_windows_batch_file(&resolved) {
            let mut cmd = Command::new("cmd");
            cmd.arg("/D")
                .arg("/S")
                .arg("/C")
                .arg(windows_batch_call_command(&resolved, args));
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
fn windows_batch_call_command(resolved: &Path, args: &[String]) -> String {
    std::iter::once("call".to_string())
        .chain(std::iter::once(quote_windows_cmd(
            &resolved.display().to_string(),
        )))
        .chain(args.iter().map(|arg| quote_windows_cmd(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
fn resolve_windows_program(
    program: &str,
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
) -> PathBuf {
    let raw = Path::new(program);
    let first_existing = |base: &Path| {
        windows_program_candidates(base, env_overrides)
            .into_iter()
            .find(|candidate| candidate.exists())
    };
    if has_windows_path_separator(program) || raw.is_absolute() {
        return first_existing(raw).unwrap_or_else(|| raw.to_path_buf());
    }
    for dir in windows_search_dirs(cwd, env_overrides) {
        if let Some(found) = first_existing(&dir.join(program)) {
            return found;
        }
    }
    raw.to_path_buf()
}

#[cfg(windows)]
fn windows_search_dirs(
    cwd: Option<&Path>,
    env_overrides: Option<&BTreeMap<String, String>>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(cwd) = cwd {
        dirs.push(cwd.to_path_buf());
    } else if let Ok(current) = std::env::current_dir() {
        dirs.push(current);
    }
    if let Some(path) = env_value(env_overrides, "PATH").or_else(|| std::env::var("PATH").ok()) {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs
}

#[cfg(windows)]
fn windows_program_candidates(
    path: &Path,
    env_overrides: Option<&BTreeMap<String, String>>,
) -> Vec<PathBuf> {
    if path.extension().is_none() {
        let mut candidates: Vec<PathBuf> = windows_pathexts(env_overrides)
            .into_iter()
            .map(|ext| {
                let mut with_ext = path.as_os_str().to_os_string();
                with_ext.push(ext);
                PathBuf::from(with_ext)
            })
            .collect();
        candidates.push(path.to_path_buf());
        return candidates;
    }
    vec![path.to_path_buf()]
}

#[cfg(windows)]
fn windows_pathexts(env_overrides: Option<&BTreeMap<String, String>>) -> Vec<String> {
    env_value(env_overrides, "PATHEXT")
        .or_else(|| std::env::var("PATHEXT").ok())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string())
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.starts_with('.') && ext.len() > 1)
        .map(|ext| ext.to_ascii_lowercase())
        .collect()
}

#[cfg(windows)]
fn env_value(env_overrides: Option<&BTreeMap<String, String>>, key: &str) -> Option<String> {
    env_overrides.and_then(|env| {
        env.iter().find(|(c, _)| c.eq_ignore_ascii_case(key)).map(|(_, v)| v.clone())
    })
}

#[cfg(windows)]
fn has_windows_path_separator(program: &str) -> bool {
    program.contains('\\') || program.contains('/')
}

#[cfg(windows)]
fn is_windows_batch_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
        ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat")
    })
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
mod tests;
