//! `TokenZeroEngine` methods extracted from `lib.rs`.
#![allow(unused_imports)]

use super::cache_pack::{
    cache_pack_manifest_path, cache_pack_sources, previous_cache_digest, read_line_range_from_file,
};
use super::collect::*;
use super::config::{
    EngineConfig, FETCH_ALLOW_ENV, FETCH_DENY_ENV, FETCH_ENABLED_ENV, ServeFlight, new_session_id,
};
use super::fetch_cache::{epoch_secs, fetch_index_path, load_fetch_index, record_fetch};
use super::fetch_guard::{FETCH_META_MARKER, split_fetch_meta, validate_fetch_target};
use super::metrics;
use super::paths::*;
use super::render::*;
use super::session::{
    DiffTelemetry, SeenState, ServeKey, ServedRecord, SessionMemory, SessionSummary,
};
use super::{
    Accounting, ContentType, DEFAULT_MCP_IDLE_TIMEOUT_SECS, DEFAULT_SHELL_TIMEOUT_SECS,
    DIFF_MAX_BYTES, DIFF_MAX_LINES, DIFF_READS_ENV, EditHunk, MAX_MCP_IDLE_TIMEOUT_SECS,
    MAX_SEARCH_VISITED_FILES, MAX_SHELL_TIMEOUT_SECS, MIN_SEARCH_VISITED_FILES, Mode, RG_PATH_ENV,
    SEARCH_BACKEND_ENV, SEARCH_VISIT_MULTIPLIER, SESSION_DEDUP_ENV, SearchBackend, ServeOptions,
    ShellRenderInput, TokenZeroEngine, ToolResponse, cache_maintenance, count_tokens,
    detect_content_type, make_capsule, make_capsule_with_raw_tokens, ref_record, render_shell,
    sha256_hex, shell_combined_output, shell_spill_dir, shell_timeout_from_secs,
    split_command_string,
};
use crate::recall;
use globset::{GlobBuilder, GlobMatcher};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, SystemTime};
use tokenzero_filters::rewrite_command;
use tokenzero_recovery::{ExpansionResult, RecoveryStore, StoredPayload};
use tokenzero_runtime::{
    RunOutputPolicy, StreamCapture, contains_platform_shell_syntax, run_command_with_policy,
};

impl TokenZeroEngine {
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
        let output_policy = RunOutputPolicy {
            per_stream_capture_bytes: self.config.shell_capture_bytes,
            spill_threshold_bytes: self.config.shell_spill_bytes,
            spill_dir: Some(shell_spill_dir(&self.config.cache_path)),
        }
        .normalized();
        let result = match run_command_with_policy(
            &run_argv,
            cwd,
            Some(&child_env),
            stdin,
            timeout_override.unwrap_or(self.config.shell_timeout),
            false,
            output_policy,
        ) {
            Ok(result) => result,
            Err(err) => {
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
        let mut store = RecoveryStore::new(Some(self.config.cache_path.clone()));
        let command_digest = sha256_hex(display_command);
        let stdout_source = PathBuf::from(format!("shell:stdout:{command_digest}"));
        let stderr_source = PathBuf::from(format!("shell:stderr:{command_digest}"));
        let combined_source = PathBuf::from(format!("shell:combined:{command_digest}"));
        let stdout_stored = store.store_payload_deferred(
            &stdout_display,
            ContentType::ShellOutput,
            Some(&stdout_source),
            None,
            None,
        );
        let stderr_stored = store.store_payload_deferred(
            &stderr_display,
            ContentType::ShellOutput,
            Some(&stderr_source),
            None,
            None,
        );
        let combined_stored = store.store_payload_deferred(
            &output,
            ContentType::ShellOutput,
            Some(&combined_source),
            None,
            None,
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
            "stdout": {
                "bytes": stdout_display.len(),
                "bytes_seen": result.stdout_capture.bytes_seen,
                "captured_bytes": result.stdout_capture.captured_bytes,
                "truncated": result.stdout_capture.truncated,
                "spill_path": result.stdout_capture.spill_path,
                "spill_bytes": result.stdout_capture.spill_bytes,
                "sha256": sha256_hex(&stdout_display),
                "sha256_scope": "captured_display",
                "ref": stdout_stored.blob_ref
            },
            "stderr": {
                "bytes": stderr_display.len(),
                "bytes_seen": result.stderr_capture.bytes_seen,
                "captured_bytes": result.stderr_capture.captured_bytes,
                "truncated": result.stderr_capture.truncated,
                "spill_path": result.stderr_capture.spill_path,
                "spill_bytes": result.stderr_capture.spill_bytes,
                "sha256": sha256_hex(&stderr_display),
                "sha256_scope": "captured_display",
                "ref": stderr_stored.blob_ref
            },
            "combined": {
                "bytes": output.len(),
                "truncated": streams_truncated,
                "sha256": sha256_hex(&output),
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
        let capture_source = PathBuf::from(format!("shell:capture:{command_digest}"));
        let capture_stored = store.store_payload_deferred(
            &capture_text,
            ContentType::JsonConfig,
            Some(&capture_source),
            None,
            None,
        );
        if let Err(err) = store.persist_pending() {
            return degraded_shell_response(command, mode, &output, err.to_string());
        }
        let mut refs = vec![
            ref_record(
                "stdout",
                stdout_stored.blob_ref.clone(),
                stdout_display.len(),
            ),
            ref_record(
                "stderr",
                stderr_stored.blob_ref.clone(),
                stderr_display.len(),
            ),
            ref_record("combined", combined_stored.blob_ref.clone(), output.len()),
            ref_record(
                "capture",
                capture_stored.blob_ref.clone(),
                capture_text.len(),
            ),
        ];
        let refs_complete = prune_dead_refs(&store, &mut refs);
        let raw_tokens = count_tokens(&output);
        let visible_text = if refs_complete {
            render.visible.clone()
        } else {
            output.trim_end().to_string()
        };
        let visible_tokens = if refs_complete {
            count_tokens(&visible_text)
        } else {
            raw_tokens
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
            "output_strategy": capture["parser_metadata"]["output_strategy"]
        }));
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
