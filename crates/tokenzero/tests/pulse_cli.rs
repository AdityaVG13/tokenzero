use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::{Command, Output}, thread};
use tempfile::tempdir;

const PULSE_EVENT: &str = "{\"schema_version\":\"tokenzero.pulse.v1\",\"event\":\"tool_call\",\"timestamp_unix\":1,\"tool\":\"read\",\"mode\":\"hybrid\",\"raw_tokens\":100,\"visible_tokens\":20,\"recovery_tokens\":0,\"task_lossless\":true,\"cache_hit\":false,\"retry_count\":0,\"failure\":false,\"exact_ref_count\":1,\"latency_ms\":1,\"source_hash\":null}\n";

fn pulse(root: &Path, args: &[&str]) -> Output {
    let mut full = vec!["pulse", "--root", root.to_str().unwrap()];
    full.extend_from_slice(args);
    Command::cargo_bin("tokenzero").unwrap().args(&full).output().unwrap()
}

fn pulse_json(root: &Path, args: &[&str]) -> Value {
    let output = pulse(root, args);
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

fn write_event(path: &Path) { fs::write(path, PULSE_EVENT).unwrap(); }

fn assert_doctor_ok(root: &Path, event_count: Option<u64>) -> Value {
    let json = pulse_json(root, &["doctor", "--json"]);
    assert_eq!(json["ok"], true);
    assert_eq!(json["sqlite_integrity"], "ok");
    assert_eq!(json["marker_match"], true);
    if let Some(n) = event_count { assert_eq!(json["event_count"], n); }
    json
}

#[test]
fn pulse_jsonl_sqlite_commands_roundtrip() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.jsonl");
    let export = dir.path().join("export.jsonl");
    write_event(&input);
    let imported = pulse_json(dir.path(), &["import-jsonl", input.to_str().unwrap(), "--json"]);
    assert_eq!(imported["ok"], true);
    assert_eq!(imported["event_count"], 1);
    assert_eq!(assert_doctor_ok(dir.path(), None)["hot_index_used"], true);
    pulse_json(dir.path(), &["export-jsonl", export.to_str().unwrap(), "--json"]);
    assert_eq!(fs::read_to_string(export).unwrap(), fs::read_to_string(input).unwrap());
}

#[test]
fn pulse_import_rejects_corrupt_jsonl_without_replacing_ledger() {
    let dir = tempdir().unwrap();
    let good = dir.path().join("good.jsonl");
    let bad = dir.path().join("bad.jsonl");
    write_event(&good);
    fs::write(&bad, "{not valid json\n").unwrap();
    assert!(pulse(dir.path(), &["import-jsonl", good.to_str().unwrap()]).status.success());
    assert!(!pulse(dir.path(), &["import-jsonl", bad.to_str().unwrap()]).status.success());
    let report = pulse_json(dir.path(), &["--json"]);
    assert_eq!(report["event_count"], 1);
    assert_eq!(report["skipped_lines"], 0);
}

#[test]
fn pulse_import_jsonl_accepts_current_ledger_path() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.jsonl");
    write_event(&input);
    pulse_json(dir.path(), &["import-jsonl", input.to_str().unwrap(), "--json"]);
    let ledger = dir.path().join(".tokenzero/pulse/events.jsonl");
    let before = fs::read(&ledger).unwrap();
    let second = pulse_json(dir.path(), &["import-jsonl", ledger.to_str().unwrap(), "--json"]);
    assert_eq!(second["ok"], true);
    assert_eq!(second["event_count"], 1);
    assert_eq!(fs::read(&ledger).unwrap(), before);
    let doctor = assert_doctor_ok(dir.path(), None);
    assert_eq!(doctor["ok"], true);
    assert_eq!(doctor["marker_match"], true);
}

#[test]
fn pulse_sync_json_reports_held_lock_contract() {
    let dir = tempdir().unwrap();
    let pulse_dir = dir.path().join(".tokenzero/pulse");
    fs::create_dir_all(&pulse_dir).unwrap();
    let lock_path = pulse_dir.join("sync.lock");
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .open(&lock_path).unwrap();
    lock.lock().unwrap();
    let output = pulse(dir.path(), &["sync", "--json"]);
    lock.unlock().unwrap();
    assert!(!output.status.success());
    assert!(output.stderr.is_empty(), "stderr should stay empty for JSON mode: {}", String::from_utf8_lossy(&output.stderr));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.pulse.error.v1");
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert_eq!(json["operation"], "pulse sync");
    assert_eq!(json["error_kind"], "would_block");
    assert_eq!(json["retryable"], true);
    assert_eq!(json["exit_code"], 1);
    assert!(json["error"].as_str().unwrap().contains(lock_path.to_str().unwrap()));
}

#[test]
fn pulse_survives_multi_process_tool_recording_stress() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.txt");
    fs::write(&input, "pulse contention fixture\n").unwrap();
    let workers = 16;
    let handles: Vec<_> = (0..workers).map(|_| {
        let root = dir.path().to_owned();
        let input = input.clone();
        thread::spawn(move || {
            Command::cargo_bin("tokenzero").unwrap().env("TOKENZERO_ROOT", &root)
                .args(["read", "--max-visible-tokens", "100", input.to_str().unwrap()])
                .output().unwrap()
        })
    }).collect();
    for handle in handles {
        let output = handle.join().unwrap();
        assert!(output.status.success(), "stdout:\n{}\nstderr:\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    }
    let report = pulse_json(dir.path(), &["--json"]);
    assert_eq!(report["event_count"], workers);
    assert_eq!(report["skipped_lines"], 0);
    assert_doctor_ok(dir.path(), Some(workers as u64));
}
