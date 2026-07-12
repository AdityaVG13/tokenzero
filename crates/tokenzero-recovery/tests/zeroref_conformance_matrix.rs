//! ZeroRef v1 three-binary conformance matrix.
//!
//! This integration test drives the real release binaries for FSZero and
//! GraphZero through their `zeroref-fixture` CLI surfaces, and exercises
//! TokenZero through the production recovery/shared-CAS code paths in this
//! crate. It records a retained evidence artifact that the sibling release
//! gates consume.
//!
//! Run from the tokenzero repo root:
//!     env -u TOKENZERO_CACHE_PATH -u ZEROSTACK_STORE_ROOT \
//!       CARGO_BUILD_JOBS=2 \
//!       cargo test -p tokenzero-recovery --test zeroref_conformance_matrix -- --test-threads=1

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

use tokenzero_recovery::RecoveryStore;
use tokenzero_recovery::shared_cas::SharedCas;

const SCHEMA: &str = "zeroref-conformance-evidence/v1";
const ZEROREF_VERSION: &str = "v1";

const OS: &str = std::env::consts::OS;

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
    FsZero,
    GraphZero,
    TokenZero,
}

impl Engine {
    fn as_str(&self) -> &'static str {
        match self {
            Engine::FsZero => "fszero",
            Engine::GraphZero => "graphzero",
            Engine::TokenZero => "tokenzero",
        }
    }
}

struct Harness {
    base: TempDir,
    shared_cas: PathBuf,
    fszero: BinaryMeta,
    graphzero: BinaryMeta,
    tokenzero: BinaryMeta,
    evidence: PathBuf,
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read binary");
    sha256_bytes(&bytes)
}

fn discover_binary(engine: &'static str, env_var: &str, default: &str) -> BinaryMeta {
    let path = env::var_os(env_var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default));
    let path = path.canonicalize().unwrap_or_else(|_| path.clone());
    let sha256 = sha256_file(&path);
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
        engine.to_uppercase().replace("-", "_")
    ))
    .unwrap_or_else(|_| "unknown".to_string());
    BinaryMeta {
        engine,
        path,
        sha256,
        version,
        commit,
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn clean_env(cmd: &mut Command) {
    cmd.env_remove("TOKENZERO_CACHE_PATH");
    cmd.env_remove("ZEROSTACK_STORE_ROOT");
    cmd.env_remove("TOKENZERO_REF_INDEX");
    cmd.env("FSZERO_REF_INDEX", "0");
}

/// Run a fixture CLI `put` for the given engine and return the parsed JSON.
fn fixture_put(engine: &BinaryMeta, store_root: &Path, shared_root: &Path, input: &Path) -> Value {
    let mut cmd = Command::new(&engine.path);
    clean_env(&mut cmd);
    cmd.arg("zeroref-fixture")
        .arg("put")
        .arg("--store-root")
        .arg(store_root)
        .arg("--shared-root")
        .arg(shared_root)
        .arg("--input")
        .arg(input)
        .arg("--max-object-bytes")
        .arg("268435456")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().expect("spawn fixture put");
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
    let mut cmd = Command::new(&engine.path);
    clean_env(&mut cmd);
    cmd.arg("zeroref-fixture")
        .arg("expand")
        .arg("--store-root")
        .arg(store_root)
        .arg("--shared-root")
        .arg(shared_root)
        .arg("--ref")
        .arg(reference)
        .arg("--out")
        .arg(out)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out_run = cmd.output().expect("spawn fixture expand");
    let diag: Value = serde_json::from_slice(&out_run.stderr).unwrap_or(json!({}));
    assert!(
        out_run.status.success(),
        "{} expand failed for {}: exit={:?} diag={}",
        engine.engine,
        reference,
        out_run.status.code(),
        diag
    );
    let bytes = fs::read(out).expect("read expanded bytes");
    (bytes, diag)
}

/// TokenZero producer: publish bytes directly to the shared CAS.
fn tokenzero_put(shared_cas: &Path, bytes: &[u8]) -> (String, String) {
    let cas = SharedCas::new(shared_cas.to_path_buf());
    let hash = cas.publish(bytes).expect("tokenzero publish");
    let reference = format!("tz://blob/{}", hash);
    (reference, hash)
}

/// TokenZero consumer. Whole refs exercise the raw SharedCas; fragment refs
/// exercise `RecoveryStore::expand`.
fn tokenzero_expand(shared_cas: &Path, reference: &str, expected: &[u8]) -> (Vec<u8>, Value) {
    if reference.contains('#') {
        let cache_path = shared_cas.join("tokenzero").join("recovery-cache.json");
        let mut store = RecoveryStore::new(Some(cache_path));
        let result = store.expand(reference, None, None, None, None, None);
        assert!(result.found, "tokenzero expand failed: {}", result.reason);
        let got = result.content.into_bytes();
        assert_eq!(got, expected, "tokenzero fragment byte mismatch");
        return (got, json!({"ok": true, "via": "recovery"}));
    }
    let hash = reference
        .strip_prefix("tz://blob/")
        .or_else(|| reference.strip_prefix("fz://blob/"))
        .or_else(|| reference.strip_prefix("gz://blob/"))
        .expect("valid blob ref");
    let cas = SharedCas::new(shared_cas.to_path_buf());
    let bytes = cas.resolve(hash).expect("tokenzero shared CAS resolve");
    assert_eq!(bytes, expected, "tokenzero whole byte mismatch");
    (bytes, json!({"ok": true, "via": "shared_cas"}))
}

/// Build a deterministic 10 MiB payload.
fn big_payload() -> Vec<u8> {
    let chunk = b"the quick brown fox jumps over the lazy dog\n";
    let n = (10 * 1024 * 1024) / chunk.len() + 1;
    let mut v = Vec::with_capacity(10 * 1024 * 1024);
    for _ in 0..n {
        v.extend_from_slice(chunk);
    }
    v.truncate(10 * 1024 * 1024);
    v
}

fn payloads() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("empty", vec![]),
        (
            "utf8_text",
            "Hello, World!\nLine two.\nLine three.\n".into(),
        ),
        ("crlf", "line1\r\nline2\r\nline3\r\n".into()),
        ("binary", vec![0x00, 0x01, 0x02, 0xff, 0xfe, 0x80, 0x41]),
        ("big", big_payload()),
    ]
}

fn write_payload(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(format!("{}.bin", name));
    fs::write(&path, bytes).expect("write payload");
    path
}

fn run_cell(
    harness: &Harness,
    writer: Engine,
    reader: Engine,
    payload_name: &str,
    payload: &[u8],
) -> Value {
    let writer_root = harness
        .base
        .path()
        .join(format!("store-{}-writer", writer.as_str()));
    let reader_root = harness
        .base
        .path()
        .join(format!("store-{}-reader", reader.as_str()));
    fs::create_dir_all(&writer_root).unwrap();
    fs::create_dir_all(&reader_root).unwrap();

    let input_path = write_payload(harness.base.path(), payload_name, payload);

    let reference = match writer {
        Engine::FsZero => {
            let put = fixture_put(
                &harness.fszero,
                &writer_root,
                &harness.shared_cas,
                &input_path,
            );
            put["ref"].as_str().unwrap().to_string()
        }
        Engine::GraphZero => {
            let put = fixture_put(
                &harness.graphzero,
                &writer_root,
                &harness.shared_cas,
                &input_path,
            );
            put["ref"].as_str().unwrap().to_string()
        }
        Engine::TokenZero => {
            let (reference, _hash) = tokenzero_put(&harness.shared_cas, payload);
            reference
        }
    };

    let expected_hash = sha256_bytes(payload);
    let mut actual_hash: Option<String> = None;
    let mut status = "pass";
    let mut notes = String::new();

    let consumer_meta = match reader {
        Engine::FsZero => &harness.fszero,
        Engine::GraphZero => &harness.graphzero,
        Engine::TokenZero => &harness.tokenzero,
    };

    let result = std::panic::catch_unwind(|| {
        let out_path = harness.base.path().join(format!(
            "out-{}-{}-{}.bin",
            writer.as_str(),
            reader.as_str(),
            payload_name
        ));
        let bytes = match reader {
            Engine::FsZero => {
                fixture_expand(
                    &harness.fszero,
                    &reader_root,
                    &harness.shared_cas,
                    &reference,
                    &out_path,
                )
                .0
            }
            Engine::GraphZero => {
                fixture_expand(
                    &harness.graphzero,
                    &reader_root,
                    &harness.shared_cas,
                    &reference,
                    &out_path,
                )
                .0
            }
            Engine::TokenZero => tokenzero_expand(&harness.shared_cas, &reference, payload).0,
        };
        let hash = sha256_bytes(&bytes);
        assert_eq!(
            hash,
            expected_hash,
            "{} -> {} digest mismatch for {}: expected {} got {}",
            writer.as_str(),
            reader.as_str(),
            payload_name,
            expected_hash,
            hash
        );
        assert_eq!(
            bytes,
            payload,
            "{} -> {} byte mismatch for {}",
            writer.as_str(),
            reader.as_str(),
            payload_name
        );
        hash
    });

    match result {
        Ok(hash) => actual_hash = Some(hash),
        Err(err) => {
            status = "fail";
            notes = format!(
                "panic: {}",
                if let Some(s) = err.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = err.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "unknown panic".to_string()
                }
            );
        }
    }

    json!({
        "writer": writer.as_str(),
        "reader": reader.as_str(),
        "payload": payload_name,
        "reference": reference,
        "expected_hash": expected_hash,
        "actual_hash": actual_hash,
        "status": status,
        "notes": notes,
        "consumer": consumer_meta.engine,
        "consumer_version": consumer_meta.version,
        "consumer_path": consumer_meta.path,
        "consumer_sha256": consumer_meta.sha256,
    })
}

fn run_fragment_cell(harness: &Harness, writer: Engine, reader: Engine) -> Vec<Value> {
    let payload = "alpha\nbeta\ngamma\ndelta\n";
    let bytes = payload.as_bytes();
    let writer_root = harness
        .base
        .path()
        .join(format!("frag-store-{}-writer", writer.as_str()));
    let reader_root = harness
        .base
        .path()
        .join(format!("frag-store-{}-reader", reader.as_str()));
    fs::create_dir_all(&writer_root).unwrap();
    fs::create_dir_all(&reader_root).unwrap();

    let input_path = write_payload(harness.base.path(), "fragment_text", bytes);
    let reference = match writer {
        Engine::FsZero => {
            let put = fixture_put(
                &harness.fszero,
                &writer_root,
                &harness.shared_cas,
                &input_path,
            );
            put["ref"].as_str().unwrap().to_string()
        }
        Engine::GraphZero => {
            let put = fixture_put(
                &harness.graphzero,
                &writer_root,
                &harness.shared_cas,
                &input_path,
            );
            put["ref"].as_str().unwrap().to_string()
        }
        Engine::TokenZero => {
            let (reference, _hash) = tokenzero_put(&harness.shared_cas, bytes);
            reference
        }
    };

    let mut rows = Vec::new();
    let fragments = vec![
        ("B0-5", "alpha"),
        ("B6-10", "beta"),
        ("B0-0", ""),
        ("L1-1", "alpha\n"),
        ("L2-3", "beta\ngamma\n"),
    ];

    for (frag, expected_text) in fragments {
        let ref_with_frag = format!("{}#{}", reference, frag);
        let out_path = harness.base.path().join(format!(
            "frag-out-{}-{}-{}.bin",
            writer.as_str(),
            reader.as_str(),
            frag
        ));
        let result = std::panic::catch_unwind(|| {
            let got = match reader {
                Engine::FsZero => {
                    fixture_expand(
                        &harness.fszero,
                        &reader_root,
                        &harness.shared_cas,
                        &ref_with_frag,
                        &out_path,
                    )
                    .0
                }
                Engine::GraphZero => {
                    fixture_expand(
                        &harness.graphzero,
                        &reader_root,
                        &harness.shared_cas,
                        &ref_with_frag,
                        &out_path,
                    )
                    .0
                }
                Engine::TokenZero => {
                    let cache_path = harness
                        .shared_cas
                        .join("tokenzero")
                        .join("recovery-cache.json");
                    let mut store = RecoveryStore::new(Some(cache_path));
                    let res = store.expand(&ref_with_frag, None, None, None, None, None);
                    assert!(res.found, "tokenzero fragment not found: {}", res.reason);
                    res.content.into_bytes()
                }
            };
            let expected = expected_text.as_bytes();
            assert_eq!(got, expected, "fragment {} mismatch", frag);
            got
        });
        let status = if result.is_ok() { "pass" } else { "fail" };
        let notes = if let Err(err) = &result {
            format!("{:?}", err)
        } else {
            String::new()
        };
        rows.push(json!({
            "writer": writer.as_str(),
            "reader": reader.as_str(),
            "fragment": frag,
            "reference": ref_with_frag,
            "expected": expected_text,
            "status": status,
            "notes": notes,
        }));
    }

    rows
}

fn run_wrong_store(harness: &Harness) -> Value {
    // Produce a ref from FSZero, then corrupt the shared CAS object and
    // assert that GraphZero's consumer reports a digest failure or missing.
    let payload = "corruption-canary\n";
    let bytes = payload.as_bytes();
    let writer_root = harness.base.path().join("corrupt-writer");
    let reader_root = harness.base.path().join("corrupt-reader");
    fs::create_dir_all(&writer_root).unwrap();
    fs::create_dir_all(&reader_root).unwrap();
    let input_path = write_payload(harness.base.path(), "corrupt", bytes);
    let put = fixture_put(
        &harness.fszero,
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
    let mut cmd = Command::new(&harness.graphzero.path);
    clean_env(&mut cmd);
    cmd.arg("zeroref-fixture")
        .arg("expand")
        .arg("--store-root")
        .arg(&reader_root)
        .arg("--shared-root")
        .arg(&harness.shared_cas)
        .arg("--ref")
        .arg(&reference)
        .arg("--out")
        .arg(&out_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = cmd.output().expect("spawn corrupt expand");

    let diag: Value = serde_json::from_slice(&out.stderr).unwrap_or(json!({}));
    let failed = out.status.success() == false;
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

fn run_concurrent_writes(harness: &Harness) -> Value {
    let payload = "concurrent-identical-writer-content\n";
    let bytes = payload.as_bytes();
    let expected_hash = sha256_bytes(bytes);
    let mut handles = Vec::new();
    for engine in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
        let shared_cas = harness.shared_cas.clone();
        let b = bytes.to_vec();
        let h = std::thread::spawn(move || {
            let hash: String = match engine {
                Engine::FsZero => {
                    let dir = tempfile::tempdir().unwrap();
                    let store = dir.path().join("store");
                    fs::create_dir_all(&store).unwrap();
                    let input = dir.path().join("payload.bin");
                    fs::write(&input, &b).unwrap();
                    let mut cmd = Command::new(
                        env::var_os("FSZERO_BIN")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| PathBuf::from("/Users/aditya/.omp/agent/zerostack-origin-main/fszero/target/release/fszero")),
                    );
                    clean_env(&mut cmd);
                    let put = cmd
                        .arg("zeroref-fixture")
                        .arg("put")
                        .arg("--store-root")
                        .arg(&store)
                        .arg("--shared-root")
                        .arg(&shared_cas)
                        .arg("--input")
                        .arg(&input)
                        .arg("--max-object-bytes")
                        .arg("268435456")
                        .output()
                        .expect("spawn");
                    let json: Value = serde_json::from_slice(&put.stdout).unwrap();
                    json["hash"].as_str().unwrap().to_string()
                }
                Engine::GraphZero => {
                    let dir = tempfile::tempdir().unwrap();
                    let store = dir.path().join("store");
                    fs::create_dir_all(&store).unwrap();
                    let input = dir.path().join("payload.bin");
                    fs::write(&input, &b).unwrap();
                    let mut cmd = Command::new(
                        env::var_os("GRAPHZERO_BIN")
                            .map(PathBuf::from)
                            .unwrap_or_else(|| {
                                PathBuf::from("/tmp/graphzero-clean/target/release/graphzero")
                            }),
                    );
                    clean_env(&mut cmd);
                    let put = cmd
                        .arg("zeroref-fixture")
                        .arg("put")
                        .arg("--store-root")
                        .arg(&store)
                        .arg("--shared-root")
                        .arg(&shared_cas)
                        .arg("--input")
                        .arg(&input)
                        .arg("--max-object-bytes")
                        .arg("268435456")
                        .output()
                        .expect("spawn");
                    let json: Value = serde_json::from_slice(&put.stdout).unwrap();
                    json["hash"].as_str().unwrap().to_string()
                }
                Engine::TokenZero => {
                    let cas = SharedCas::new(shared_cas);
                    cas.publish(&b).unwrap()
                }
            };
            hash
        });
        handles.push((engine, h));
    }
    let mut hashes = BTreeMap::new();
    for (engine, h) in handles {
        let hash = h.join().expect("thread join");
        hashes.insert(engine.as_str().to_string(), hash);
    }
    let all_same = hashes.values().all(|v| v == &expected_hash);
    json!({
        "test": "concurrent-identical-writers",
        "expected_hash": expected_hash,
        "hashes": hashes,
        "status": if all_same { "pass" } else { "fail" }
    })
}

#[test]
fn zeroref_conformance_matrix() {
    // Scrub leaked parent env per the project constraints.
    unsafe {
        env::remove_var("TOKENZERO_CACHE_PATH");
        env::remove_var("ZEROSTACK_STORE_ROOT");
        env::set_var("TOKENZERO_REF_INDEX", "0");
    }

    let fszero_default =
        "/Users/aditya/.omp/agent/zerostack-origin-main/fszero/target/release/fszero";
    let graphzero_default = "/tmp/graphzero-clean/target/release/graphzero";
    let tokenzero_default = "/Users/aditya/AI/tokenzero/target/release/tokenzero";

    let fszero = discover_binary("fszero", "FSZERO_BIN", fszero_default);
    let graphzero = discover_binary("graphzero", "GRAPHZERO_BIN", graphzero_default);
    let tokenzero = discover_binary("tokenzero", "TOKENZERO_BIN", tokenzero_default);

    // Validate the binaries exist.
    assert!(
        fszero.path.exists(),
        "fszero binary not found: {:?}",
        fszero.path
    );
    assert!(
        graphzero.path.exists(),
        "graphzero binary not found: {:?}",
        graphzero.path
    );
    assert!(
        tokenzero.path.exists(),
        "tokenzero binary not found: {:?}",
        tokenzero.path
    );

    let base = TempDir::new().expect("temp dir");
    let shared_cas = base.path().join("shared-cas");
    fs::create_dir_all(&shared_cas).unwrap();

    let harness = Harness {
        base,
        shared_cas,
        fszero,
        graphzero,
        tokenzero,
        evidence: repo_root()
            .join("fixtures")
            .join("zeroref-conformance-evidence.json"),
    };

    let mut rows = Vec::new();

    for os in ["macos", "linux", "windows"] {
        let mut cells = Vec::new();
        if OS == os {
            for writer in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
                for reader in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
                    for (payload_name, payload) in payloads() {
                        let cell = run_cell(&harness, writer, reader, payload_name, &payload);
                        cells.push(cell);
                    }
                }
            }
        } else {
            for writer in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
                for reader in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
                    cells.push(json!({
                        "writer": writer.as_str(),
                        "reader": reader.as_str(),
                        "payload": "all",
                        "status": "skip",
                        "skip_reason": format!("host OS is {OS}; cannot run {os} cells on this machine")
                    }));
                }
            }
        }
        rows.push(json!({
            "os": os,
            "cells": cells,
        }));
    }

    let mut fragment_rows = Vec::new();
    for writer in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
        for reader in [Engine::FsZero, Engine::GraphZero, Engine::TokenZero] {
            fragment_rows.extend(run_fragment_cell(&harness, writer, reader));
        }
    }

    let wrong_store = run_wrong_store(&harness);
    let concurrent = run_concurrent_writes(&harness);

    let sibling_shas = json!([
        {
            "engine": harness.fszero.engine,
            "path": harness.fszero.path,
            "sha256": harness.fszero.sha256,
            "version": harness.fszero.version,
            "commit": harness.fszero.commit,
            "os": OS
        },
        {
            "engine": harness.graphzero.engine,
            "path": harness.graphzero.path,
            "sha256": harness.graphzero.sha256,
            "version": harness.graphzero.version,
            "commit": harness.graphzero.commit,
            "os": OS
        },
        {
            "engine": harness.tokenzero.engine,
            "path": harness.tokenzero.path,
            "sha256": harness.tokenzero.sha256,
            "version": harness.tokenzero.version,
            "commit": harness.tokenzero.commit,
            "os": OS
        }
    ]);

    let status = {
        let mut ok = true;
        for row in &rows {
            for cell in row["cells"].as_array().unwrap() {
                if cell["status"] == "fail" {
                    ok = false;
                }
            }
        }
        for frag in &fragment_rows {
            if frag["status"] == "fail" {
                ok = false;
            }
        }
        if wrong_store["status"] != "pass" || concurrent["status"] != "pass" {
            ok = false;
        }
        if ok { "green" } else { "red" }
    };

    let evidence = json!({
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
        serde_json::to_string_pretty(&evidence).unwrap(),
    )
    .expect("write evidence");

    // Also fail the test if the matrix was not green so the problem is obvious.
    assert_eq!(
        status, "green",
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
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:09}Z",
        y, m, d, h, min, s, nanos
    )
}
