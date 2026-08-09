use super::*;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar};
use std::thread;
use std::time::Instant;
use tokenzero_runtime::{run_command_with_policy_observer, run_command_with_policy_observers};

#[cfg(test)]
mod rewrite_execution_tests {
    use super::*;

    #[test]
    fn applied_cat_rewrite_builds_the_executed_argv() {
        let command = "cat src/lib.rs";
        let rewrite = rewrite_for_shell(command, "on", false, false);

        assert!(rewrite.applied);
        assert_eq!(rewrite.rewritten_command, "tokenzero read src/lib.rs");
        assert_eq!(
            shell_execution_argv(command, None, &rewrite),
            ["tokenzero", "read", "src/lib.rs"]
        );
    }

    #[test]
    fn explicit_argv_is_authoritative_and_rewrite_is_truthfully_skipped() {
        let command = "cat display-only.txt";
        let rewrite = rewrite_for_shell(command, "on", false, true);
        let explicit = vec![
            "printf".to_string(),
            "%s".to_string(),
            "literal".to_string(),
        ];

        assert!(!rewrite.applied);
        assert_eq!(
            rewrite.reason,
            "explicit argv is authoritative; command rewrite skipped"
        );
        assert_eq!(
            shell_execution_argv(command, Some(explicit.clone()), &rewrite),
            explicit
        );
    }

    #[test]
    fn explicit_argv_skip_retains_an_unsafe_command_reason() {
        let rewrite = rewrite_for_shell("rm -rf target", "on", false, true);

        assert!(!rewrite.applied);
        assert!(!rewrite.safe);
        assert!(rewrite.reason.contains("unsafe destructive mutation"));
    }
}

#[derive(Debug)]
struct BackgroundJobState {
    status: &'static str,
    pid: Option<u32>,
    pgid: Option<u32>,
    exit_code: Option<i32>,
    version: u64,
    completed_at: Option<Instant>,
    terminate_requested: bool,
    log_error: Option<String>,
}

const MAX_BACKGROUND_JOBS: usize = 256;
const MAX_JOB_TAIL_BYTES: usize = 64 * 1024;
const DEFAULT_JOB_TAIL_BYTES: usize = 8 * 1024;
const COMPLETED_JOB_TTL: Duration = Duration::from_secs(15 * 60);
const UNCHANGED_NEXT_POLL_MS: u64 = 20_000;

fn decode_job_tail(bytes: &[u8]) -> (String, bool, usize) {
    let consumed = if std::str::from_utf8(bytes).is_ok() {
        bytes.len()
    } else {
        // Lossy UTF-8 can expand one invalid input byte to the three-byte
        // replacement character. Cap the raw prefix so the shared typed tail
        // remains within its 64 KiB serialized bound and leaves a cursor for
        // the next poll instead of stranding binary output.
        bytes.len().min(MAX_JOB_TAIL_BYTES / 3)
    };
    let selected = &bytes[..consumed];
    (
        String::from_utf8_lossy(selected).into_owned(),
        std::str::from_utf8(selected).is_ok(),
        consumed,
    )
}

fn shell_argv(command: &str) -> Vec<String> {
    if contains_platform_shell_syntax(command, tokenzero_runtime::current_platform()) {
        vec![command.to_string()]
    } else {
        split_command_string(command)
    }
}

fn rewrite_for_shell(
    command: &str,
    rewrite_mode: &str,
    no_rewrite: bool,
    explicit_argv: bool,
) -> tokenzero_filters::RewriteResult {
    let rewrite_requested = !no_rewrite && rewrite_mode != "off";
    let mut result = rewrite_command(command, rewrite_mode, rewrite_requested);
    if explicit_argv && rewrite_requested {
        let safety_reason = (!result.safe).then(|| result.reason.clone());
        result.applied = false;
        result.rewritten_command = command.to_string();
        result.reason = match safety_reason {
            Some(reason) => {
                format!("explicit argv is authoritative; command rewrite skipped; {reason}")
            }
            None => "explicit argv is authoritative; command rewrite skipped".to_string(),
        };
    }
    result
}

fn shell_execution_argv(
    command: &str,
    argv: Option<Vec<String>>,
    rewrite: &tokenzero_filters::RewriteResult,
) -> Vec<String> {
    match argv {
        Some(argv) => argv,
        None => shell_argv(if rewrite.applied {
            &rewrite.rewritten_command
        } else {
            command
        }),
    }
}

#[cfg(test)]
#[derive(Debug)]
struct PollInterleave {
    length_observed: std::sync::Barrier,
    publication_done: std::sync::Barrier,
}

#[derive(Debug)]
struct BackgroundJob {
    id: String,
    sequence: u64,
    log: PathBuf,
    log_file: Arc<Mutex<fs::File>>,
    state: Mutex<BackgroundJobState>,
    changed: Condvar,
    #[cfg(test)]
    poll_interleave: Option<Arc<PollInterleave>>,
}

fn background_job_is_complete(job: &BackgroundJob) -> bool {
    matches!(lock(&job.state).status, "exited" | "failed")
}

fn background_job_is_expired(job: &BackgroundJob, now: Instant) -> bool {
    lock(&job.state)
        .completed_at
        .is_some_and(|completed| now.saturating_duration_since(completed) >= COMPLETED_JOB_TTL)
}

#[derive(Debug, Default)]
pub(crate) struct BackgroundJobRegistry {
    next_id: AtomicU64,
    terminating: AtomicBool,
    jobs: Mutex<BTreeMap<String, Arc<BackgroundJob>>>,
}

static BACKGROUND_JOBS: OnceLock<BackgroundJobRegistry> = OnceLock::new();

fn background_jobs() -> &'static BackgroundJobRegistry {
    BACKGROUND_JOBS.get_or_init(BackgroundJobRegistry::default)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn job_log_len(job: &BackgroundJob) -> Result<usize, String> {
    let file = lock(&job.log_file);
    let metadata = file
        .metadata()
        .map_err(|error| format!("read background log metadata: {error}"))?;
    if !metadata.is_file() {
        return Err("background log handle is not a regular file".to_string());
    }
    usize::try_from(metadata.len())
        .map_err(|_| "background log length exceeds this platform".to_string())
}

fn read_job_window(
    job: &BackgroundJob,
    since: usize,
    tail_bytes: usize,
) -> Result<(Vec<u8>, usize, usize), String> {
    let mut file = lock(&job.log_file);
    let before = file
        .metadata()
        .map_err(|error| format!("read background log metadata: {error}"))?;
    if !before.is_file() {
        return Err("background log handle is not a regular file".to_string());
    }
    let log_bytes = usize::try_from(before.len())
        .map_err(|_| "background log length exceeds this platform".to_string())?;
    let start = since.min(log_bytes);
    let requested = tail_bytes.clamp(1, MAX_JOB_TAIL_BYTES);
    let read_limit = requested.min(log_bytes.saturating_sub(start));
    file.seek(SeekFrom::Start(start as u64))
        .map_err(|error| format!("seek background log: {error}"))?;
    let mut bytes = Vec::with_capacity(read_limit);
    (&mut *file)
        .take(read_limit as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read background log: {error}"))?;
    if bytes.len() != read_limit {
        return Err(format!(
            "background log returned a short read: expected {read_limit} bytes, got {}",
            bytes.len()
        ));
    }
    let after = file
        .metadata()
        .map_err(|error| format!("re-read background log metadata: {error}"))?;
    if after.len() != before.len() || !after.is_file() {
        return Err("background log changed during its bounded poll read".to_string());
    }
    Ok((bytes, start, log_bytes))
}

fn store_shell_payload(
    store: &mut RecoveryStore,
    text: &str,
    digest: &str,
    kind: &str,
    content_type: ContentType,
) -> StoredPayload {
    let source = PathBuf::from(format!("shell:{kind}:{digest}"));
    store.store_payload_deferred(text, content_type, Some(&source), None, None)
}

fn store_shell_outputs(
    store: &mut RecoveryStore,
    stdout: &str,
    stderr: &str,
    combined: &str,
    digest: &str,
) -> (StoredPayload, StoredPayload, StoredPayload) {
    (
        store_shell_payload(store, stdout, digest, "stdout", ContentType::ShellOutput),
        store_shell_payload(store, stderr, digest, "stderr", ContentType::ShellOutput),
        store_shell_payload(
            store,
            combined,
            digest,
            "canonical-combined-witness",
            ContentType::ShellOutput,
        ),
    )
}

fn verify_full_stream_digest(
    text: &str,
    capture: &StreamCapture,
    stream: &str,
) -> Result<(), String> {
    let expected = capture
        .full_stream_sha256
        .as_deref()
        .ok_or_else(|| format!("{stream} exact witness omitted its observer-time digest"))?;
    let actual = sha256_hex(text);
    if actual != expected {
        return Err(format!(
            "{stream} exact witness digest changed: expected {expected}, got {actual}"
        ));
    }
    Ok(())
}

fn exact_shell_stream_text(
    display: &str,
    capture: &StreamCapture,
    stream: &str,
    max_bytes: usize,
) -> Result<String, String> {
    if capture.bytes_seen > max_bytes {
        return Err(format!(
            "{stream} exact witness is {} bytes; configured recovery read limit is {max_bytes}",
            capture.bytes_seen
        ));
    }
    if capture.truncated {
        let path = capture
            .spill_path
            .as_deref()
            .ok_or_else(|| format!("{stream} capture was truncated without a spill file"))?;
        if capture.spill_bytes != capture.bytes_seen {
            return Err(format!(
                "{stream} spill covers {} of {} observed bytes",
                capture.spill_bytes, capture.bytes_seen
            ));
        }
        let file = fs::File::open(path).map_err(|error| format!("open {stream} spill: {error}"))?;
        let mut bytes = Vec::with_capacity(capture.bytes_seen);
        file.take(
            u64::try_from(max_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read {stream} spill: {error}"))?;
        if bytes.len() != capture.bytes_seen {
            return Err(format!(
                "{stream} spill length changed: expected {}, got {}",
                capture.bytes_seen,
                bytes.len()
            ));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{stream} exact witness is not valid UTF-8"))?;
        verify_full_stream_digest(&text, capture, stream)?;
        return Ok(text);
    }
    if !capture.captured_utf8_lossless {
        return Err(format!("{stream} exact witness is not valid UTF-8"));
    }
    if capture.captured_bytes != capture.bytes_seen || display.len() != capture.bytes_seen {
        return Err(format!(
            "{stream} in-memory capture covers {} display bytes of {} observed bytes",
            display.len(),
            capture.bytes_seen
        ));
    }
    verify_full_stream_digest(display, capture, stream)?;
    Ok(display.to_string())
}

#[cfg(test)]
mod exact_stream_witness_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn same_length_spill_mutation_degrades_without_refs() {
        let dir = tempdir().unwrap();
        let spill = dir.path().join("stdout.log");
        let observed = "original";
        fs::write(&spill, observed).unwrap();
        let capture = StreamCapture {
            bytes_seen: observed.len(),
            captured_bytes: 3,
            truncated: true,
            captured_utf8_lossless: true,
            full_stream_sha256: Some(sha256_hex(observed)),
            spill_path: Some(spill.display().to_string()),
            spill_bytes: observed.len(),
        };
        fs::write(&spill, "mutation").unwrap();

        let error = exact_shell_stream_text("ori", &capture, "stdout", 1024).unwrap_err();
        assert!(error.contains("digest changed"), "{error}");
        let response = degraded_shell_response("probe", Mode::Auto, "preview", error);
        assert!(response.refs.is_empty());
        assert_eq!(
            response.telemetry.as_ref().unwrap()["transport_status"],
            "degraded"
        );
        assert!(response.safety.is_none());
    }

    #[test]
    fn absent_observer_digest_never_authorizes_exact_recovery() {
        let capture = StreamCapture {
            bytes_seen: 3,
            captured_bytes: 3,
            truncated: false,
            captured_utf8_lossless: true,
            full_stream_sha256: None,
            spill_path: None,
            spill_bytes: 0,
        };
        let error = exact_shell_stream_text("abc", &capture, "stdout", 1024).unwrap_err();
        assert!(
            error.contains("omitted its observer-time digest"),
            "{error}"
        );
    }
}

fn shell_ref(kind: &str, stored: &StoredPayload, bytes: usize) -> tokenzero_core::RefRecord {
    ref_record(kind, stored.blob_ref.clone(), bytes)
}

fn verify_persisted_shell_witnesses(
    cache_path: &Path,
    witnesses: &[(&str, &StoredPayload, &str)],
) -> Result<(), String> {
    let mut verifier = RecoveryStore::new(Some(cache_path.to_path_buf()));
    for (kind, stored, expected) in witnesses {
        if !verifier.has_ref_local(&stored.blob_ref) {
            return Err(format!(
                "persisted {kind} witness ref is not durable: {}",
                stored.blob_ref
            ));
        }
        let expanded = verifier.expand(&stored.blob_ref, Some("raw"), None, None, None, None);
        if !expanded.found {
            return Err(format!(
                "persisted {kind} witness cannot expand: {}",
                expanded.reason
            ));
        }
        if expanded.content.as_bytes() != expected.as_bytes() {
            return Err(format!(
                "persisted {kind} witness content does not match its exact stream"
            ));
        }
    }
    Ok(())
}

macro_rules! shell_stream_capture {
    ($display:expr, $full:expr, $capture:expr, $stored:expr) => {
        json!({
            "bytes": $full.len(),
            "bytes_seen": $capture.bytes_seen,
            "captured_bytes": $capture.captured_bytes,
            "preview_bytes": $display.len(),
            "truncated": $capture.truncated,
            "preview_truncated": $capture.truncated,
            "captured_utf8_lossless": $capture.captured_utf8_lossless,
            "full_stream_sha256": $capture.full_stream_sha256,
            "spill_path": $capture.spill_path,
            "spill_bytes": $capture.spill_bytes,
            "sha256": $stored
                .blob_ref
                .strip_prefix("tz://blob/")
                .unwrap_or(&$stored.blob_ref),
            "sha256_scope": "full_stream",
            "ref_covers_full_stream": true,
            "ref": $stored.blob_ref
        })
    };
}

impl BackgroundJobRegistry {
    fn insert_bounded(&self, job: Arc<BackgroundJob>) -> Result<(), String> {
        let mut jobs = lock(&self.jobs);
        let now = Instant::now();
        jobs.retain(|_, existing| !background_job_is_expired(existing, now));
        while jobs.len() >= MAX_BACKGROUND_JOBS {
            let oldest_completed = jobs
                .iter()
                .filter(|(_, existing)| background_job_is_complete(existing))
                .min_by_key(|(_, existing)| existing.sequence)
                .map(|(id, _)| id.clone());
            let Some(oldest_completed) = oldest_completed else {
                return Err(format!(
                    "background job registry is full ({MAX_BACKGROUND_JOBS} running jobs)"
                ));
            };
            jobs.remove(&oldest_completed);
        }
        jobs.insert(job.id.clone(), job);
        Ok(())
    }

    fn start(
        &self,
        argv: Vec<String>,
        cwd: Option<PathBuf>,
        env: BTreeMap<String, String>,
        timeout: Duration,
        log_dir: PathBuf,
    ) -> Result<Value, String> {
        if self.terminating.load(Ordering::SeqCst) {
            return Err("background jobs are unavailable during session teardown".to_string());
        }
        fs::create_dir_all(&log_dir).map_err(|err| format!("create background log dir: {err}"))?;
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = format!("tzjob-{}-{sequence}", std::process::id());
        let log = log_dir.join(format!("{id}.log"));
        let live_log = OpenOptions::new()
            .read(true)
            .append(true)
            .create_new(true)
            .open(&log)
            .map_err(|err| format!("create background log: {err}"))?;
        let live_log = Arc::new(Mutex::new(live_log));
        let job = Arc::new(BackgroundJob {
            id: id.clone(),
            sequence,
            log: log.clone(),
            log_file: Arc::clone(&live_log),
            state: Mutex::new(BackgroundJobState {
                status: "running",
                pid: None,
                pgid: None,
                exit_code: None,
                version: 0,
                completed_at: None,
                terminate_requested: false,
                log_error: None,
            }),
            changed: Condvar::new(),
            #[cfg(test)]
            poll_interleave: None,
        });
        if let Err(error) = self.insert_bounded(Arc::clone(&job)) {
            drop(job);
            drop(live_log);
            let _ = fs::remove_file(&log);
            return Err(error);
        }
        // Close the check/insert race with raw session teardown. Once the
        // registry enters terminating state, no detached child may be spawned.
        if self.terminating.load(Ordering::SeqCst) {
            lock(&self.jobs).remove(&id);
            drop(job);
            drop(live_log);
            let _ = fs::remove_file(&log);
            return Err("background jobs are unavailable during session teardown".to_string());
        }
        crate::shell_hooks::reserve_background_job(&id);
        let worker_id = id.clone();
        let spawn = thread::Builder::new()
            .name(format!("tokenzero-{id}"))
            .spawn(move || {
                let observed = Arc::clone(&job);
                let stream_job = Arc::clone(&job);
                let stream_log = Arc::clone(&live_log);
                let completion_log = Arc::clone(&live_log);
                let result = run_command_with_policy_observers(
                    &argv,
                    cwd.as_deref(),
                    Some(&env),
                    None,
                    timeout,
                    false,
                    RunOutputPolicy {
                        spill_dir: Some(log_dir),
                        ..RunOutputPolicy::default()
                    },
                    move |pid, pgid, state| {
                        if state == "running" {
                            let mut current = lock(&observed.state);
                            current.pid = pid;
                            current.pgid = pgid;
                            let terminate_requested = current.terminate_requested;
                            drop(current);
                            crate::shell_hooks::note_background_child(&observed.id, pid, pgid);
                            if terminate_requested {
                                if let Some(group) = pgid {
                                    terminate_background_groups(&[group]);
                                }
                            }
                        }
                    },
                    move |_, chunk| {
                        let write_error = {
                            let mut log = lock(&stream_log);
                            log.write_all(chunk).and_then(|()| log.flush()).err()
                        };
                        let mut current = lock(&stream_job.state);
                        let failed_group = write_error.as_ref().and_then(|error| {
                            current
                                .log_error
                                .get_or_insert_with(|| format!("write background log: {error}"));
                            current.pgid
                        });
                        current.version = current.version.saturating_add(1);
                        drop(current);
                        if let Some(group) = failed_group {
                            terminate_background_groups(&[group]);
                        }
                        stream_job.changed.notify_all();
                    },
                );
                let (failure_text, exit_code, status) = match result {
                    Ok(result) => (None, result.exit_code, "exited"),
                    Err(err) => (
                        Some(format!("background shell failed: {err}")),
                        None,
                        "failed",
                    ),
                };
                let completion_log_error = failure_text.and_then(|text| {
                    let mut log = lock(&completion_log);
                    writeln!(log, "{text}").and_then(|()| log.flush()).err()
                });
                let mut current = lock(&job.state);
                if let Some(error) = completion_log_error {
                    current
                        .log_error
                        .get_or_insert_with(|| format!("write background failure log: {error}"));
                }
                current.status = status;
                current.exit_code = exit_code;
                current.completed_at = Some(Instant::now());
                current.version = current.version.saturating_add(1);
                drop(current);
                job.changed.notify_all();
                crate::shell_hooks::finish_background_job(&worker_id);
            });
        if let Err(err) = spawn {
            lock(&self.jobs).remove(&id);
            crate::shell_hooks::finish_background_job(&id);
            return Err(format!("spawn background worker: {err}"));
        }
        Ok(json!({"job": id, "log": log.display().to_string(), "cursor": 0, "version": 0}))
    }
    fn poll(
        &self,
        id: &str,
        wait: Duration,
        since: usize,
        tail_bytes: usize,
    ) -> Result<Value, String> {
        let job = {
            let mut jobs = lock(&self.jobs);
            let now = Instant::now();
            jobs.retain(|_, existing| !background_job_is_expired(existing, now));
            jobs.get(id)
                .cloned()
                .ok_or_else(|| format!("unknown background job: {id}"))?
        };
        // Snapshot state before the log length. If a writer publishes between
        // the length read and the second state lock, its version change makes
        // the bytes immediately observable instead of losing the wake-up.
        let snapshot_version = lock(&job.state).version;
        let available_before_wait = job_log_len(&job)?;
        #[cfg(test)]
        if let Some(interleave) = &job.poll_interleave {
            interleave.length_observed.wait();
            interleave.publication_done.wait();
        }
        let mut state = lock(&job.state);
        let observed_version = state.version;
        if state.status == "running"
            && state.version == snapshot_version
            && !wait.is_zero()
            && since >= available_before_wait
        {
            let deadline = Instant::now() + wait.min(Duration::from_secs(30));
            while state.status == "running" && state.version == observed_version {
                let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                    break;
                };
                let observed = job
                    .changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(|poison| poison.into_inner());
                state = observed.0;
                if observed.1.timed_out() {
                    break;
                }
            }
        }
        let status = state.status;
        let pid = state.pid;
        let exit_code = state.exit_code;
        let version = state.version;
        let log_error = state.log_error.clone();
        drop(state);
        if let Some(error) = log_error {
            return Err(error);
        }

        let (window, start, log_bytes) = read_job_window(&job, since, tail_bytes)?;
        let changed = !window.is_empty() || status != "running";
        if !changed {
            return Ok(json!({
                "status": status,
                "pid": pid,
                "unchanged": true,
                "cursor": start,
                "version": version,
                "nextPollMs": UNCHANGED_NEXT_POLL_MS,
            }));
        }

        let (tail, tail_utf8_lossless, consumed) = decode_job_tail(&window);
        let end = start.saturating_add(consumed);
        let mut response = json!({
            "status": status,
            "pid": pid,
            "exitCode": exit_code,
            "tail": tail,
            "tailUtf8Lossless": tail_utf8_lossless,
            "tailBytes": end.saturating_sub(start),
            "log": job.log.display().to_string(),
            "logBytes": log_bytes,
            "cursor": end,
            "version": version,
            "changed": true,
            "unchanged": false,
        });
        if status == "running" {
            response["nextPollMs"] = json!(if end < log_bytes {
                0
            } else {
                UNCHANGED_NEXT_POLL_MS
            });
        }
        Ok(response)
    }

    fn terminate_all(&self) {
        self.terminating.store(true, Ordering::SeqCst);
        let groups = {
            let jobs = lock(&self.jobs);
            jobs.values()
                .filter_map(|job| {
                    let mut state = lock(&job.state);
                    if state.status != "running" {
                        return None;
                    }
                    state.terminate_requested = true;
                    state.pgid
                })
                .collect::<Vec<_>>()
        };
        terminate_background_groups(&groups);
    }
}

/// Explicit raw-worker teardown hook. Static registries do not run `Drop` at
/// process exit, so session shutdown must invoke this before leaving serve.
pub(crate) fn terminate_all_background_jobs() {
    if let Some(registry) = BACKGROUND_JOBS.get() {
        registry.terminate_all();
    }
}

#[cfg(test)]
pub(crate) fn reset_background_job_termination_for_tests() {
    if let Some(registry) = BACKGROUND_JOBS.get() {
        registry.terminating.store(false, Ordering::SeqCst);
    }
}

impl Drop for BackgroundJobRegistry {
    fn drop(&mut self) {
        self.terminate_all();
    }
}

#[cfg(unix)]
fn terminate_background_groups(groups: &[u32]) {
    let targets = groups
        .iter()
        .copied()
        .filter(|group| *group != 0)
        .map(|group| format!("-{group}"))
        .collect::<Vec<_>>();
    for target in &targets {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", "--", target])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    if !targets.is_empty() {
        thread::sleep(Duration::from_millis(50));
    }
    for target in &targets {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", target])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

#[cfg(not(unix))]
fn terminate_background_groups(_: &[u32]) {}

impl TokenZeroEngine {
    /// Resolve shell cwd: explicit wins; otherwise default to `call_root` (plan/server
    /// root), never silent process-cwd inheritance without an echoed path.
    fn resolve_shell_cwd(&self, cwd: Option<&Path>) -> Result<(PathBuf, &'static str), String> {
        match cwd {
            Some(path) => {
                if !self.path_allowed(path) {
                    return Err(format!("cwd is outside allowed roots: {}", path.display()));
                }
                Ok((path.to_path_buf(), "explicit"))
            }
            None => {
                let root = self.config.call_root.clone();
                if !self.path_allowed(&root) {
                    return Err(format!(
                        "call_root is outside allowed roots: {}",
                        root.display()
                    ));
                }
                Ok((root, "call_root"))
            }
        }
    }

    pub fn shell_background(
        &self,
        command: &str,
        cwd: Option<&Path>,
        timeout_override: Option<Duration>,
    ) -> Result<Value, String> {
        let (resolved_cwd, cwd_source) = self.resolve_shell_cwd(cwd)?;
        let run_argv = shell_argv(command);
        let child_env = inner_env();
        let mut launched = background_jobs().start(
            run_argv,
            Some(resolved_cwd.clone()),
            child_env,
            timeout_override.unwrap_or(self.config.shell_timeout),
            shell_spill_dir(&self.config.cache_path),
        )?;
        if let Some(obj) = launched.as_object_mut() {
            obj.insert("cwd".to_string(), json!(resolved_cwd.display().to_string()));
            obj.insert("cwd_source".to_string(), json!(cwd_source));
            obj.insert("rewrite_applied".to_string(), json!(false));
            obj.insert(
                "rewrite_skip_reason".to_string(),
                json!("background shell API does not accept a rewrite policy"),
            );
        }
        Ok(launched)
    }

    pub fn shell_job(&self, id: &str) -> Result<Value, String> {
        background_jobs().poll(id, Duration::ZERO, 0, DEFAULT_JOB_TAIL_BYTES)
    }

    pub fn shell_job_wait(
        &self,
        id: &str,
        wait: Duration,
        since: usize,
        tail_bytes: usize,
    ) -> Result<Value, String> {
        background_jobs().poll(id, wait, since, tail_bytes)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn shell(
        &self,
        command: &str,
        argv: Option<Vec<String>>,
        cwd: Option<&Path>,
        mode: Mode,
        rewrite: Option<&str>,
        no_rewrite: bool,
        env: Option<BTreeMap<String, String>>,
        stdin: Option<&str>,
        timeout_override: Option<Duration>,
    ) -> ToolResponse {
        let (resolved_cwd, cwd_source) = match self.resolve_shell_cwd(cwd) {
            Ok(resolved) => resolved,
            Err(message) => {
                return ToolResponse::error(
                    "shell",
                    "path_outside_allowed_roots",
                    message,
                    Some("set cwd under an allowed root".to_string()),
                );
            }
        };
        let cwd = resolved_cwd.as_path();
        let rewrite_mode = rewrite.unwrap_or("off");
        let rewrite_result = rewrite_for_shell(command, rewrite_mode, no_rewrite, argv.is_some());
        let run_argv = shell_execution_argv(command, argv, &rewrite_result);
        let env_summary = env
            .as_ref()
            .map(|values| values.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        // Children of any TokenZero-executed command must never re-enter the
        // PATH shim layer (a shim re-wrapping an inner grep corrupts piped
        // results); the shim guard checks this variable. Caller-provided
        // values win. Command::envs is additive, so the inherited
        // environment is unaffected.
        let mut child_env = env.unwrap_or_default();
        child_env
            .entry("TOKENZERO_INNER".to_string())
            .or_insert_with(|| "1".to_string());
        let output_policy = self.shell_output_policy();
        let result = match run_command_with_policy_observer(
            &run_argv,
            Some(cwd),
            Some(&child_env),
            stdin,
            timeout_override.unwrap_or(self.config.shell_timeout),
            false,
            output_policy,
            crate::shell_hooks::note_child,
        ) {
            Ok(result) => result,
            Err(err) => {
                crate::shell_hooks::note_child(None, None, "spawn_failed");
                return ToolResponse::error(
                    "shell",
                    "spawn_failed",
                    err.to_string(),
                    Some("verify command path and cwd".to_string()),
                );
            }
        };
        let stdout_display = captured_stream_text(&result.stdout, &result.stdout_capture, "stdout");
        let stderr_display = captured_stream_text(&result.stderr, &result.stderr_capture, "stderr");
        let streams_truncated = result.stdout_capture.truncated || result.stderr_capture.truncated;
        let display_command = result.command.as_str();
        let render_command = if display_command.chars().count() > 60 {
            format!("{}…", display_command.chars().take(59).collect::<String>())
        } else {
            display_command.to_string()
        };
        let output = shell_combined_output(
            display_command,
            result.exit_code,
            &stdout_display,
            &stderr_display,
        );
        let mut store = self.recovery_store();
        let exact_witness_limit = store.config.max_load_bytes;
        let stdout_full = match exact_shell_stream_text(
            &stdout_display,
            &result.stdout_capture,
            "stdout",
            exact_witness_limit,
        ) {
            Ok(text) => text,
            Err(error) => return degraded_shell_response(command, mode, &output, error),
        };
        let stderr_full = match exact_shell_stream_text(
            &stderr_display,
            &result.stderr_capture,
            "stderr",
            exact_witness_limit,
        ) {
            Ok(text) => text,
            Err(error) => return degraded_shell_response(command, mode, &output, error),
        };
        // This is a deterministic recovery witness built from two exact streams.
        // It is not evidence of the process's temporal stdout/stderr interleaving.
        let full_output = shell_combined_output(
            display_command,
            result.exit_code,
            &stdout_full,
            &stderr_full,
        );
        let command_digest = sha256_hex(display_command);
        let (stdout_stored, stderr_stored, combined_stored) = store_shell_outputs(
            &mut store,
            &stdout_full,
            &stderr_full,
            &full_output,
            &command_digest,
        );
        let render = render_shell(ShellRenderInput {
            command: &render_command,
            stdout: &stdout_display,
            stderr: &stderr_display,
            exit_code: result.exit_code,
            timed_out: result.timed_out,
            mode,
            max_visible_tokens: self.config.max_visible_tokens,
            stdout_ref: (!stdout_display.is_empty()).then_some(stdout_stored.blob_ref.as_str()),
            stderr_ref: (!stderr_display.is_empty()).then_some(stderr_stored.blob_ref.as_str()),
            combined_ref: Some(&combined_stored.blob_ref),
        });
        let effective_cwd = result
            .cwd
            .clone()
            .unwrap_or_else(|| resolved_cwd.display().to_string());
        let capture = json!({
            "schema_version": "tokenzero.capture.v1",
            "command": display_command,
            "argv": result.argv,
            "cwd": effective_cwd,
            "cwd_source": cwd_source,
            "env_summary": env_summary,
            "timing": {"duration_ms": result.duration_ms, "timed_out": result.timed_out},
            "exit_code": result.exit_code,
            "stdout": shell_stream_capture!(&stdout_display, &stdout_full, result.stdout_capture, stdout_stored),
            "stderr": shell_stream_capture!(&stderr_display, &stderr_full, result.stderr_capture, stderr_stored),
            "combined": {
                "bytes": full_output.len(),
                "preview_bytes": output.len(),
                "preview_truncated": streams_truncated,
                "kind": "canonical_stdout_stderr_witness",
                "temporal_interleaving_claimed": false,
                "sha256": combined_stored.blob_ref.strip_prefix("tz://blob/").unwrap_or(&combined_stored.blob_ref),
                "sha256_scope": "canonical_full_stream_witness",
                "ref_covers_full_streams": true,
                "ref": combined_stored.blob_ref
            },
            "allocator_pressure_relief": result.allocator_pressure_relief,
            "parser_metadata": {
                "policy": render.policy.policy.clone(),
                "policy_reason": render.policy.reason.clone(),
                "family": render.policy.family.clone(),
                "output_strategy": render.output_strategy.clone()
            },
            "command_status": render.command_status.clone()
        });
        let capture_text = serde_json::to_string(&capture).unwrap_or_else(|_| "{}".to_string());
        let capture_stored = store_shell_payload(
            &mut store,
            &capture_text,
            &command_digest,
            "capture",
            ContentType::JsonConfig,
        );
        let mut refs = Vec::new();
        if !stdout_full.is_empty() {
            refs.push(shell_ref("stdout", &stdout_stored, stdout_full.len()));
        }
        if !stderr_full.is_empty() {
            refs.push(shell_ref("stderr", &stderr_stored, stderr_full.len()));
        }
        refs.push(shell_ref("combined", &combined_stored, full_output.len()));
        refs.push(shell_ref("capture", &capture_stored, capture_text.len()));
        let persisted = persist_refs(&mut store, &mut refs);
        if let Some(error) = persisted.error {
            return degraded_shell_response(command, mode, &output, error);
        }
        if let Err(error) = verify_persisted_shell_witnesses(
            &self.config.cache_path,
            &[
                ("stdout", &stdout_stored, &stdout_full),
                ("stderr", &stderr_stored, &stderr_full),
                ("combined", &combined_stored, &full_output),
            ],
        ) {
            return degraded_shell_response(command, mode, &output, error);
        }
        let refs_complete = persisted.refs_complete;
        let raw_tokens = shell_raw_tokens(command, result.exit_code, &stdout_full, &stderr_full);
        // Tokenizers can encode long repeated runs as a single token. Keep the
        // token accounting exact, but bound inline transport by bytes as well.
        let fits_default_inline_extent =
            output.len() <= DEFAULT_SHELL_INLINE_BUDGET.saturating_mul(4);
        let fits_configured_inline_extent =
            output.len() <= self.config.shell_inline_budget.saturating_mul(4);
        let inline_shell_output = refs_complete
            && !streams_truncated
            && render.command_status.command_success
            && self.config.shell_inline_budget > 0
            && raw_tokens <= self.config.shell_inline_budget
            && fits_configured_inline_extent;
        let small_shell_output = refs_complete
            && !streams_truncated
            && render.command_status.command_success
            && raw_tokens <= DEFAULT_SHELL_INLINE_BUDGET
            && fits_default_inline_extent;
        let visible_text = if inline_shell_output {
            output.trim_end().to_string()
        } else if refs_complete
            && self.config.shell_inline_budget == 0
            && !streams_truncated
            && raw_tokens <= DEFAULT_SHELL_INLINE_BUDGET
            && fits_default_inline_extent
            && render.command_status.command_success
        {
            format!("# shell ok\ncombined_ref: {}", combined_stored.blob_ref)
        } else if refs_complete {
            render.visible.clone()
        } else {
            output.trim_end().to_string()
        };
        let response_refs = if small_shell_output {
            vec![shell_ref("combined", &combined_stored, full_output.len())]
        } else {
            refs
        };
        // The plan root is already part of the zero_execute request. Surface cwd only
        // when the command deliberately runs somewhere else.
        let visible_text = if resolved_cwd == self.config.call_root
            || visible_text.contains("\ncwd: ")
            || visible_text.starts_with("cwd: ")
        {
            visible_text
        } else if visible_text.starts_with("# shell") {
            let mut lines = visible_text.lines();
            let first = lines.next().unwrap_or("# shell");
            let rest: Vec<&str> = lines.collect();
            if rest.is_empty() {
                format!("{first}\ncwd: {effective_cwd}")
            } else {
                format!("{first}\ncwd: {effective_cwd}\n{}", rest.join("\n"))
            }
        } else {
            format!("cwd: {effective_cwd}\n{visible_text}")
        };
        // Presentation masking never changes bytes persisted above. Explicit
        // exact/passthrough modes remain deliberate escapes; auto and all
        // compact policies mask recognized secrets before agent exposure.
        let mask_visible_output =
            !matches!(mode.effective_policy(), Mode::Exact | Mode::Passthrough);
        let visible_text = if mask_visible_output {
            tokenzero_core::mask_visible_secrets(&visible_text)
        } else {
            visible_text
        };
        // Shell refs leave the process and may be replayed by an upstream
        // execution cache long after session aliases have been pruned. Emit
        // canonical content-addressed refs only: persist_refs above has
        // synchronously made these durable before this response is built.
        let stdout_vis = stdout_stored.blob_ref.clone();
        let stderr_vis = stderr_stored.blob_ref.clone();
        let combined_vis = combined_stored.blob_ref.clone();
        let capture_vis = capture_stored.blob_ref.clone();
        let visible_tokens = if inline_shell_output || refs_complete {
            count_tokens(&visible_text)
        } else {
            raw_tokens
        };
        let output_strategy = if inline_shell_output {
            "inline_shell".to_string()
        } else {
            capture["parser_metadata"]["output_strategy"]
                .as_str()
                .unwrap_or(&render.output_strategy)
                .to_string()
        };
        let mut response = ToolResponse::ok(
            "shell",
            render
                .policy
                .policy
                .parse()
                .unwrap_or(mode.effective_policy()),
            visible_text,
            response_refs,
            Accounting {
                raw_tokens,
                visible_tokens,
                recovery_tokens: store.recovery_tokens,
                billed_tokens: visible_tokens,
                cached_tokens: 0,
                exact_ref_tokens: Some(if small_shell_output {
                    count_tokens(&combined_vis)
                } else {
                    let mut total = count_tokens(&combined_vis) + count_tokens(&capture_vis);
                    if !stdout_full.is_empty() {
                        total += count_tokens(&stdout_vis);
                    }
                    if !stderr_full.is_empty() {
                        total += count_tokens(&stderr_vis);
                    }
                    total
                }),
            },
        );
        response.content_type = Some(ContentType::ShellOutput.to_string());
        if streams_truncated {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "shell_output_preview_truncated".to_string(),
                message: "visible shell previews were truncated; durable stdout/stderr refs retain the complete UTF-8 streams".to_string(),
                repair: Some("expand stdout_ref or stderr_ref for the complete stream".to_string()),
            });
        }
        let mut telemetry = json!({
            "command": display_command,
            "argv": capture["argv"],
            "execution_mode": result.execution_mode,
            "alias_dependency": result.alias_dependency,
            "cwd": capture["cwd"],
            "cwd_source": capture["cwd_source"],
            "transport_status": "ok",
            "preview_truncated": streams_truncated,
            "refs_cover_full_output": true,
            "command_success": capture["command_status"]["command_success"],
            "exit_code": capture["exit_code"],
            "failed_segment": capture["command_status"]["failed_segment"],
            "status_label": capture["command_status"]["status_label"],
            "pipeline_masking_warning": capture["command_status"]["pipeline_masking_warning"],
            "pipeline_rerun_command": capture["command_status"]["pipeline_rerun_command"],
            "shell_syntax_summary": capture["command_status"]["shell_syntax_summary"],
            "policy": capture["parser_metadata"]["policy"],
            "policy_reason": capture["parser_metadata"]["policy_reason"],
            "family": capture["parser_metadata"]["family"],
            "timeout": result.timed_out,
            "background_io_terminated": result.io_grace_expired,
            "stdout_preview": preview(&tokenzero_core::mask_visible_secrets(&stdout_display)),
            "stderr_preview": preview(&tokenzero_core::mask_visible_secrets(&stderr_display)),
            "stdout_capture": capture["stdout"],
            "stderr_capture": capture["stderr"],
            "allocator_pressure_relief": capture["allocator_pressure_relief"],
            "output_truncated": streams_truncated,
            "rewrite_applied": rewrite_result.applied,
            "rewrite_skip_reason": rewrite_result.reason,
            "latency_ms": result.duration_ms,
            "raw_tokens": raw_tokens,
            "visible_tokens": visible_tokens,
            "recovery_tokens": store.recovery_tokens,
            "capture_ref": capture_vis,
            "combined_ref": combined_vis,
            "output_strategy": output_strategy
        });
        if !stdout_full.is_empty() {
            telemetry["stdout_ref"] = json!(stdout_vis);
        }
        if !stderr_full.is_empty() {
            telemetry["stderr_ref"] = json!(stderr_vis);
        }
        response.telemetry = Some(telemetry);
        response.safety = Some(json!({
            "schema_version": "tokenzero.shell_safety.v1",
            "secret_masking": mask_visible_output,
            "hidden_critical_evidence_requires_ref": true,
            "refs_available": true,
            "refs_cover_full_output": true,
            "combined_witness_temporal_interleaving": "not_claimed"
        }));
        response
    }
}

#[cfg(test)]
mod job_tail_tests {
    use super::*;

    fn job_with_handle(log: PathBuf, file: fs::File) -> BackgroundJob {
        BackgroundJob {
            id: "job-read-test".to_string(),
            sequence: 0,
            log,
            log_file: Arc::new(Mutex::new(file)),
            state: Mutex::new(BackgroundJobState {
                status: "running",
                pid: None,
                pgid: None,
                exit_code: None,
                version: 0,
                completed_at: None,
                terminate_requested: false,
                log_error: None,
            }),
            changed: Condvar::new(),
            poll_interleave: None,
        }
    }

    #[test]
    fn retained_log_handle_reads_only_the_requested_window() {
        let mut file = tempfile::tempfile().unwrap();
        let bytes = vec![b'x'; MAX_JOB_TAIL_BYTES * 4];
        file.write_all(&bytes).unwrap();
        file.flush().unwrap();
        let job = job_with_handle(PathBuf::from("retained.log"), file);

        let (window, start, log_bytes) = read_job_window(&job, 0, MAX_JOB_TAIL_BYTES).unwrap();
        assert_eq!(start, 0);
        assert_eq!(window.len(), MAX_JOB_TAIL_BYTES);
        assert_eq!(log_bytes, bytes.len());
    }

    #[test]
    fn a_chunk_published_between_length_and_relock_does_not_lose_its_wake() {
        let file = tempfile::tempfile().unwrap();
        let hook = Arc::new(PollInterleave {
            length_observed: std::sync::Barrier::new(2),
            publication_done: std::sync::Barrier::new(2),
        });
        let mut observed = job_with_handle(PathBuf::from("interleaved.log"), file);
        observed.poll_interleave = Some(Arc::clone(&hook));
        let observed = Arc::new(observed);
        let registry = BackgroundJobRegistry::default();
        registry.insert_bounded(Arc::clone(&observed)).unwrap();

        let writer_job = Arc::clone(&observed);
        let writer = thread::spawn(move || {
            hook.length_observed.wait();
            {
                let mut log = lock(&writer_job.log_file);
                log.write_all(b"ready").unwrap();
                log.flush().unwrap();
            }
            let mut state = lock(&writer_job.state);
            state.version = state.version.saturating_add(1);
            drop(state);
            writer_job.changed.notify_all();
            hook.publication_done.wait();
        });

        let started = Instant::now();
        let result = registry
            .poll(&observed.id, Duration::from_secs(2), 0, 16)
            .unwrap();
        writer.join().unwrap();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert_eq!(result["tail"], "ready");
        assert_eq!(result["cursor"], 5);
        assert_eq!(result["changed"], true);
    }

    #[test]
    fn retained_log_read_failure_is_loud() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("write-only.log");
        fs::write(&path, b"unreadable through this handle").unwrap();
        let file = OpenOptions::new().write(true).open(&path).unwrap();
        let job = job_with_handle(path, file);

        let error = read_job_window(&job, 0, 16).unwrap_err();
        assert!(error.contains("read background log"), "{error}");
    }

    #[test]
    fn invalid_three_byte_tail_is_not_lossless_when_replacement_length_matches() {
        let truncated_four_byte_scalar = [0xf0, 0x90, 0x80];
        let (tail, lossless, consumed) = decode_job_tail(&truncated_four_byte_scalar);

        assert_eq!(consumed, truncated_four_byte_scalar.len());
        assert_eq!(tail.len(), truncated_four_byte_scalar.len());
        assert_eq!(tail, "�");
        assert!(
            !lossless,
            "serialized length is not UTF-8 validity evidence"
        );
    }

    #[test]
    fn binary_tail_stays_typed_bounded_and_advances_by_consumed_raw_bytes() {
        let binary = vec![0xff; MAX_JOB_TAIL_BYTES];
        let (tail, lossless, consumed) = decode_job_tail(&binary);

        assert!(!lossless);
        assert!(tail.len() <= MAX_JOB_TAIL_BYTES);
        assert_eq!(consumed, MAX_JOB_TAIL_BYTES / 3);
        assert!(consumed > 0 && consumed < binary.len());

        let (_, _, next_consumed) = decode_job_tail(&binary[consumed..]);
        assert!(next_consumed > 0, "the next cursor must keep progressing");
    }
}

#[cfg(all(test, unix))]
mod background_tests {
    use super::*;

    #[test]
    fn poll_returns_output_change_then_terminal_state() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackgroundJobRegistry::default();
        let launched = registry
            .start(
                vec![
                    "/bin/bash".to_string(),
                    "-c".to_string(),
                    "sleep 0.05; printf done".to_string(),
                ],
                None,
                BTreeMap::new(),
                Duration::from_secs(2),
                dir.path().to_path_buf(),
            )
            .unwrap();
        let id = launched["job"].as_str().unwrap();
        let output = registry
            .poll(id, Duration::from_secs(1), 0, DEFAULT_JOB_TAIL_BYTES)
            .unwrap();
        assert!(output["tail"].as_str().unwrap().contains("done"));
        let cursor = output["cursor"].as_u64().unwrap() as usize;
        let terminal = if output["status"] == "exited" {
            output
        } else {
            registry
                .poll(id, Duration::from_secs(1), cursor, DEFAULT_JOB_TAIL_BYTES)
                .unwrap()
        };
        assert_eq!(terminal["status"], "exited");
        assert_eq!(
            registry
                .poll(id, Duration::ZERO, cursor, DEFAULT_JOB_TAIL_BYTES)
                .unwrap()["tail"],
            ""
        );
    }

    #[test]
    fn poll_returns_live_bounded_delta_before_exit() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackgroundJobRegistry::default();
        let launched = registry
            .start(
                vec![
                    "/bin/bash".to_string(),
                    "-c".to_string(),
                    "printf first; sleep 0.2; printf second".to_string(),
                ],
                None,
                BTreeMap::new(),
                Duration::from_secs(2),
                dir.path().to_path_buf(),
            )
            .unwrap();
        let id = launched["job"].as_str().unwrap();

        let first = registry.poll(id, Duration::from_secs(1), 0, 5).unwrap();
        assert_eq!(first["tail"], "first");
        assert_eq!(first["tailBytes"], 5);
        assert_eq!(first["cursor"], 5);
        assert!(first["version"].as_u64().unwrap() >= 1);

        let second = registry.poll(id, Duration::from_secs(1), 5, 64).unwrap();
        assert!(second["tail"].as_str().unwrap().contains("second"));
        let cursor = second["cursor"].as_u64().unwrap() as usize;
        assert!(cursor > 5);
        let terminal = registry
            .poll(id, Duration::from_secs(1), cursor, 64)
            .unwrap();
        assert_eq!(terminal["status"], "exited");
    }

    #[test]
    fn unchanged_poll_is_tiny_and_supplies_backoff() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackgroundJobRegistry::default();
        let launched = registry
            .start(
                vec!["sleep".to_string(), "1".to_string()],
                None,
                BTreeMap::new(),
                Duration::from_secs(2),
                dir.path().to_path_buf(),
            )
            .unwrap();
        let id = launched["job"].as_str().unwrap();

        let observed = registry
            .poll(id, Duration::from_millis(10), 0, DEFAULT_JOB_TAIL_BYTES)
            .unwrap();
        assert_eq!(observed["status"], "running");
        assert_eq!(observed["unchanged"], true);
        assert_eq!(observed["nextPollMs"], UNCHANGED_NEXT_POLL_MS);
        assert!(observed.get("tail").is_none());
        assert!(observed.get("log").is_none());
    }

    #[test]
    fn five_minute_silent_job_needs_at_most_seven_visible_observations() {
        let five_minutes_ms = 5 * 60 * 1_000_u64;
        let observation_cycle_ms = 30_000 + UNCHANGED_NEXT_POLL_MS;
        assert!(five_minutes_ms.div_ceil(observation_cycle_ms) <= 7);
    }

    #[test]
    fn registry_drop_kills_running_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let registry = BackgroundJobRegistry::default();
        let launched = registry
            .start(
                vec!["sleep".to_string(), "30".to_string()],
                None,
                BTreeMap::new(),
                Duration::from_secs(60),
                dir.path().to_path_buf(),
            )
            .unwrap();
        let id = launched["job"].as_str().unwrap();
        let pid = (0..50)
            .find_map(|_| {
                let pid = registry
                    .poll(id, Duration::ZERO, 0, DEFAULT_JOB_TAIL_BYTES)
                    .unwrap()["pid"]
                    .as_u64();
                if pid.is_none() {
                    thread::sleep(Duration::from_millis(10));
                }
                pid
            })
            .expect("background child did not publish its pid") as u32;
        drop(registry);
        thread::sleep(Duration::from_millis(200));
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success());
        assert!(!alive, "background child {pid} survived registry drop");
    }
}

#[cfg(test)]
mod accumulator_bounds {
    use super::*;

    fn job(sequence: u64, status: &'static str) -> Arc<BackgroundJob> {
        Arc::new(BackgroundJob {
            id: format!("job-{sequence}"),
            sequence,
            log: PathBuf::from(format!("job-{sequence}.log")),
            log_file: Arc::new(Mutex::new(tempfile::tempfile().unwrap())),
            state: Mutex::new(BackgroundJobState {
                status,
                pid: None,
                pgid: None,
                exit_code: None,
                version: u64::from(status != "running"),
                completed_at: (status != "running").then(Instant::now),
                terminate_requested: false,
                log_error: None,
            }),
            changed: Condvar::new(),
            poll_interleave: None,
        })
    }

    #[test]
    fn terminating_registry_rejects_late_background_launches() {
        let registry = BackgroundJobRegistry::default();
        registry.terminate_all();
        let error = registry
            .start(
                vec!["never-spawn".to_string()],
                None,
                BTreeMap::new(),
                Duration::from_secs(1),
                PathBuf::from("unused"),
            )
            .unwrap_err();
        assert_eq!(
            error,
            "background jobs are unavailable during session teardown"
        );
    }

    #[test]
    fn expired_terminal_jobs_are_pruned_but_recent_terminal_jobs_are_retained() {
        let registry = BackgroundJobRegistry::default();
        let expired = job(0, "exited");
        lock(&expired.state).completed_at =
            Instant::now().checked_sub(COMPLETED_JOB_TTL + Duration::from_secs(1));
        lock(&registry.jobs).insert(expired.id.clone(), expired);
        registry.insert_bounded(job(1, "exited")).unwrap();

        let retained = lock(&registry.jobs);
        assert!(!retained.contains_key("job-0"));
        assert!(retained.contains_key("job-1"));
    }

    #[test]
    fn background_registry_evicts_completed_and_rejects_all_running() {
        let registry = BackgroundJobRegistry::default();
        for sequence in 0..MAX_BACKGROUND_JOBS as u64 {
            registry.insert_bounded(job(sequence, "exited")).unwrap();
        }
        registry
            .insert_bounded(job(MAX_BACKGROUND_JOBS as u64, "exited"))
            .unwrap();
        let retained = registry.jobs.lock().unwrap();
        assert_eq!(retained.len(), MAX_BACKGROUND_JOBS);
        assert!(!retained.contains_key("job-0"));
        drop(retained);

        let running = BackgroundJobRegistry::default();
        for sequence in 0..MAX_BACKGROUND_JOBS as u64 {
            running.insert_bounded(job(sequence, "running")).unwrap();
        }
        let error = running
            .insert_bounded(job(MAX_BACKGROUND_JOBS as u64, "running"))
            .unwrap_err();
        assert!(error.contains("registry is full"));
        assert_eq!(running.jobs.lock().unwrap().len(), MAX_BACKGROUND_JOBS);
    }
}
