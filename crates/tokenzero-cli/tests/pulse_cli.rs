use assert_cmd::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use std::thread;
use tempfile::tempdir;

#[test]
fn pulse_jsonl_sqlite_commands_roundtrip() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.jsonl");
    let export = dir.path().join("export.jsonl");
    std::fs::write(
        &input,
        "{\"schema_version\":\"tokenzero.pulse.v1\",\"event\":\"tool_call\",\"timestamp_unix\":1,\"tool\":\"read\",\"mode\":\"hybrid\",\"raw_tokens\":100,\"visible_tokens\":20,\"recovery_tokens\":0,\"task_lossless\":true,\"cache_hit\":false,\"retry_count\":0,\"failure\":false,\"exact_ref_count\":1,\"latency_ms\":1,\"source_hash\":null}\n",
    )
    .unwrap();

    let import = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "import-jsonl",
            input.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        import.status.success(),
        "{}",
        String::from_utf8_lossy(&import.stderr)
    );
    let imported: Value = serde_json::from_slice(&import.stdout).unwrap();
    assert_eq!(imported["ok"], true);
    assert_eq!(imported["event_count"], 1);

    let doctor = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor_json["ok"], true);
    assert_eq!(doctor_json["sqlite_integrity"], "ok");
    assert_eq!(doctor_json["marker_match"], true);
    assert_eq!(doctor_json["hot_index_used"], true);

    let exported = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "export-jsonl",
            export.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(export).unwrap(),
        std::fs::read_to_string(input).unwrap()
    );
}

#[test]
fn pulse_import_rejects_corrupt_jsonl_without_replacing_ledger() {
    let dir = tempdir().unwrap();
    let good = dir.path().join("good.jsonl");
    let bad = dir.path().join("bad.jsonl");
    std::fs::write(
        &good,
        "{\"schema_version\":\"tokenzero.pulse.v1\",\"event\":\"tool_call\",\"timestamp_unix\":1,\"tool\":\"read\",\"mode\":\"hybrid\",\"raw_tokens\":100,\"visible_tokens\":20,\"recovery_tokens\":0,\"task_lossless\":true,\"cache_hit\":false,\"retry_count\":0,\"failure\":false,\"exact_ref_count\":1,\"latency_ms\":1,\"source_hash\":null}\n",
    )
    .unwrap();
    std::fs::write(&bad, "{not valid json\n").unwrap();

    let first = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "import-jsonl",
            good.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(first.status.success());

    let second = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "import-jsonl",
            bad.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!second.status.success());

    let report = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["pulse", "--root", dir.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report_json: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report_json["event_count"], 1);
    assert_eq!(report_json["skipped_lines"], 0);
}

#[test]
fn pulse_import_jsonl_accepts_current_ledger_path() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.jsonl");
    std::fs::write(
        &input,
        "{\"schema_version\":\"tokenzero.pulse.v1\",\"event\":\"tool_call\",\"timestamp_unix\":1,\"tool\":\"read\",\"mode\":\"hybrid\",\"raw_tokens\":100,\"visible_tokens\":20,\"recovery_tokens\":0,\"task_lossless\":true,\"cache_hit\":false,\"retry_count\":0,\"failure\":false,\"exact_ref_count\":1,\"latency_ms\":1,\"source_hash\":null}\n",
    )
    .unwrap();

    let first = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "import-jsonl",
            input.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let ledger = dir.path().join(".tokenzero/pulse/events.jsonl");
    let before = std::fs::read(&ledger).unwrap();

    let second = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "import-jsonl",
            ledger.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let json: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["event_count"], 1);
    assert_eq!(std::fs::read(&ledger).unwrap(), before);

    let doctor = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor_json["ok"], true);
    assert_eq!(doctor_json["marker_match"], true);
}

#[test]
fn pulse_sync_json_reports_held_lock_contract() {
    let dir = tempdir().unwrap();
    let pulse_dir = dir.path().join(".tokenzero/pulse");
    fs::create_dir_all(&pulse_dir).unwrap();
    let lock_path = pulse_dir.join("sync.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    lock.lock().unwrap();

    let output = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "sync",
            "--json",
        ])
        .output()
        .unwrap();
    lock.unlock().unwrap();

    assert!(!output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr should stay empty for JSON mode: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], "tokenzero.pulse.error.v1");
    assert_eq!(json["ok"], false);
    assert_eq!(json["status"], "error");
    assert_eq!(json["operation"], "pulse sync");
    assert_eq!(json["error_kind"], "would_block");
    assert_eq!(json["retryable"], true);
    assert_eq!(json["exit_code"], 1);
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains(lock_path.to_str().unwrap())
    );
}

#[test]
fn pulse_survives_multi_process_tool_recording_stress() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("input.txt");
    std::fs::write(&input, "pulse contention fixture\n").unwrap();

    let workers = 16;
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let root = dir.path().to_owned();
            let input = input.clone();
            thread::spawn(move || {
                Command::cargo_bin("tokenzero")
                    .unwrap()
                    .env("TOKENZERO_ROOT", &root)
                    .args([
                        "read",
                        "--max-visible-tokens",
                        "100",
                        input.to_str().unwrap(),
                    ])
                    .output()
                    .unwrap()
            })
        })
        .collect();

    for handle in handles {
        let output = handle.join().unwrap();
        assert!(
            output.status.success(),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let report = Command::cargo_bin("tokenzero")
        .unwrap()
        .args(["pulse", "--root", dir.path().to_str().unwrap(), "--json"])
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "{}",
        String::from_utf8_lossy(&report.stderr)
    );
    let report_json: Value = serde_json::from_slice(&report.stdout).unwrap();
    assert_eq!(report_json["event_count"], workers);
    assert_eq!(report_json["skipped_lines"], 0);

    let doctor = Command::cargo_bin("tokenzero")
        .unwrap()
        .args([
            "pulse",
            "--root",
            dir.path().to_str().unwrap(),
            "doctor",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        doctor.status.success(),
        "{}",
        String::from_utf8_lossy(&doctor.stderr)
    );
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert_eq!(doctor_json["ok"], true);
    assert_eq!(doctor_json["event_count"], workers);
    assert_eq!(doctor_json["sqlite_integrity"], "ok");
    assert_eq!(doctor_json["marker_match"], true);
}
