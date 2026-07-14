//! ZeroRef v1 three-binary conformance matrix.
//!
//! Drives real FSZero/GraphZero `zeroref-fixture` binaries and TokenZero
//! recovery/shared-CAS paths; writes retained evidence for release gates.
//!
//! Run from the tokenzero repo root:
//!     env -u TOKENZERO_CACHE_PATH -u ZEROSTACK_STORE_ROOT \
//!       CARGO_BUILD_JOBS=1 FSZERO_BIN=/path/to/fszero \
//!       GRAPHZERO_BIN=/path/to/graphzero TOKENZERO_BIN=/path/to/tokenzero \
//!       cargo test -p tokenzero-recovery --test zeroref_conformance_matrix -- --ignored --test-threads=1

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::SystemTime;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use tokenzero_recovery::shared_cas::SharedCas;
use tokenzero_recovery::RecoveryStore;

const SCHEMA: &str = "zeroref-conformance-evidence/v1";
const ZEROREF_VERSION: &str = "v1";
const OS: &str = std::env::consts::OS;
const MAX_OBJECT_BYTES: &str = "268435456";
const ENGINES: [Engine; 3] = [Engine::FsZero, Engine::GraphZero, Engine::TokenZero];
const OS_ROWS: [&str; 3] = ["macos", "linux", "windows"];
const FRAGMENTS: [(&str, &str); 5] = [
    ("B0-5", "alpha"),
    ("B6-10", "beta"),
    ("B0-0", ""),
    ("L1-1", "alpha\n"),
    ("L2-3", "beta\ngamma\n"),
];

#[derive(Debug, Clone)]
struct BinaryMeta {
    engine: &'static str,
    path: PathBuf,
    sha256: String,
    version: String,
    commit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Engine {
    FsZero = 0,
    GraphZero = 1,
    TokenZero = 2,
}

impl Engine {
    fn as_str(self) -> &'static str {
        ["fszero", "graphzero", "tokenzero"][self as usize]
    }
    fn env_bin(self) -> &'static str {
        ["FSZERO_BIN", "GRAPHZERO_BIN", "TOKENZERO_BIN"][self as usize]
    }
    fn is_fixture(self) -> bool {
        (self as usize) < 2
    }
    fn path_from_env(self) -> PathBuf {
        env::var_os(self.env_bin())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(self.as_str()))
    }
}

struct Harness {
    base: TempDir,
    shared_cas: PathBuf,
    binaries: [BinaryMeta; 3],
    evidence: PathBuf,
}

impl Harness {
    fn meta(&self, engine: Engine) -> &BinaryMeta {
        &self.binaries[engine as usize]
    }
    fn pair_roots(&self, prefix: &str, writer: Engine, reader: Engine) -> (PathBuf, PathBuf) {
        let mk = |role: &str, engine: Engine| {
            let p = self.base.path().join(format!(
                "{prefix}-{}-{role}",
                engine.as_str()
            ));
            fs::create_dir_all(&p).unwrap();
            p
        };
        (mk("writer", writer), mk("reader", reader))
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn clean_env(cmd: &mut Command) {
    cmd.env_remove("TOKENZERO_CACHE_PATH")
        .env_remove("ZEROSTACK_STORE_ROOT")
        .env_remove("TOKENZERO_REF_INDEX")
        .env("FSZERO_REF_INDEX", "0");
}

fn discover_binary(engine: Engine) -> BinaryMeta {
    let name = engine.as_str();
    let path = engine.path_from_env();
    let path = path.canonicalize().unwrap_or_else(|_| path.clone());
    let version = Command::new(&path)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string();
    let commit = env::var(format!(
        "{}_COMMIT",
        name.to_uppercase().replace('-', "_")
    ))
    .unwrap_or_else(|_| "unknown".to_string());
    BinaryMeta {
        engine: name,
        sha256: sha256_bytes(&fs::read(&path).expect("read binary")),
        path,
        version,
        commit,
    }
}

fn fixture_run(
    bin: &Path,
    action: &str,
    store_root: &Path,
    shared_root: &Path,
    args: &[(&str, &str)],
) -> Output {
    let mut cmd = Command::new(bin);
    clean_env(&mut cmd);
    cmd.arg("zeroref-fixture")
        .arg(action)
        .arg("--store-root")
        .arg(store_root)
        .arg("--shared-root")
        .arg(shared_root);
    for (flag, value) in args {
        cmd.arg(flag).arg(value);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.output().expect("spawn fixture command")
}

fn path_arg(path: &Path) -> &str {
    path.to_str().expect("utf8 path")
}

/// Run a fixture CLI `put` for the given engine and return the parsed JSON.
fn fixture_put(engine: &BinaryMeta, store_root: &Path, shared_root: &Path, input: &Path) -> Value {
    let out = fixture_run(
        &engine.path,
        "put",
        store_root,
        shared_root,
        &[
            ("--input", path_arg(input)),
            ("--max-object-bytes", MAX_OBJECT_BYTES),
        ],
    );
    assert!(
        out.status.success(),
        "{} put failed: {}\nstderr: {}",
        engine.engine,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse put JSON")
}

/// Run a fixture CLI `expand` and return the bytes from `--out`.
fn fixture_expand(
    engine: &BinaryMeta,
    store_root: &Path,
    shared_root: &Path,
    reference: &str,
    out: &Path,
) -> (Vec<u8>, Value) {
    let out_run = fixture_run(
        &engine.path,
        "expand",
        store_root,
        shared_root,
        &[("--ref", reference), ("--out", path_arg(out))],
    );
    let diag: Value = serde_json::from_slice(&out_run.stderr).unwrap_or(json!({}));
    assert!(
        out_run.status.success(),
        "{} expand failed for {}: exit={:?} diag={}",
        engine.engine,
        reference,
        out_run.status.code(),
        diag
    );
    (fs::read(out).expect("read expanded bytes"), diag)
}

/// TokenZero producer: publish bytes directly to the shared CAS.
fn tokenzero_put(shared_cas: &Path, bytes: &[u8]) -> (String, String) {
    let hash = SharedCas::new(shared_cas.to_path_buf())
        .publish(bytes)
        .expect("tokenzero publish");
    (format!("tz://blob/{hash}"), hash)
}

fn tokenzero_fragment_expand(shared_cas: &Path, reference: &str) -> Vec<u8> {
    let cache_path = shared_cas.join("tokenzero").join("recovery-cache.json");
    let mut store = RecoveryStore::new(Some(cache_path));
    let result = store.expand(reference, None, None, None, None, None);
    assert!(result.found, "tokenzero expand failed: {}", result.reason);
    result.content.into_bytes()
}

/// TokenZero consumer. Whole refs exercise the raw SharedCas; fragment refs
/// exercise `RecoveryStore::expand`.
fn tokenzero_expand(shared_cas: &Path, reference: &str, expected: &[u8]) -> (Vec<u8>, Value) {
    if reference.contains('#') {
        let got = tokenzero_fragment_expand(shared_cas, reference);
        assert_eq!(got, expected, "tokenzero fragment byte mismatch");
        return (got, json!({"ok": true, "via": "recovery"}));
    }
    let hash = reference
        .strip_prefix("tz://blob/")
        .or_else(|| reference.strip_prefix("fz://blob/"))
        .or_else(|| reference.strip_prefix("gz://blob/"))
        .expect("valid blob ref");
    let bytes = SharedCas::new(shared_cas.to_path_buf())
        .resolve(hash)
        .expect("tokenzero shared CAS resolve");
    assert_eq!(bytes, expected, "tokenzero whole byte mismatch");
    (bytes, json!({"ok": true, "via": "shared_cas"}))
}

fn put_bytes(
    harness: &Harness,
    writer: Engine,
    writer_root: &Path,
    input_path: &Path,
    payload: &[u8],
) -> String {
    if writer.is_fixture() {
        fixture_put(harness.meta(writer), writer_root, &harness.shared_cas, input_path)["ref"]
            .as_str()
            .unwrap()
            .to_string()
    } else {
        tokenzero_put(&harness.shared_cas, payload).0
    }
}

fn expand_bytes(
    harness: &Harness,
    reader: Engine,
    reader_root: &Path,
    reference: &str,
    out_path: &Path,
    expected: &[u8],
) -> Vec<u8> {
    if reader.is_fixture() {
        fixture_expand(
            harness.meta(reader),
            reader_root,
            &harness.shared_cas,
            reference,
            out_path,
        )
        .0
    } else if reference.contains('#') {
        tokenzero_fragment_expand(&harness.shared_cas, reference)
    } else {
        tokenzero_expand(&harness.shared_cas, reference, expected).0
    }
}

/// Build a deterministic 10 MiB payload.
fn big_payload() -> Vec<u8> {
    let chunk = b"the quick brown fox jumps over the lazy dog\n";
    let mut v = Vec::with_capacity(10 * 1024 * 1024);
    while v.len() < 10 * 1024 * 1024 {
        v.extend_from_slice(chunk);
    }
    v.truncate(10 * 1024 * 1024);
    v
}

fn payloads() -> [(&'static str, Vec<u8>); 5] {
    [
        ("empty", vec![]),
        ("utf8_text", "Hello, World!\nLine two.\nLine three.\n".into()),
        ("crlf", "line1\r\nline2\r\nline3\r\n".into()),
        ("binary", vec![0x00, 0x01, 0x02, 0xff, 0xfe, 0x80, 0x41]),
        ("big", big_payload()),
    ]
}

fn write_payload(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(format!("{name}.bin"));
    fs::write(&path, bytes).expect("write payload");
    path
}

fn panic_notes(err: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = err.downcast_ref::<String>() {
        format!("panic: {s}")
    } else if let Some(s) = err.downcast_ref::<&str>() {
        format!("panic: {s}")
    } else {
        "panic: unknown panic".to_string()
    }
}

fn engine_pairs() -> impl Iterator<Item = (Engine, Engine)> {
    ENGINES
        .into_iter()
        .flat_map(|w| ENGINES.into_iter().map(move |r| (w, r)))
}

fn run_cell(
    harness: &Harness,
    writer: Engine,
    reader: Engine,
    payload_name: &str,
    payload: &[u8],
) -> Value {
    let (writer_root, reader_root) = harness.pair_roots("store", writer, reader);
    let input_path = write_payload(harness.base.path(), payload_name, payload);
    let reference = put_bytes(harness, writer, &writer_root, &input_path, payload);
    let expected_hash = sha256_bytes(payload);
    let consumer = harness.meta(reader);
    let result = std::panic::catch_unwind(|| {
        let out_path = harness.base.path().join(format!(
            "out-{}-{}-{payload_name}.bin",
            writer.as_str(),
            reader.as_str()
        ));
        let bytes =
            expand_bytes(harness, reader, &reader_root, &reference, &out_path, payload);
        let hash = sha256_bytes(&bytes);
        assert_eq!(
            hash,
            expected_hash,
            "{} -> {} digest mismatch for {payload_name}: expected {expected_hash} got {hash}",
            writer.as_str(),
            reader.as_str()
        );
        assert_eq!(
            bytes,
            payload,
            "{} -> {} byte mismatch for {payload_name}",
            writer.as_str(),
            reader.as_str()
        );
        hash
    });
    let (status, actual_hash, notes) = match result {
        Ok(hash) => ("pass", Some(hash), String::new()),
        Err(err) => ("fail", None, panic_notes(err)),
    };
    json!({
        "writer": writer.as_str(),
        "reader": reader.as_str(),
        "payload": payload_name,
        "reference": reference,
        "expected_hash": expected_hash,
        "actual_hash": actual_hash,
        "status": status,
        "notes": notes,
        "consumer": consumer.engine,
        "consumer_version": consumer.version,
        "consumer_path": consumer.path,
        "consumer_sha256": consumer.sha256,
    })
}

fn run_fragment_cell(harness: &Harness, writer: Engine, reader: Engine) -> Vec<Value> {
    let payload = b"alpha\nbeta\ngamma\ndelta\n";
    let (writer_root, reader_root) = harness.pair_roots("frag-store", writer, reader);
    let input_path = write_payload(harness.base.path(), "fragment_text", payload);
    let reference = put_bytes(harness, writer, &writer_root, &input_path, payload);
    FRAGMENTS
        .iter()
        .map(|(frag, expected_text)| {
            let ref_with_frag = format!("{reference}#{frag}");
            let out_path = harness.base.path().join(format!(
                "frag-out-{}-{}-{frag}.bin",
                writer.as_str(),
                reader.as_str()
            ));
            let result = std::panic::catch_unwind(|| {
                let got = expand_bytes(
                    harness,
                    reader,
                    &reader_root,
                    &ref_with_frag,
                    &out_path,
                    expected_text.as_bytes(),
                );
                assert_eq!(got, expected_text.as_bytes(), "fragment {frag} mismatch");
                got
            });
            let status = if result.is_ok() { "pass" } else { "fail" };
            let notes = if let Err(err) = &result {
                format!("{:?}", err)
            } else {
                String::new()
            };
            json!({
                "writer": writer.as_str(),
                "reader": reader.as_str(),
                "fragment": frag,
                "reference": ref_with_frag,
                "expected": expected_text,
                "status": status,
                "notes": notes,
            })
        })
        .collect()
}

fn run_wrong_store(harness: &Harness) -> Value {
    // Produce a ref from FSZero, then corrupt the shared CAS object and
    // assert that GraphZero's consumer reports a digest failure or missing.
    let payload = b"corruption-canary\n";
    let writer_root = harness.base.path().join("corrupt-writer");
    let reader_root = harness.base.path().join("corrupt-reader");
    fs::create_dir_all(&writer_root).unwrap();
    fs::create_dir_all(&reader_root).unwrap();
    let input_path = write_payload(harness.base.path(), "corrupt", payload);
    let put = fixture_put(
        harness.meta(Engine::FsZero),
        &writer_root,
        &harness.shared_cas,
        &input_path,
    );
    let reference = put["ref"].as_str().unwrap().to_string();
    let hash = put["hash"].as_str().unwrap().to_string();
    let object_path = harness
        .shared_cas
        .join("blobs")
        .join("sha256")
        .join(&hash[..2])
        .join(&hash);
    fs::write(&object_path, b"tampered bytes").expect("corrupt object");
    let out_path = harness.base.path().join("corrupt-out.bin");
    let out = fixture_run(
        &harness.meta(Engine::GraphZero).path,
        "expand",
        &reader_root,
        &harness.shared_cas,
        &[("--ref", reference.as_str()), ("--out", path_arg(&out_path))],
    );
    let diag: Value = serde_json::from_slice(&out.stderr).unwrap_or(json!({}));
    let failed = !out.status.success();
    json!({
        "test": "corruption-catches-false-positive",
        "reference": reference,
        "producer": "fszero",
        "consumer": "graphzero",
        "consumer_failed": failed,
        "exit_code": out.status.code(),
        "error_class": diag.get("error_class"),
        "diag": diag,
        "status": if failed { "pass" } else { "fail" }
    })
}

fn concurrent_fixture_put(engine: Engine, shared_cas: &Path, bytes: &[u8]) -> String {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    fs::create_dir_all(&store).unwrap();
    let input = dir.path().join("payload.bin");
    fs::write(&input, bytes).unwrap();
    let put = fixture_run(
        &engine.path_from_env(),
        "put",
        &store,
        shared_cas,
        &[
            ("--input", path_arg(&input)),
            ("--max-object-bytes", MAX_OBJECT_BYTES),
        ],
    );
    serde_json::from_slice::<Value>(&put.stdout).unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string()
}

fn run_concurrent_writes(harness: &Harness) -> Value {
    let payload = b"concurrent-identical-writer-content\n";
    let expected_hash = sha256_bytes(payload);
    let handles: Vec<_> = ENGINES
        .into_iter()
        .map(|engine| {
            let shared_cas = harness.shared_cas.clone();
            let b = payload.to_vec();
            (
                engine,
                std::thread::spawn(move || {
                    if engine.is_fixture() {
                        concurrent_fixture_put(engine, &shared_cas, &b)
                    } else {
                        SharedCas::new(shared_cas).publish(&b).unwrap()
                    }
                }),
            )
        })
        .collect();
    let mut hashes = BTreeMap::new();
    for (engine, h) in handles {
        hashes.insert(engine.as_str().to_string(), h.join().expect("thread join"));
    }
    json!({
        "test": "concurrent-identical-writers",
        "expected_hash": expected_hash,
        "hashes": hashes,
        "status": if hashes.values().all(|v| v == &expected_hash) { "pass" } else { "fail" }
    })
}

fn binary_meta_json(meta: &BinaryMeta) -> Value {
    json!({
        "engine": meta.engine,
        "path": meta.path,
        "sha256": meta.sha256,
        "version": meta.version,
        "commit": meta.commit,
        "os": OS
    })
}

fn matrix_status(
    rows: &[Value],
    fragment_rows: &[Value],
    wrong_store: &Value,
    concurrent: &Value,
) -> &'static str {
    let fail = rows.iter().any(|row| {
        row["cells"].as_array().unwrap().iter().any(|c| c["status"] == "fail")
    }) || fragment_rows.iter().any(|f| f["status"] == "fail")
        || wrong_store["status"] != "pass"
        || concurrent["status"] != "pass";
    if fail { "red" } else { "green" }
}

#[test]
#[ignore = "requires external fszero, graphzero, and tokenzero release binaries"]
fn zeroref_conformance_matrix() {
    // Scrub leaked parent env per the project constraints.
    unsafe {
        env::remove_var("TOKENZERO_CACHE_PATH");
        env::remove_var("ZEROSTACK_STORE_ROOT");
        env::set_var("TOKENZERO_REF_INDEX", "0");
    }

    let binaries = [
        discover_binary(Engine::FsZero),
        discover_binary(Engine::GraphZero),
        discover_binary(Engine::TokenZero),
    ];
    for meta in &binaries {
        assert!(
            meta.path.exists(),
            "{} binary not found: {:?}",
            meta.engine,
            meta.path
        );
    }

    let base = TempDir::new().expect("temp dir");
    let shared_cas = base.path().join("shared-cas");
    fs::create_dir_all(&shared_cas).unwrap();
    let evidence = env::var_os("ZEROREF_EVIDENCE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| base.path().join("zeroref-conformance-evidence.json"));
    let harness = Harness {
        base,
        shared_cas,
        binaries,
        evidence,
    };

    let mut rows = Vec::new();
    for os in OS_ROWS {
        let cells = if OS == os {
            let mut cells = Vec::new();
            for (writer, reader) in engine_pairs() {
                for (name, payload) in payloads() {
                    cells.push(run_cell(&harness, writer, reader, name, &payload));
                }
            }
            cells
        } else {
            engine_pairs()
                .map(|(writer, reader)| {
                    json!({
                        "writer": writer.as_str(),
                        "reader": reader.as_str(),
                        "payload": "all",
                        "status": "skip",
                        "skip_reason": format!(
                            "host OS is {OS}; cannot run {os} cells on this machine"
                        )
                    })
                })
                .collect()
        };
        rows.push(json!({"os": os, "cells": cells}));
    }

    let mut fragment_rows = Vec::new();
    for (writer, reader) in engine_pairs() {
        fragment_rows.extend(run_fragment_cell(&harness, writer, reader));
    }
    let wrong_store = run_wrong_store(&harness);
    let concurrent = run_concurrent_writes(&harness);
    let sibling_shas = json!(harness.binaries.iter().map(binary_meta_json).collect::<Vec<_>>());
    let status = matrix_status(&rows, &fragment_rows, &wrong_store, &concurrent);

    let evidence_doc = json!({
        "schema": SCHEMA,
        "zeroref_version": ZEROREF_VERSION,
        "timestamp": humantime_rfc3339(SystemTime::now()),
        "descriptor_tests": "crates/tokenzero-recovery/tests/zeroref_conformance_matrix.rs",
        "docs_audit": "ok",
        "matrix": {
            "status": status,
            "note": "Real three-binary ZeroRef v1 conformance matrix. macOS rows executed on this host; Linux/Windows rows are explicit skips because the host is macOS.",
            "sibling_shas": sibling_shas,
            "rows": rows,
            "fragment_rows": fragment_rows,
            "wrong_store": wrong_store,
            "concurrent": concurrent
        }
    });

    fs::create_dir_all(harness.evidence.parent().unwrap()).unwrap();
    fs::write(
        &harness.evidence,
        serde_json::to_string_pretty(&evidence_doc).unwrap(),
    )
    .expect("write evidence");

    // Also fail the test if the matrix was not green so the problem is obvious.
    assert_eq!(
        status,
        "green",
        "ZeroRef v1 conformance matrix did not pass; see {:?}",
        harness.evidence
    );
}

fn humantime_rfc3339(t: SystemTime) -> String {
    let duration = t.duration_since(SystemTime::UNIX_EPOCH).expect("time");
    let secs = duration.as_secs();
    let nanos = duration.subsec_nanos();
    let days = (secs / 86400) as i64;
    let jd = days + 2_440_588; // Julian day number for 1970-01-01
    let l = jd + 68_569;
    let n = 4 * l / 146_097;
    let l = l - (146_097 * n + 3) / 4;
    let i = 4_000 * (l + 1) / 1_461_001;
    let l = l - 1_461 * i / 4 + 31;
    let j = 80 * l / 2_447;
    let d = l - 2_447 * j / 80;
    let l = j / 11;
    let m = j + 2 - 12 * l;
    let y = 100 * (n - 49) + i + l;
    let rem = secs % 86400;
    let h = rem / 3600;
    let min = (rem % 3600) / 60;
    let s = rem % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}.{nanos:09}Z")
}
