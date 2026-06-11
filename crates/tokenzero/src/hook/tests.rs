use super::*;

const EXE: &str = "/opt/tz/tokenzero";

fn bash_payload(command: &str) -> String {
    json!({
        "session_id": "unit",
        "cwd": "/tmp",
        "permission_mode": "default",
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {
            "command": command,
            "description": "unit case",
            "timeout": 120000,
        }
    })
    .to_string()
}

#[test]
fn rewrite_wraps_command_under_run_sh_c() {
    assert_eq!(
        rewrite_decision("true", EXE).unwrap(),
        "'/opt/tz/tokenzero' run -- sh -c 'true'"
    );
}

#[test]
fn rewrite_escapes_embedded_single_quotes() {
    assert_eq!(
        rewrite_decision("echo 'a b'", EXE).unwrap(),
        "'/opt/tz/tokenzero' run -- sh -c 'echo '\"'\"'a b'\"'\"''"
    );
}

#[test]
fn rewrite_preserves_double_quotes_dollars_backticks_newlines_verbatim() {
    // No single quotes in the command: the wrapper must pass these bytes
    // through completely untouched inside its own single quotes.
    let command = "echo \"$HOME\" `tick` —日本語\necho second";
    let rewritten = rewrite_decision(command, EXE).unwrap();
    assert_eq!(
        rewritten,
        format!("'/opt/tz/tokenzero' run -- sh -c '{command}'")
    );
}

#[test]
fn rewrite_quotes_self_exe_path() {
    let rewritten = rewrite_decision("true", "/odd path/tokenzero").unwrap();
    assert!(rewritten.starts_with("'/odd path/tokenzero' run -- sh -c"));
}

#[test]
fn rewrite_skips_unwrappable_commands() {
    let skipped = [
        "",
        "   ",
        "cd",
        "cd /tmp && ls",
        "make && cd build && ctest",
        "git clone x && cd x && npm install",
        "export FOO=1 && make",
        "cargo build; unset FOO",
        "true || alias ll='ls -la'",
        "server & sleep 1 && curl localhost",
        "make && vim notes.txt",
        "TOKENZERO_NO_WRAP=1 npm test",
        "TOKENZERO_NO_WRAP=0 npm test",
        "cat <<EOF\nbody\nEOF",
        "grep x <<< 'herestring'",
        "echo bg &",
        "sleep 5 &&",
        "vim notes.txt",
        "vi notes.txt",
        "nano notes.txt",
        "less file",
        "more file",
        "top",
        "htop",
        "ssh host uptime",
        "python -i",
        "irb",
        "psql -d db",
        "mysql -u root",
        "docker exec -it box sh",
        "git rebase -i main",
        "git add -i",
        "sudo make install",
        "echo tokenzero is great",
        "tokenzero doctor --json",
    ];
    for command in skipped {
        assert!(
            rewrite_decision(command, EXE).is_none(),
            "expected skip for {command:?}"
        );
    }
}

#[test]
fn rewrite_does_not_skip_lookalikes() {
    let wrapped = [
        "python script.py",
        "vimdiff a b",
        "cdparanoia --version",
        "cdk deploy && ls",
        "git rebase main",
        "git add -A",
        "docker exec box ls",
        "echo 'vim is fine to mention mid-command'",
        "echo 'a & b'",
        "echo \"a & b\"",
        "cargo test 2>&1",
        "cargo test 2>&1 | tail -5",
        "echo one && echo two",
        "exportfs -ra",
    ];
    for command in wrapped {
        assert!(
            rewrite_decision(command, EXE).is_some(),
            "expected rewrite for {command:?}"
        );
    }
}

#[test]
fn no_wrap_env_is_value_aware() {
    assert!(!no_wrap_enabled(None));
    assert!(no_wrap_enabled(Some("1".to_string())));
    assert!(no_wrap_enabled(Some("yes".to_string())));
    assert!(!no_wrap_enabled(Some("0".to_string())));
    assert!(!no_wrap_enabled(Some("false".to_string())));
    assert!(!no_wrap_enabled(Some("OFF".to_string())));
}

#[test]
fn top_level_scan_respects_quotes_and_redirects() {
    assert_eq!(top_level_segments("a && b; c | d").unwrap().len(), 4);
    assert!(top_level_segments("server & curl x").is_none());
    assert!(top_level_segments("echo bg &").is_none());
    assert_eq!(top_level_segments("echo 'a & b'").unwrap().len(), 1);
    assert_eq!(top_level_segments("echo \"a ; b\"").unwrap().len(), 1);
    assert_eq!(top_level_segments("cargo test 2>&1").unwrap().len(), 1);
    assert_eq!(top_level_segments("cmd &>out").unwrap().len(), 1);
    assert_eq!(top_level_segments("echo a\\&b").unwrap().len(), 1);
}

#[test]
fn decision_preserves_sibling_tool_input_keys() {
    let decision = claude_code_decision("rewrite", &bash_payload("true"), EXE, false).unwrap();
    let output = &decision["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "PreToolUse");
    assert_eq!(output["permissionDecision"], "allow");
    let updated = &output["updatedInput"];
    assert_eq!(updated["description"], "unit case");
    assert_eq!(updated["timeout"], 120000);
    assert_eq!(
        updated["command"],
        "'/opt/tz/tokenzero' run -- sh -c 'true'"
    );
}

#[test]
fn decision_passes_through_non_bash_and_malformed_payloads() {
    let non_bash = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "/tmp/x"}
    })
    .to_string();
    assert!(claude_code_decision("rewrite", &non_bash, EXE, false).is_none());
    assert!(claude_code_decision("rewrite", "{not json", EXE, false).is_none());
    assert!(claude_code_decision("rewrite", "", EXE, false).is_none());
    let missing_command = json!({"tool_name": "Bash", "tool_input": {}}).to_string();
    assert!(claude_code_decision("rewrite", &missing_command, EXE, false).is_none());
    let non_string_command =
        json!({"tool_name": "Bash", "tool_input": {"command": 42}}).to_string();
    assert!(claude_code_decision("rewrite", &non_string_command, EXE, false).is_none());
}

#[test]
fn decision_honors_no_wrap_and_off_and_unknown_modes() {
    let payload = bash_payload("true");
    assert!(claude_code_decision("rewrite", &payload, EXE, true).is_none());
    assert!(claude_code_decision("off", &payload, EXE, false).is_none());
    assert!(claude_code_decision("rewirte", &payload, EXE, false).is_none());
}

#[test]
fn guide_mode_denies_with_reason_and_no_updated_input() {
    let decision = claude_code_decision("guide", &bash_payload("true"), EXE, false).unwrap();
    let output = &decision["hookSpecificOutput"];
    assert_eq!(output["permissionDecision"], "deny");
    assert!(
        output["permissionDecisionReason"]
            .as_str()
            .unwrap()
            .contains("TokenZero")
    );
    assert!(output.get("updatedInput").is_none());
}

#[test]
fn guide_mode_passes_through_skip_cases() {
    assert!(claude_code_decision("guide", &bash_payload("cd /tmp"), EXE, false).is_none());
    assert!(claude_code_decision("guide", &bash_payload("vim x"), EXE, false).is_none());
}

fn read_payload(path: &str, extra: Value) -> String {
    let mut tool_input = json!({"file_path": path});
    if let (Some(input), Some(extra)) = (tool_input.as_object_mut(), extra.as_object()) {
        input.extend(extra.clone());
    }
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Read",
        "tool_input": tool_input,
    })
    .to_string()
}

fn large_file(dir: &tempfile::TempDir) -> String {
    let path = dir.path().join("large.txt");
    std::fs::write(&path, "x".repeat(READ_GUARD_DEFAULT_MAX_BYTES as usize + 1)).unwrap();
    path.display().to_string()
}

#[test]
fn read_guard_denies_unbounded_large_reads_in_both_modes() {
    let dir = tempfile::tempdir().unwrap();
    let path = large_file(&dir);
    for mode in ["rewrite", "guide"] {
        let decision = claude_code_decision(mode, &read_payload(&path, json!({})), EXE, false)
            .expect("large unbounded read must be denied");
        let output = &decision["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "deny");
        let reason = output["permissionDecisionReason"].as_str().unwrap();
        assert!(reason.contains("tz_read"));
        assert!(reason.contains("limit/offset"));
    }
}

#[test]
fn read_guard_passes_through_bounded_small_missing_and_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = large_file(&dir);
    // Bounded reads stay native: Edit requires a prior native Read.
    let bounded = read_payload(&path, json!({"limit": 100}));
    assert!(claude_code_decision("rewrite", &bounded, EXE, false).is_none());
    let offset_only = read_payload(&path, json!({"offset": 5000}));
    assert!(claude_code_decision("rewrite", &offset_only, EXE, false).is_none());
    // Small file, missing file, no-wrap opt-out, and off/unknown modes.
    let small = dir.path().join("small.txt");
    std::fs::write(&small, "hello").unwrap();
    let small = read_payload(&small.display().to_string(), json!({}));
    assert!(claude_code_decision("rewrite", &small, EXE, false).is_none());
    let missing = read_payload(&dir.path().join("nope").display().to_string(), json!({}));
    assert!(claude_code_decision("rewrite", &missing, EXE, false).is_none());
    let unbounded = read_payload(&path, json!({}));
    assert!(claude_code_decision("rewrite", &unbounded, EXE, true).is_none());
    assert!(claude_code_decision("off", &unbounded, EXE, false).is_none());
    assert!(claude_code_decision("rewirte", &unbounded, EXE, false).is_none());
}

#[test]
fn read_guard_threshold_parses_env_override() {
    assert_eq!(read_guard_threshold(None), READ_GUARD_DEFAULT_MAX_BYTES);
    assert_eq!(
        read_guard_threshold(Some("garbage".into())),
        READ_GUARD_DEFAULT_MAX_BYTES
    );
    assert_eq!(read_guard_threshold(Some("1024".into())), 1024);
    assert_eq!(read_guard_threshold(Some("0".into())), u64::MAX);
}

#[test]
fn session_start_restores_pack_only_after_compact_or_resume() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join(".tokenzero");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(
        cache_dir.join("recovery-cache.json"),
        json!({
            "order": ["tz://file/f1"],
            "files": {"f1": {"ref_id": "tz://file/f1", "path": "a.rs", "text": "alpha"}},
            "blobs": {}
        })
        .to_string(),
    )
    .unwrap();
    let payload = |source: &str| {
        json!({
            "hook_event_name": "SessionStart",
            "source": source,
            "cwd": dir.path().display().to_string(),
        })
        .to_string()
    };

    let decision = session_start_decision(&payload("compact"), 400).unwrap();
    let output = &decision["hookSpecificOutput"];
    assert_eq!(output["hookEventName"], "SessionStart");
    let context = output["additionalContext"].as_str().unwrap();
    assert!(context.contains("tz://file/f1"), "{context}");
    assert!(context.contains("expand"), "{context}");

    assert!(session_start_decision(&payload("resume"), 400).is_some());
    assert!(session_start_decision(&payload("startup"), 400).is_none());
    assert!(session_start_decision(&payload("clear"), 400).is_none());
    assert!(session_start_decision("{not json", 400).is_none());

    // Workspace without a recovery cache: nothing to restore, silent.
    let other = tempfile::tempdir().unwrap();
    let empty = json!({
        "hook_event_name": "SessionStart",
        "source": "compact",
        "cwd": other.path().display().to_string(),
    })
    .to_string();
    assert!(session_start_decision(&empty, 400).is_none());
}
