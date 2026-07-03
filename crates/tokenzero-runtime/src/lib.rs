#![deny(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::OsString;
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
        let per_stream_capture_bytes = std::env::var("TOKENZERO_SHELL_CAPTURE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_SHELL_CAPTURE_BYTES);
        let spill_threshold_bytes = std::env::var("TOKENZERO_SHELL_SPILL_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(DEFAULT_SHELL_SPILL_BYTES);
        let spill_dir = std::env::var_os("TOKENZERO_SHELL_SPILL_DIR").map(PathBuf::from);
        Self {
            per_stream_capture_bytes,
            spill_threshold_bytes,
            spill_dir,
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
    /// The main child exited, but background descendants kept the stdio
    /// pipes open past the IO grace window and the group was terminated.
    /// Honest success rather than a timeout; captured output may stop early.
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
            ok,
            command: self.command,
            argv: self.argv,
            execution_mode: self.execution_mode,
            alias_dependency: self.alias_dependency,
            cwd: self.cwd,
            exit_code,
            stdout: process_io.stdout.text,
            stderr: process_io.stderr.text,
            stdout_capture: process_io.stdout.capture,
            stderr_capture: process_io.stderr.capture,
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
    let powershell_script = windows
        && !argv
            .first()
            .is_some_and(|value| is_windows_shell_host(value))
        && looks_like_powershell_syntax(&joined_command);
    let needs_shell = explicit_shell
        || argv.len() == 1 && contains_platform_shell_syntax(&argv[0], platform)
        || argv_has_shell_operator_tokens(argv)
        || powershell_script
        || windows
            && argv
                .first()
                .is_some_and(|value| is_windows_shell_builtin(value));
    if needs_shell {
        let (shell, shell_arg, shell_platform) = if windows && powershell_script {
            (
                "powershell".to_string(),
                "-Command".to_string(),
                "powershell",
            )
        } else if windows {
            ("cmd".to_string(), "/C".to_string(), "cmd")
        } else {
            ("/bin/sh".to_string(), "-c".to_string(), "posix")
        };
        let command_string = shell_command_string_from_argv(argv, shell_platform);
        let shell_argv = if windows && powershell_script {
            vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                command_string,
            ]
        } else if windows {
            vec!["cmd".to_string(), "/C".to_string(), command_string]
        } else {
            vec!["/bin/sh".to_string(), "-c".to_string(), command_string]
        };
        Ok(RuntimePlan {
            execution_mode: ExecutionMode::Shell,
            argv: shell_argv,
            shell: Some(shell),
            shell_arg: Some(shell_arg),
            cwd: cwd.map(|p| p.display().to_string()),
            platform: platform.to_string(),
            explicit_binary: false,
            alias_dependency: false,
        })
    } else {
        Ok(RuntimePlan {
            execution_mode: ExecutionMode::Argv,
            argv: argv.to_vec(),
            shell: None,
            shell_arg: None,
            cwd: cwd.map(|p| p.display().to_string()),
            platform: platform.to_string(),
            explicit_binary: true,
            alias_dependency: false,
        })
    }
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
    let output_policy = output_policy.normalized();
    let plan = plan_command(argv, cwd, explicit_shell)?;
    let command_display = command_display_for_plan(argv, &plan);
    let result_builder =
        RunResultBuilder::from_plan(command_display.clone(), &plan, cwd, &output_policy);
    let start = Instant::now();
    let mut command = match plan.execution_mode {
        ExecutionMode::Argv => {
            // argv is non-empty for a successful plan (plan_command rejects empty
            // commands), but split_first makes that explicit and panic-proof at
            // the process-spawn boundary rather than relying on the invariant.
            let (program, rest) = plan.argv.split_first().ok_or(RuntimeError::EmptyCommand)?;
            command_for_argv(program, rest, cwd, env_overrides)
        }
        ExecutionMode::Shell => {
            let (program, rest) = plan.argv.split_first().ok_or(RuntimeError::EmptyCommand)?;
            let mut cmd = Command::new(program);
            cmd.args(rest);
            cmd
        }
    };
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if let Some(env) = env_overrides {
        command.envs(env);
    }
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    configure_process_group(&mut command);
    let mut child = command.spawn()?;
    let process_group = ProcessGroup::for_child(&child);
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
    // SAFETY: stdin writes can block indefinitely when the child keeps the pipe
    // open but never reads it. Keep the writer on a separate thread so
    // wait_timeout always remains the authority for child-process liveness.
    let stdin_writer = spawn_stdin_writer(stdin, child.stdin.take());
    let status = match child.wait_timeout(timeout)? {
        Some(status) => status,
        None => {
            process_group.terminate();
            let _ = child.kill();
            let status = child.wait()?;
            let process_io = collect_process_io(
                stdin_writer,
                stdout_reader,
                stderr_reader,
                true,
                start,
                timeout,
                process_group,
                false,
            )?;
            return Ok(result_builder.finish(false, status.code(), process_io, true, start));
        }
    };
    let process_io = collect_process_io(
        stdin_writer,
        stdout_reader,
        stderr_reader,
        false,
        start,
        timeout,
        process_group,
        true,
    )?;
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
        ExecutionMode::Shell => plan
            .argv
            .last()
            .cloned()
            .unwrap_or_else(|| input_argv.join(" ")),
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
        return AllocatorPressureRelief {
            attempted: false,
            reclaimed_bytes: None,
        };
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

    // SAFETY: this declaration matches the macOS malloc ABI:
    // `size_t malloc_zone_pressure_relief(malloc_zone_t *zone, size_t goal)`.
    // `malloc_zone_t` is opaque, so `*mut c_void` is only used as the ABI
    // carrier. Passing a null zone is the documented request to examine all
    // zones, and `goal = 0` asks malloc to reclaim as much cached memory as it
    // can. No Rust allocation or borrowed pointer is passed to C, no ownership
    // crosses the boundary, and the return value is only telemetry.
    let reclaimed = unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
    AllocatorPressureRelief {
        attempted: true,
        reclaimed_bytes: Some(reclaimed),
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_allocator_pressure_relief() -> AllocatorPressureRelief {
    AllocatorPressureRelief {
        attempted: false,
        reclaimed_bytes: None,
    }
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
    // A dead main child cannot produce more output; any still-open pipe is
    // held by a background descendant. Collect for a short grace instead of
    // the full command deadline, then terminate the group: prompt return, no
    // orphans, and no false timeout for a command that already exited.
    let deadline = if child_exited {
        let grace = Instant::now()
            .checked_add(child_exited_io_grace())
            .unwrap_or_else(Instant::now);
        deadline_from(start, timeout).min(grace)
    } else {
        deadline_from(start, timeout)
    };
    let mut timed_out = false;
    let mut io_grace_expired = false;
    let mut stdin_result = match stdin_writer.as_mut() {
        Some(writer) => poll_worker_until(writer, deadline)?,
        None => Some(Ok(())),
    };
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
            if let Some(writer) = stdin_writer.as_mut() {
                stdin_result = poll_worker_until(writer, cleanup_deadline)?;
            }
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
    let stdout = stdout_result
        .ok_or_else(|| timed_out_worker_error("shell stdout reader"))?
        .map_err(RuntimeError::Io)?;
    let stderr = stderr_result
        .ok_or_else(|| timed_out_worker_error("shell stderr reader"))?
        .map_err(RuntimeError::Io)?;
    Ok(ProcessIo {
        stdout,
        stderr,
        timed_out,
        io_grace_expired,
    })
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
    let handle = thread::spawn(move || {
        let _ = sender.send(work());
    });
    IoWorker {
        name,
        receiver,
        handle: Some(handle),
    }
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

fn process_io_shutdown_grace() -> Duration {
    // After terminating the group, give the IO workers room to observe the pipe
    // close and drain. Kept well under the 5s descendant sleeps in the runtime
    // tests so a regression in the group kill fails loudly instead of being
    // absorbed by the grace window.
    Duration::from_secs(2)
}

/// IO wait after the main child has already EXITED: an exited process
/// flushes within milliseconds, so any pipe still open belongs to a
/// background descendant that may never close it. Much tighter than the
/// kill-path shutdown grace.
fn child_exited_io_grace() -> Duration {
    Duration::from_millis(250)
}

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
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg("--")
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(50));
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
    let truncated = bytes_seen > captured.len();
    let text = String::from_utf8_lossy(&captured).into_owned();
    Ok(CapturedStream {
        text,
        capture: StreamCapture {
            bytes_seen,
            captured_bytes: captured.len(),
            truncated,
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
        let root = self
            .dir
            .clone()
            .unwrap_or_else(|| std::env::temp_dir().join("tokenzero-spills"));
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let pid = std::process::id();
        Ok(root.join(format!("tokenzero-{pid}-{stamp}-{}.log", self.stream_name)))
    }

    fn path_string(&self) -> Option<String> {
        self.path.as_ref().map(|path| path.display().to_string())
    }

    fn bytes_written(&self) -> usize {
        self.bytes_written
    }
}

/// Age after which a spill file is reclaimable: spills back `spill_path`
/// pointers inside a session; a day later the session that knew the path is
/// gone.
pub const DEFAULT_SPILL_TTL: Duration = Duration::from_secs(24 * 60 * 60);
/// Ceiling for the total bytes a spill directory may hold after an age pass;
/// oldest spills are reclaimed first until the directory fits.
pub const DEFAULT_SPILL_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

/// Outcome of a spill-directory prune. With `dry_run`, `removed_*` counts
/// what would be reclaimed without unlinking anything.
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

/// Reclaim spill files written by `SpillWriter`: everything older than
/// `max_age`, then oldest-first until the directory holds at most
/// `max_total_bytes`. Only `tokenzero-*.log` files are considered, per-file
/// failures are counted rather than fatal, and a missing directory is an
/// empty report — callers may invoke this fail-open on every startup.
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
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("tokenzero-") || !name.ends_with(".log") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        report.scanned_files += 1;
        let modified = meta.modified().unwrap_or(now);
        let expired = now
            .duration_since(modified)
            .map(|age| age > max_age)
            .unwrap_or(false);
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
        let _ = cwd;
        let _ = env_overrides;
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
    if has_windows_path_separator(program) || raw.is_absolute() {
        return windows_program_candidates(raw, env_overrides)
            .into_iter()
            .find(|candidate| candidate.exists())
            .unwrap_or_else(|| raw.to_path_buf());
    }

    for dir in windows_search_dirs(cwd, env_overrides) {
        let base = dir.join(program);
        if let Some(found) = windows_program_candidates(&base, env_overrides)
            .into_iter()
            .find(|candidate| candidate.exists())
        {
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
        let mut candidates = windows_pathexts(env_overrides)
            .into_iter()
            .map(|ext| {
                let mut with_ext = path.as_os_str().to_os_string();
                with_ext.push(ext);
                PathBuf::from(with_ext)
            })
            .collect::<Vec<_>>();
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
        env.iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.clone())
    })
}

#[cfg(windows)]
fn has_windows_path_separator(program: &str) -> bool {
    program.contains('\\') || program.contains('/')
}

#[cfg(windows)]
fn is_windows_batch_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))
}

pub fn split_command_string(command: &str) -> Vec<String> {
    split_command_string_for_platform(command, current_platform())
}

pub fn split_command_string_for_platform(command: &str, platform: &str) -> Vec<String> {
    let preserve_backslashes = matches!(platform, "windows" | "cmd" | "powershell" | "pwsh");
    let single_quote_groups = single_quote_groups_for_platform(command, platform);
    let doubled_quote_escape = doubled_quote_escape_for_platform(command, platform);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut token_started = false;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            escaped = false;
            token_started = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') && !preserve_backslashes {
            // POSIX: inside double quotes a backslash is literal unless it
            // precedes $, `, ", or \ — so "a\|b" must stay a\|b (BRE
            // alternation), not collapse to a|b.
            if quote == Some('"') && !matches!(chars.peek().copied(), Some('$' | '`' | '"' | '\\'))
            {
                current.push('\\');
                token_started = true;
                continue;
            }
            escaped = true;
            token_started = true;
            continue;
        }
        if Some(ch) == quote
            && doubled_quote_escape == Some(ch)
            && chars.peek().copied() == Some(ch)
        {
            current.push(ch);
            let _ = chars.next();
            token_started = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            token_started = true;
            continue;
        }
        if quote.is_none() && (ch == '"' || ch == '\'' && single_quote_groups) {
            quote = Some(ch);
            token_started = true;
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            if token_started {
                out.push(std::mem::take(&mut current));
                token_started = false;
            }
            continue;
        }
        current.push(ch);
        token_started = true;
    }
    if escaped {
        current.push('\\');
    }
    if token_started {
        out.push(current);
    }
    out
}

fn doubled_quote_escape_for_platform(command: &str, platform: &str) -> Option<char> {
    match platform {
        "cmd" => Some('"'),
        "powershell" | "pwsh" => Some('\''),
        "windows" => {
            if first_windows_cmd_word(command)
                .as_deref()
                .is_some_and(is_powershell_shell_host)
            {
                Some('\'')
            } else {
                Some('"')
            }
        }
        _ => None,
    }
}

pub fn contains_shell_syntax(value: &str) -> bool {
    contains_shell_syntax_with_single_quotes(value, true)
}

fn contains_shell_syntax_with_single_quotes(value: &str, single_quote_groups: bool) -> bool {
    if starts_with_posix_env_assignment(value) {
        return true;
    }
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut at_word_start = true;
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            at_word_start = false;
            index += 1;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            at_word_start = false;
            index += 1;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            at_word_start = false;
            index += 1;
            continue;
        }
        if quote.is_none() && (ch == '"' || ch == '\'' && single_quote_groups) {
            quote = Some(ch);
            at_word_start = false;
            index += 1;
            continue;
        }
        let next = chars.get(index + 1).copied();
        // Parameter expansion happens in unquoted and double-quoted contexts:
        // routing $VAR/${...}/$( through a real shell preserves expansion
        // instead of direct-exec'ing the literal bytes.
        if quote != Some('\'')
            && ch == '$'
            && next.is_some_and(|next| {
                next == '(' || next == '{' || next == '_' || next.is_ascii_alphabetic()
            })
        {
            return true;
        }
        if quote.is_none() {
            if matches!(ch, '|' | ';' | '>' | '<' | '`' | '\n') || ch == '&' && next == Some('&') {
                return true;
            }
            // Tilde expansion only applies to an unquoted word-leading ~.
            if ch == '~'
                && at_word_start
                && next.is_none_or(|next| {
                    next == '/' || next.is_whitespace() || next.is_ascii_alphanumeric()
                })
            {
                return true;
            }
            at_word_start = ch.is_whitespace();
        } else {
            at_word_start = false;
        }
        index += 1;
    }
    false
}

fn single_quote_groups_for_platform(value: &str, platform: &str) -> bool {
    match platform {
        "cmd" => false,
        "windows" => first_windows_cmd_word(value)
            .as_deref()
            .is_some_and(is_powershell_shell_host),
        "powershell" | "pwsh" => true,
        _ => true,
    }
}

fn first_windows_cmd_word(value: &str) -> Option<String> {
    let mut quote = false;
    let mut word = String::new();
    for ch in value.chars() {
        if ch == '"' {
            quote = !quote;
            continue;
        }
        if !quote && ch.is_whitespace() {
            break;
        }
        word.push(ch);
    }
    if word.is_empty() { None } else { Some(word) }
}

fn starts_with_posix_env_assignment(value: &str) -> bool {
    let words = split_command_string(value);
    words.len() > 1
        && words
            .first()
            .is_some_and(|word| is_posix_env_assignment(word))
}

fn is_posix_env_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

pub fn contains_platform_shell_syntax(value: &str, platform: &str) -> bool {
    contains_shell_syntax_with_single_quotes(
        value,
        single_quote_groups_for_platform(value, platform),
    ) || matches!(platform, "windows" | "powershell" | "pwsh")
        && looks_like_powershell_syntax(value)
}

pub fn looks_like_powershell_syntax(value: &str) -> bool {
    if first_unquoted_word(value).is_some_and(|word| is_powershell_command_word(&word)) {
        return true;
    }

    let mut quote: Option<char> = None;
    let mut escaped = false;
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if escaped {
            escaped = false;
            index += 1;
            continue;
        }
        if ch == '`' && quote != Some('\'') {
            escaped = true;
            index += 1;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            index += 1;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            index += 1;
            continue;
        }
        if quote != Some('\'') {
            let next = chars.get(index + 1).copied();
            if ch == '$' && next.is_some_and(is_powershell_variable_start) {
                return true;
            }
            if ch == '[' && chars[index + 1..].windows(3).any(|w| w == [']', ':', ':']) {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn first_unquoted_word(value: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut word = String::new();
    for ch in value.chars() {
        if escaped {
            if quote.is_none() || quote == Some('"') {
                word.push(ch);
            }
            escaped = false;
            continue;
        }
        if ch == '`' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if Some(ch) == quote {
            quote = None;
            continue;
        }
        if quote.is_none() && (ch == '\'' || ch == '"') {
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_whitespace() {
            break;
        }
        word.push(ch);
    }
    if word.is_empty() { None } else { Some(word) }
}

fn is_powershell_variable_start(ch: char) -> bool {
    ch == '{' || ch == '_' || ch == '?' || ch.is_ascii_alphabetic()
}

fn is_windows_shell_host(value: &str) -> bool {
    is_powershell_shell_host(value) || windows_shell_host_stem(value) == "cmd"
}

fn is_powershell_shell_host(value: &str) -> bool {
    matches!(
        windows_shell_host_stem(value).as_str(),
        "powershell" | "pwsh"
    )
}

fn windows_shell_host_stem(value: &str) -> String {
    let leaf = value.rsplit(['\\', '/']).next().unwrap_or(value);
    Path::new(leaf)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(leaf)
        .to_ascii_lowercase()
}

fn is_powershell_command_word(word: &str) -> bool {
    let Some((verb, noun)) = word.split_once('-') else {
        return matches!(
            word.to_ascii_lowercase().as_str(),
            "foreach"
                | "where"
                | "if"
                | "else"
                | "elseif"
                | "for"
                | "while"
                | "try"
                | "catch"
                | "finally"
                | "param"
                | "function"
        );
    };
    !noun.is_empty()
        && matches!(
            verb.to_ascii_lowercase().as_str(),
            "add"
                | "clear"
                | "convertfrom"
                | "convertto"
                | "copy"
                | "export"
                | "foreach"
                | "format"
                | "get"
                | "import"
                | "invoke"
                | "join"
                | "move"
                | "new"
                | "out"
                | "pop"
                | "push"
                | "remove"
                | "resolve"
                | "select"
                | "set"
                | "sort"
                | "split"
                | "start"
                | "stop"
                | "tee"
                | "test"
                | "where"
                | "write"
        )
}

fn argv_has_shell_operator_tokens(argv: &[String]) -> bool {
    argv.iter().any(|arg| is_shell_operator_token(arg))
}

fn is_shell_operator_token(arg: &str) -> bool {
    matches!(
        arg,
        "|" | "||" | "&&" | ";" | ">" | ">>" | "<" | "<<" | "2>" | "2>>" | "&>"
    )
}

pub fn is_windows_shell_builtin(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "assoc"
            | "break"
            | "call"
            | "cd"
            | "chdir"
            | "cls"
            | "color"
            | "copy"
            | "date"
            | "del"
            | "dir"
            | "echo"
            | "erase"
            | "exit"
            | "for"
            | "ftype"
            | "if"
            | "md"
            | "mkdir"
            | "mklink"
            | "move"
            | "path"
            | "pause"
            | "popd"
            | "prompt"
            | "pushd"
            | "rd"
            | "rem"
            | "ren"
            | "rename"
            | "rmdir"
            | "set"
            | "shift"
            | "start"
            | "time"
            | "title"
            | "type"
            | "ver"
            | "verify"
            | "vol"
    )
}

pub fn quote_posix(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:@%+=".contains(c))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

pub fn quote_windows_cmd(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./:\\@+=".contains(c))
    {
        value.to_string()
    } else {
        let mut quoted = String::with_capacity(value.len() + 2);
        quoted.push('"');
        for ch in value.chars() {
            match ch {
                '"' => quoted.push_str("\\\""),
                '%' => quoted.push_str("%%"),
                '^' => quoted.push_str("^^"),
                _ => quoted.push(ch),
            }
        }
        quoted.push('"');
        quoted
    }
}

pub fn quote_powershell(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    format!("'{}'", value.replace('\'', "''"))
}

pub fn quote_for(platform: &str, args: &[String]) -> String {
    args.iter()
        .map(|arg| match platform {
            "windows" | "cmd" => quote_windows_cmd(arg),
            "powershell" | "pwsh" => quote_powershell(arg),
            _ => quote_posix(arg),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn executable_path_from_current() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

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

pub fn os_string_vec(values: &[String]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests;
