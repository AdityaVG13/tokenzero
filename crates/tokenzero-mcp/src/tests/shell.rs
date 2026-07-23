use super::*;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

#[test]
fn compact_shell_text_render_omits_ref_footer() {
    let mut response = ToolResponse::ok(
        "shell",
        Mode::Passthrough,
        "11.12.1".to_string(),
        vec![
            ref_record("stdout", "tz://blob/stdout".to_string(), 8),
            ref_record("combined", "tz://blob/combined".to_string(), 45),
        ],
        Accounting {
            raw_tokens: 15,
            visible_tokens: 2,
            recovery_tokens: 0,
            billed_tokens: 2,
            cached_tokens: 0,
            exact_ref_tokens: Some(14),
        },
    );
    response.telemetry = Some(json!({
        "output_strategy": "compact_adaptive_shell"
    }));

    assert_eq!(render_text(&response), "11.12.1\n");
}

#[test]
fn full_shell_text_render_does_not_duplicate_header_refs() {
    // exact_first_adaptive_shell capsules carry stdout/stderr/combined
    // refs in their header; the trailer must only add refs the visible
    // text lacks (capture_ref), never repeat the anchored ones.
    let visible = "# shell\ncommand: seq 1 300\nstatus: command_success\n\
                       stdout_ref: tz://blob/bstdout\nstderr_ref: tz://blob/bstderr\n\
                       combined_ref: tz://blob/bcombined\n\n1\n2"
        .to_string();
    let mut response = ToolResponse::ok(
        "shell",
        Mode::Auto,
        visible,
        vec![
            ref_record("stdout", "tz://blob/bstdout".to_string(), 8),
            ref_record("stderr", "tz://blob/bstderr".to_string(), 0),
            ref_record("combined", "tz://blob/bcombined".to_string(), 45),
            ref_record("capture", "tz://blob/bcapture".to_string(), 60),
        ],
        Accounting {
            raw_tokens: 100,
            visible_tokens: 40,
            recovery_tokens: 0,
            billed_tokens: 40,
            cached_tokens: 0,
            exact_ref_tokens: Some(28),
        },
    );
    response.telemetry = Some(json!({
        "output_strategy": "exact_first_adaptive_shell"
    }));

    let text = render_text(&response);
    assert_eq!(text.matches("tz://blob/bstdout").count(), 1, "{text}");
    assert_eq!(text.matches("tz://blob/bstderr").count(), 1, "{text}");
    assert_eq!(text.matches("tz://blob/bcombined").count(), 1, "{text}");
    assert!(text.contains("capture_ref: tz://blob/bcapture"), "{text}");
}

#[test]
fn clean_shell_omits_empty_stderr_ref() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Write-Output clean-empty-stderr"
    } else {
        "printf 'clean-empty-stderr\\n'"
    };
    let response = engine.shell(
        command,
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let visible = response
        .visible
        .as_ref()
        .map(|v| v.text.as_str())
        .unwrap_or("");
    assert!(
        !visible.contains("stderr_ref:"),
        "empty stderr must not be referenced: {visible}"
    );
    assert!(
        response.refs.iter().all(|row| row.kind != "stderr"),
        "refs must omit empty stderr: {:?}",
        response.refs
    );
    let telemetry = response.telemetry.as_ref().unwrap();
    assert!(
        telemetry.get("stderr_ref").is_none(),
        "telemetry must omit empty stderr_ref: {telemetry}"
    );
}

#[test]
fn shell_emits_canonical_refs_recoverable_by_a_fresh_engine() {
    let dir = tempdir().unwrap();
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command Write-Output durable-shell-ref"
    } else {
        "printf 'durable-shell-ref\\n'"
    };
    let response = TokenZeroEngine::new(EngineConfig::for_root(dir.path())).shell(
        command,
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    let stdout_ref = response.telemetry.as_ref().unwrap()["stdout_ref"]
        .as_str()
        .unwrap();
    assert!(
        stdout_ref.starts_with("tz://blob/"),
        "shell response refs must survive session alias pruning: {stdout_ref}"
    );
    let expanded = TokenZeroEngine::new(EngineConfig::for_root(dir.path())).expand(
        stdout_ref,
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    assert_eq!(expanded.visible.unwrap().text, "durable-shell-ref\n");
}

#[test]
fn short_similar_shell_output_stays_verbatim() {
    let dir = tempdir().unwrap();
    let command = if cfg!(windows) {
        "powershell -NoProfile -Command 1..30 | ForEach-Object { 'similar-line-{0:D2}' -f $_ }"
    } else {
        "for i in $(seq 1 30); do printf 'similar-line-%02d\\n' \"$i\"; done"
    };
    let response = TokenZeroEngine::new(EngineConfig::for_root(dir.path())).shell(
        command,
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    let visible = response.visible.unwrap().text;
    for i in 1..=30 {
        assert!(
            visible.contains(&format!("similar-line-{i:02}")),
            "line {i} was compacted out of shell output: {visible}"
        );
    }
}

#[test]
fn shell_exact_first_stores_stream_refs_and_status_truth() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let (command, argv, expanded_needle) = if cfg!(windows) {
        (
            "powershell -NoProfile -Command [Console]::Out.Write('alpha'); [Console]::Error.Write('beta'); exit 7",
            Some(vec![
                "powershell".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                "[Console]::Out.Write('alpha'); [Console]::Error.Write('beta'); exit 7".to_string(),
            ]),
            Some("alpha"),
        )
    } else {
        ("false | true", None, None)
    };

    let response = engine.shell(
        command,
        argv,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(response.mode.as_deref(), Some("diagnostic"));
    assert_eq!(
        response.telemetry.as_ref().unwrap()["transport_status"],
        "ok"
    );
    assert_eq!(
        response.telemetry.as_ref().unwrap()["command_success"],
        false
    );
    assert!(
        response.refs.iter().any(|row| row.kind == "combined"),
        "shell must always emit a combined ref"
    );
    let combined_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "combined")
        .unwrap()
        .ref_id
        .clone();
    assert!(
        combined_ref.starts_with("tz://s/") || combined_ref.starts_with("tz://blob/"),
        "combined ref must be recoverable: {combined_ref}"
    );
    let expanded = engine.expand(&combined_ref, Some("raw"), None, None, None, None);
    let expanded_text = expanded.visible.unwrap().text;
    if let Some(needle) = expanded_needle {
        assert!(expanded_text.contains(needle));
    }
    assert!(
        !expanded_text.contains(command),
        "combined output must not echo the command"
    );
}

#[test]
fn shell_command_strings_preserve_shell_operators() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "echo one && echo two",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.telemetry.as_ref().unwrap()["execution_mode"],
        "shell"
    );
    let stdout_preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap();
    assert_eq!(
        stdout_preview
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>(),
        vec!["one", "two"]
    );
    assert_eq!(
        response.telemetry.as_ref().unwrap()["command_success"],
        true
    );
}

#[test]
fn shell_rejects_cwd_outside_allowed_roots() {
    let allowed = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(allowed.path()));

    let response = engine.shell(
        "echo nope",
        None,
        Some(outside.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "error");
    assert_eq!(
        response.error.as_ref().unwrap().code,
        "path_outside_allowed_roots"
    );
}

#[test]
fn shell_capture_record_is_compact_json() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "echo compact",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    let capture_ref = response.telemetry.as_ref().unwrap()["capture_ref"]
        .as_str()
        .unwrap()
        .to_string();
    let expanded = engine.expand(&capture_ref, Some("raw"), None, None, None, None);
    let capture_text = expanded.visible.unwrap().text;
    assert!(serde_json::from_str::<Value>(&capture_text).is_ok());
    assert_eq!(capture_text.lines().count(), 1);
}

#[cfg(not(windows))]
#[test]
fn shell_truncation_is_explicit_and_degraded() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.shell_capture_bytes = 12;
    config.shell_spill_bytes = 6;
    let engine = TokenZeroEngine::new(config);

    let response = engine.shell(
        "yes x | head -c 100 || true",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    assert_eq!(
        response.diagnostic.as_ref().unwrap().code,
        "shell_output_truncated"
    );
    assert!(
        response
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("tokenzero:stdout truncated")
    );
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["transport_status"], "degraded");
    assert_eq!(telemetry["output_truncated"], true);
    assert_eq!(telemetry["stdout_capture"]["truncated"], true);
    assert_eq!(telemetry["stdout_capture"]["bytes_seen"], 100);
    let spill_path = telemetry["stdout_capture"]["spill_path"].as_str().unwrap();
    assert_eq!(std::fs::metadata(spill_path).unwrap().len(), 100);
    assert_eq!(
        response.safety.as_ref().unwrap()["refs_cover_full_output"],
        false
    );
}

#[cfg(windows)]
#[test]
fn shell_command_string_adapts_raw_powershell_variables() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let script = "$tzTmp = Join-Path $env:TEMP 'tz-quote'; [Console]::Out.Write($tzTmp)";

    let response = engine.shell(
        script,
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["command_success"], true);
    assert_eq!(telemetry["execution_mode"], "shell");
    assert_eq!(telemetry["argv"][0], "powershell");
    assert!(
        telemetry["stdout_preview"]
            .as_str()
            .unwrap()
            .ends_with("tz-quote")
    );
}

#[test]
fn shell_accepts_common_command_argument_aliases() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    for args in [
        json!({"cmd": "echo alias"}),
        json!({"input": "echo input"}),
        json!({"args": ["echo", "one", "&&", "echo", "two"]}),
        json!(["echo", "array"]),
    ] {
        let response = call_tool(&engine, "shell", &args, None).unwrap();
        assert!(
            response.get("isError").is_none(),
            "alias args must execute successfully: {response}"
        );
        let text = response["content"][0]["text"].as_str().unwrap();
        assert!(!text.is_empty(), "{response}");
    }
}

#[cfg(unix)]
#[test]
fn shell_children_inherit_tokenzero_inner_guard() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let response = engine.shell(
        "sh -c 'echo INNER=$TOKENZERO_INNER'",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(preview.contains("INNER=1"), "{preview}");
}

#[cfg(unix)]
#[test]
fn shell_caller_env_overrides_inner_guard() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    let mut env = BTreeMap::new();
    env.insert("TOKENZERO_INNER".to_string(), "custom".to_string());
    let response = engine.shell(
        "sh -c 'echo INNER=$TOKENZERO_INNER'",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        Some(env),
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let preview = response.telemetry.as_ref().unwrap()["stdout_preview"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(preview.contains("INNER=custom"), "{preview}");
}

#[cfg(unix)]
#[test]
fn shell_scrubs_inherited_orchestration_env() {
    const PROBE_FLAG: &str = "TOKENZERO_ENV_SCRUB_PROBE_PARENT";
    if std::env::var_os(PROBE_FLAG).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "tests::shell::shell_scrubs_inherited_orchestration_env",
                "--nocapture",
            ])
            .env(PROBE_FLAG, "1")
            .env("ZEROSTACK_STORE_ROOT", "inherited-store")
            .env("FSZERO_ROOT", "inherited-fszero")
            .env("GRAPHZERO_ROOT", "inherited-graphzero")
            .env("TOKENZERO_CACHE_PATH", "inherited-cache")
            .env("TOKENZERO_ALLOWED_ROOTS", "inherited-roots")
            .env("TOKENZERO_EXPLICIT_CHILD", "inherited-value")
            .env("SHELL_ENV_CONTROL", "preserved")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "probe subprocess failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let script = r#"printf 'ZEROSTACK=%s\nFSZERO=%s\nGRAPHZERO=%s\nTOKENZERO_CACHE=%s\nTOKENZERO_ALLOWED=%s\nTOKENZERO_EXPLICIT=%s\nCONTROL=%s\nPATH_PRESENT=%s\n' "${ZEROSTACK_STORE_ROOT-absent}" "${FSZERO_ROOT-absent}" "${GRAPHZERO_ROOT-absent}" "${TOKENZERO_CACHE_PATH-absent}" "${TOKENZERO_ALLOWED_ROOTS-absent}" "${TOKENZERO_EXPLICIT_CHILD-absent}" "${SHELL_ENV_CONTROL-absent}" "${PATH:+yes}""#;
    let argv = Some(vec!["sh".to_string(), "-c".to_string(), script.to_string()]);
    let response = engine.shell(
        "env scrub probe",
        argv,
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let stdout_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "combined")
        .unwrap()
        .ref_id
        .clone();
    let stdout = engine
        .expand(&stdout_ref, Some("raw"), None, None, None, None)
        .visible
        .unwrap()
        .text;
    assert_eq!(
        stdout.trim_end(),
        "ZEROSTACK=absent\nFSZERO=absent\nGRAPHZERO=absent\nTOKENZERO_CACHE=absent\nTOKENZERO_ALLOWED=absent\nTOKENZERO_EXPLICIT=absent\nCONTROL=preserved\nPATH_PRESENT=yes"
    );

    let mut explicit_env = BTreeMap::new();
    explicit_env.insert(
        "TOKENZERO_EXPLICIT_CHILD".to_string(),
        "opted-in".to_string(),
    );
    let response = engine.shell(
        "explicit env probe",
        Some(vec![
            "sh".to_string(),
            "-c".to_string(),
            r#"printf '%s' "${TOKENZERO_EXPLICIT_CHILD-absent}""#.to_string(),
        ]),
        Some(dir.path()),
        Mode::Auto,
        None,
        true,
        Some(explicit_env),
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let stdout_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "combined")
        .unwrap()
        .ref_id
        .clone();
    let stdout = engine
        .expand(&stdout_ref, Some("raw"), None, None, None, None)
        .visible
        .unwrap()
        .text;
    assert_eq!(stdout, "opted-in");
}

#[test]
fn shell_defaults_cwd_to_call_root_without_repeating_it_in_visible_output() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join("cwd-marker.txt");
    std::fs::write(&marker, "here").unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));

    // No explicit cwd: must land in call_root, not silent process cwd.
    let response = engine.shell(
        if cfg!(windows) {
            "powershell -NoProfile -Command Get-Location | Select-Object -ExpandProperty Path"
        } else {
            "pwd"
        },
        None,
        None,
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    let cwd = telemetry["cwd"].as_str().unwrap();
    let canonical_dir = dir
        .path()
        .canonicalize()
        .unwrap_or_else(|_| dir.path().to_path_buf());
    let canonical_cwd = PathBuf::from(cwd)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(cwd));
    assert_eq!(canonical_cwd, canonical_dir, "cwd={cwd}");
    assert_eq!(telemetry["cwd_source"], "call_root");
    let visible = response.visible.as_ref().unwrap().text.clone();
    assert!(
        !visible.contains("cwd: "),
        "plan-root cwd is request context and must not bloat visible output: {visible}"
    );
}
#[test]
fn shell_small_success_has_one_ref_and_compact_visible_envelope() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = engine.shell(
        "echo hi",
        None,
        None,
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );

    assert_eq!(response.refs.len(), 1);
    assert_eq!(response.refs[0].kind, "combined");
    let visible = &response.visible.as_ref().unwrap().text;
    assert!(!visible.contains("$ echo hi"));
    assert!(!visible.contains("cwd: "));
    assert!(
        response.accounting.as_ref().unwrap().visible_tokens <= 40,
        "{visible}"
    );
}

#[test]
fn shell_explicit_cwd_sets_cwd_source_explicit() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = engine.shell(
        "echo ok",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["cwd_source"], "explicit");
    assert!(telemetry["cwd"].as_str().is_some_and(|c| !c.is_empty()));
}

#[test]
fn shell_response_refs_are_canonical_and_expand_in_fresh_engine() {
    let dir = tempdir().unwrap();
    let response = TokenZeroEngine::new(EngineConfig::for_root(dir.path())).shell(
        "echo canonical-ref-probe",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    assert_eq!(response.status, "ok");
    let combined = response
        .refs
        .iter()
        .find(|row| row.kind == "combined")
        .expect("combined ref")
        .ref_id
        .clone();
    assert!(
        combined.starts_with("tz://blob/"),
        "combined ref must be canonical, got {combined}"
    );
    let expanded = TokenZeroEngine::new(EngineConfig::for_root(dir.path())).expand(
        &combined,
        Some("raw"),
        None,
        None,
        None,
        None,
    );
    let text = expanded.visible.as_ref().unwrap().text.clone();
    assert!(
        text.contains("canonical-ref-probe"),
        "fresh-engine expand must recover bytes: {text}"
    );
}

#[cfg(unix)]
#[test]
fn shell_pipeline_propagates_upstream_failure() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = engine.shell(
        "sh -c 'echo producer-failed >&2; exit 7' | cat",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["command_success"], false);
    assert_eq!(telemetry["exit_code"], 7);
}

#[cfg(unix)]
#[test]
fn shell_pipeline_allows_explicit_failure_masking() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    let response = engine.shell(
        "sh -c 'exit 7' | cat || true",
        None,
        Some(dir.path()),
        Mode::Auto,
        None,
        false,
        None,
        None,
        None,
    );
    let telemetry = response.telemetry.as_ref().unwrap();
    assert_eq!(telemetry["command_success"], true);
    assert_eq!(telemetry["exit_code"], 0);
}
