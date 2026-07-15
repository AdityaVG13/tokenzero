use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
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
        });
        if let Err(error) = self.insert_bounded(Arc::clone(&job)) {
            let _ = fs::remove_file(&log);
            return Err(error);
        }
        crate::codemode::containment::reserve_background_job(&id);
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
                            crate::codemode::containment::note_background_child(
                                &observed.id,
                                pid,
                                pgid,
                            );
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
                crate::codemode::containment::finish_background_job(&worker_id);
            });
        if let Err(err) = spawn {
            lock(&self.jobs).remove(&id);
            crate::codemode::containment::finish_background_job(&id);
            return Err(format!("spawn background worker: {err}"));
        }
        Ok(json!({"job": id, "log": log.display().to_string()}))
    }

    fn poll(&self, id: &str) -> Result<Value, String> {
        let job = lock(&self.jobs)
            .get(id)
            .cloned()
            .ok_or_else(|| format!("unknown background job: {id}"))?;
        let state = lock(&job.state);
        let log_text = fs::read_to_string(&job.log).unwrap_or_default();
        let tail = log_text
            .chars()
            .rev()
            .take(8192)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        Ok(json!({
            "status": state.status,
            "pid": state.pid,
            "exitCode": state.exit_code,
            "tail": tail,
            "log": job.log.display().to_string(),
        }))
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
    pub(crate) fn shell_background(
        &self,
        command: &str,
        cwd: Option<&Path>,
        timeout_override: Option<Duration>,
    ) -> Result<Value, String> {
        if let Some(cwd) = cwd {
            if !self.path_allowed(cwd) {
                return Err(format!("cwd is outside allowed roots: {}", cwd.display()));
            }
        }
        let run_argv = shell_argv(command);
        let child_env = inner_env();
        background_jobs().start(
            run_argv,
            cwd.map(Path::to_path_buf),
            child_env,
            timeout_override.unwrap_or(self.config.shell_timeout),
            shell_spill_dir(&self.config.cache_path),
        )
    }

    pub(crate) fn shell_job(&self, id: &str) -> Result<Value, String> {
        background_jobs().poll(id)
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
        if let Some(cwd) = cwd {
            if !self.path_allowed(cwd) {
                return ToolResponse::error(
                    "shell",
                    "path_outside_allowed_roots",
                    format!("cwd is outside allowed roots: {}", cwd.display()),
                    Some("set cwd under an allowed root".to_string()),
                );
            }
        }
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
            cwd,
            Some(&child_env),
            stdin,
            timeout_override.unwrap_or(self.config.shell_timeout),
            false,
            output_policy,
            crate::codemode::containment::note_child,
        ) {
            Ok(result) => result,
            Err(err) => {
                crate::codemode::containment::note_child(None, None, "spawn_failed");
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
            command: display_command,
            stdout: &stdout_display,
            stderr: &stderr_display,
            exit_code: result.exit_code,
            timed_out: result.timed_out,
            mode,
            max_visible_tokens: self.config.max_visible_tokens,
            stdout_ref: Some(&stdout_stored.blob_ref),
            stderr_ref: Some(&stderr_stored.blob_ref),
            combined_ref: Some(&combined_stored.blob_ref),
        });
        let capture = json!({
            "schema_version": "tokenzero.capture.v1",
            "command": display_command,
            "argv": result.argv,
            "cwd": result.cwd,
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
        let mut refs = vec![
            shell_ref("stdout", &stdout_stored, stdout_display.len()),
            shell_ref("stderr", &stderr_stored, stderr_display.len()),
            shell_ref("combined", &combined_stored, output.len()),
            shell_ref("capture", &capture_stored, capture_text.len()),
        ];
        let persisted = persist_refs(&mut store, &mut refs);
        if let Some(error) = persisted.error {
            return degraded_shell_response(command, mode, &output, error);
        }
        let refs_complete = persisted.refs_complete;
        let raw_tokens = count_tokens(&output);
        let inline_shell_output = refs_complete
            && !streams_truncated
            && render.command_status.command_success
            && self.config.shell_inline_budget > 0
            && raw_tokens <= self.config.shell_inline_budget;
        let visible_text = if inline_shell_output {
            let trimmed = output.trim_end();
            if trimmed.is_empty() {
                format!("combined_ref: {}", combined_stored.blob_ref)
            } else {
                format!("{trimmed}\ncombined_ref: {}", combined_stored.blob_ref)
            }
        } else if refs_complete
            && self.config.shell_inline_budget == 0
            && !streams_truncated
            && raw_tokens <= DEFAULT_SHELL_INLINE_BUDGET
            && render.command_status.command_success
        {
            format!("# shell ok\ncombined_ref: {}", combined_stored.blob_ref)
        } else if refs_complete {
            render.visible.clone()
        } else {
            output.trim_end().to_string()
        };
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
            refs,
            Accounting {
                raw_tokens,
                visible_tokens,
                recovery_tokens: store.recovery_tokens,
                exact_ref_tokens: Some(
                    count_tokens(&stdout_stored.blob_ref)
                        + count_tokens(&stderr_stored.blob_ref)
                        + count_tokens(&combined_stored.blob_ref)
                        + count_tokens(&capture_stored.blob_ref),
                ),
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
        response.telemetry = Some(json!({
            "command": display_command,
            "argv": capture["argv"],
            "execution_mode": result.execution_mode,
            "alias_dependency": result.alias_dependency,
            "cwd": capture["cwd"],
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
            "capture_ref": capture_stored.blob_ref,
            "stdout_ref": stdout_stored.blob_ref,
            "stderr_ref": stderr_stored.blob_ref,
            "combined_ref": combined_stored.blob_ref,
            "output_strategy": output_strategy        }));
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
                let pid = registry.poll(id).unwrap()["pid"].as_u64();
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
