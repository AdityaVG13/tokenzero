use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar};
use std::thread;
use std::time::Instant;
use tokenzero_runtime::run_command_with_policy_observer;

#[derive(Debug)]
struct BackgroundJobState {
    status: &'static str,
    pid: Option<u32>,
    pgid: Option<u32>,
    exit_code: Option<i32>,
}

const MAX_BACKGROUND_JOBS: usize = 256;

fn shell_argv(command: &str) -> Vec<String> {
    if contains_platform_shell_syntax(command, tokenzero_runtime::current_platform()) {
        vec![command.to_string()]
    } else {
        split_command_string(command)
    }
}

#[derive(Debug)]
struct BackgroundJob {
    id: String,
    sequence: u64,
    log: PathBuf,
    state: Mutex<BackgroundJobState>,
    changed: Condvar,
}

fn background_job_is_complete(job: &BackgroundJob) -> bool {
    matches!(lock(&job.state).status, "exited" | "failed")
}

#[derive(Debug, Default)]
pub(crate) struct BackgroundJobRegistry {
    next_id: AtomicU64,
    jobs: Mutex<BTreeMap<String, Arc<BackgroundJob>>>,
}

static BACKGROUND_JOBS: OnceLock<BackgroundJobRegistry> = OnceLock::new();

fn background_jobs() -> &'static BackgroundJobRegistry {
    BACKGROUND_JOBS.get_or_init(BackgroundJobRegistry::default)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
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
            "combined",
            ContentType::ShellOutput,
        ),
    )
}

fn shell_ref(kind: &str, stored: &StoredPayload, bytes: usize) -> tokenzero_core::RefRecord {
    ref_record(kind, stored.blob_ref.clone(), bytes)
}

macro_rules! shell_stream_capture {
    ($display:expr, $capture:expr, $stored:expr) => {
        json!({
            "bytes": $display.len(),
            "bytes_seen": $capture.bytes_seen,
            "captured_bytes": $capture.captured_bytes,
            "truncated": $capture.truncated,
            "spill_path": $capture.spill_path,
            "spill_bytes": $capture.spill_bytes,
            "sha256": $stored
                .blob_ref
                .strip_prefix("tz://blob/")
                .unwrap_or(&$stored.blob_ref),
            "sha256_scope": "captured_display",
            "ref": $stored.blob_ref
        })
    };
}

impl BackgroundJobRegistry {
    fn insert_bounded(&self, job: Arc<BackgroundJob>) -> Result<(), String> {
        let mut jobs = lock(&self.jobs);
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
        fs::create_dir_all(&log_dir).map_err(|err| format!("create background log dir: {err}"))?;
        let sequence = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = format!("tzjob-{}-{sequence}", std::process::id());
        let log = log_dir.join(format!("{id}.log"));
        fs::write(&log, []).map_err(|err| format!("create background log: {err}"))?;
        let job = Arc::new(BackgroundJob {
            id: id.clone(),
            sequence,
            log: log.clone(),
            state: Mutex::new(BackgroundJobState {
                status: "running",
                pid: None,
                pgid: None,
                exit_code: None,
            }),
            changed: Condvar::new(),
        });
        if let Err(error) = self.insert_bounded(Arc::clone(&job)) {
            let _ = fs::remove_file(&log);
            return Err(error);
        }
        crate::shell_hooks::reserve_background_job(&id);
        let worker_id = id.clone();
        let spawn = thread::Builder::new()
            .name(format!("tokenzero-{id}"))
            .spawn(move || {
                let observed = Arc::clone(&job);
                let result = run_command_with_policy_observer(
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
                            drop(current);
                            crate::shell_hooks::note_background_child(&observed.id, pid, pgid);
                        }
                    },
                );
                let (text, exit_code, status) = match result {
                    Ok(result) => (
                        shell_combined_output(
                            &result.command,
                            result.exit_code,
                            &result.stdout,
                            &result.stderr,
                        ),
                        result.exit_code,
                        "exited",
                    ),
                    Err(err) => (format!("background shell failed: {err}\n"), None, "failed"),
                };
                let _ = fs::write(&job.log, text);
                let mut current = lock(&job.state);
                current.status = status;
                current.exit_code = exit_code;
                drop(current);
                job.changed.notify_all();
                crate::shell_hooks::finish_background_job(&worker_id);
            });
        if let Err(err) = spawn {
            lock(&self.jobs).remove(&id);
            crate::shell_hooks::finish_background_job(&id);
            return Err(format!("spawn background worker: {err}"));
        }
        Ok(json!({"job": id, "log": log.display().to_string()}))
    }
    fn poll(&self, id: &str, wait: Duration, cursor: usize) -> Result<Value, String> {
        let job = lock(&self.jobs)
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown background job: {id}"))?;
        let mut state = lock(&job.state);
        if state.status == "running" && !wait.is_zero() {
            let deadline = Instant::now() + wait.min(Duration::from_secs(30));
            while state.status == "running" {
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
        let log_text = fs::read_to_string(&job.log).unwrap_or_default();
        let total_chars = log_text.chars().count();
        let tail = log_text
            .chars()
            .skip(cursor.min(total_chars))
            .take(8192)
            .collect::<String>();
        Ok(
            json!({"status": state.status, "pid": state.pid, "exitCode": state.exit_code,
            "tail": tail, "log": job.log.display().to_string(), "cursor": total_chars,
            "changed": total_chars > cursor || state.status != "running"}),
        )
    }

    fn terminate_all(&self) {
        let jobs = lock(&self.jobs);
        for job in jobs.values() {
            let state = lock(&job.state);
            if state.status == "running" {
                if let Some(pgid) = state.pgid {
                    terminate_background_group(pgid);
                }
            }
        }
    }
}

impl Drop for BackgroundJobRegistry {
    fn drop(&mut self) {
        self.terminate_all();
    }
}

#[cfg(unix)]
fn terminate_background_group(pgid: u32) {
    if pgid == 0 {
        return;
    }
    let target = format!("-{pgid}");
    let _ = std::process::Command::new("kill")
        .args(["-TERM", "--", &target])
        .status();
    thread::sleep(Duration::from_millis(50));
    let _ = std::process::Command::new("kill")
        .args(["-KILL", "--", &target])
        .status();
}

#[cfg(not(unix))]
fn terminate_background_group(_: u32) {}

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
        }
        Ok(launched)
    }

    pub fn shell_job(&self, id: &str) -> Result<Value, String> {
        background_jobs().poll(id, Duration::ZERO, 0)
    }

    pub fn shell_job_wait(&self, id: &str, wait: Duration, cursor: usize) -> Result<Value, String> {
        background_jobs().poll(id, wait, cursor)
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
        let rewrite_result =
            rewrite_command(command, rewrite_mode, !no_rewrite && rewrite_mode != "off");
        let run_argv = argv.unwrap_or_else(|| {
            if contains_platform_shell_syntax(command, tokenzero_runtime::current_platform()) {
                vec![command.to_string()]
            } else {
                split_command_string(command)
            }
        });
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
        let command_digest = sha256_hex(display_command);
        let (stdout_stored, stderr_stored, combined_stored) = store_shell_outputs(
            &mut store,
            &stdout_display,
            &stderr_display,
            &output,
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
            "stdout": shell_stream_capture!(&stdout_display, result.stdout_capture, stdout_stored),
            "stderr": shell_stream_capture!(&stderr_display, result.stderr_capture, stderr_stored),
            "combined": {
                "bytes": output.len(),
                "truncated": streams_truncated,
                "sha256": combined_stored.blob_ref.strip_prefix("tz://blob/").unwrap_or(&combined_stored.blob_ref),
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
        if !stdout_display.is_empty() {
            refs.push(shell_ref("stdout", &stdout_stored, stdout_display.len()));
        }
        if !stderr_display.is_empty() {
            refs.push(shell_ref("stderr", &stderr_stored, stderr_display.len()));
        }
        refs.push(shell_ref("combined", &combined_stored, output.len()));
        refs.push(shell_ref("capture", &capture_stored, capture_text.len()));
        let persisted = persist_refs(&mut store, &mut refs);
        if let Some(error) = persisted.error {
            return degraded_shell_response(command, mode, &output, error);
        }
        let refs_complete = persisted.refs_complete;
        let raw_tokens =
            shell_raw_tokens(command, result.exit_code, &stdout_display, &stderr_display);
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
            vec![shell_ref("combined", &combined_stored, output.len())]
        } else {
            refs
        };
        // The plan root is already part of the zero_execute request. Surface cwd only
        // when the command deliberately runs somewhere else.
        let visible_text = if resolved_cwd == self.config.call_root {
            visible_text
        } else if visible_text.contains("\ncwd: ") || visible_text.starts_with("cwd: ") {
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
                    if !stdout_display.is_empty() {
                        total += count_tokens(&stdout_vis);
                    }
                    if !stderr_display.is_empty() {
                        total += count_tokens(&stderr_vis);
                    }
                    total
                }),
            },
        );
        response.content_type = Some(ContentType::ShellOutput.to_string());
        if streams_truncated {
            response.diagnostic = Some(tokenzero_core::Diagnostic {
                code: "shell_output_truncated".to_string(),
                message: "shell output exceeded per-stream capture limits; refs contain captured display output and spill paths point to full local stream logs".to_string(),
                repair: Some("rerun with a narrower command or increase TOKENZERO_SHELL_CAPTURE_BYTES".to_string()),
            });
        }
        let mut telemetry = json!({
            "command": display_command,
            "argv": capture["argv"],
            "execution_mode": result.execution_mode,
            "alias_dependency": result.alias_dependency,
            "cwd": capture["cwd"],
            "cwd_source": capture["cwd_source"],
            "transport_status": if streams_truncated { "degraded" } else { "ok" },
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
            "stdout_preview": preview(&stdout_display),
            "stderr_preview": preview(&stderr_display),
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
        if !stdout_display.is_empty() {
            telemetry["stdout_ref"] = json!(stdout_vis);
        }
        if !stderr_display.is_empty() {
            telemetry["stderr_ref"] = json!(stderr_vis);
        }
        response.telemetry = Some(telemetry);
        response.safety = Some(json!({
            "schema_version": "tokenzero.shell_safety.v1",
            "secret_masking": render.policy.policy != "exact" && render.policy.policy != "passthrough",
            "hidden_critical_evidence_requires_ref": true,
            "refs_available": true,
            "refs_cover_full_output": !streams_truncated
        }));
        response
    }
}

#[cfg(all(test, unix))]
mod background_tests {
    use super::*;

    #[test]
    fn poll_waits_for_completion_and_advances_cursor() {
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
        let observed = registry.poll(id, Duration::from_secs(1), 0).unwrap();
        assert_eq!(observed["status"], "exited");
        assert!(observed["tail"].as_str().unwrap().contains("done"));
        let cursor = observed["cursor"].as_u64().unwrap() as usize;
        assert_eq!(
            registry.poll(id, Duration::ZERO, cursor).unwrap()["tail"],
            ""
        );
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
                let pid = registry.poll(id, Duration::ZERO, 0).unwrap()["pid"].as_u64();
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
            state: Mutex::new(BackgroundJobState {
                status,
                pid: None,
                pgid: None,
                exit_code: None,
            }),
            changed: Condvar::new(),
        })
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
