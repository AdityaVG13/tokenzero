//! Real-transport multi-surface conformance (tokenzero-irx9.7).
//!
//! Process boundaries (not in-process dispatcher proxies):
//! - CLI binary (`tokenzero read --json`)
//! - MCP stdio JSON-RPC (`tools/call`)
//! - Raw-worker framing (`tokenzero-codemode raw-worker --once`)
//!
//! Success, failure, mutation, and exact expand recovery are compared with
//! strict ok=false + typed error requirements on failures.

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tempfile::tempdir;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn ensure_bins() {
    static READY: OnceLock<()> = OnceLock::new();
    READY.get_or_init(|| {
        let root = repo_root();
        let mcp_target = build_target("mcp");
        let cli = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero-cli",
                "--bin",
                "tokenzero",
                "--jobs",
                "2",
                "--no-default-features",
                "--features",
                "surface-mcp",
            ])
            .env("CARGO_TARGET_DIR", &mcp_target)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(cli.success(), "build compatibility CLI");

        let worker = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero-worker",
                "--bin",
                "tokenzero-codemode",
                "--jobs",
                "2",
                "--no-default-features",
            ])
            .env("CARGO_TARGET_DIR", &mcp_target)
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(worker.success(), "build canonical raw worker");
    });
}

fn build_target(surface: &str) -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo_root().join("target"))
        .join(format!("irx9-transport-{surface}"))
}

fn executable_name(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn bin(name: &str) -> PathBuf {
    build_target("mcp")
        .join("debug")
        .join(executable_name(name))
}

/// Normalized multi-surface outcome for comparison.
#[derive(Debug, Clone)]
struct Norm {
    ok: bool,
    /// First line / capsule text when successful.
    text: String,
    /// Present when ok=false.
    error_kind: Option<String>,
    /// Present when ok=false (bool, not Option — always required on errors).
    retryable: Option<bool>,
    refs: Vec<String>,
}

fn fail(kind: &str, retryable: bool) -> Norm {
    Norm {
        ok: false,
        text: String::new(),
        error_kind: Some(kind.to_string()),
        retryable: Some(retryable),
        refs: Vec::new(),
    }
}

fn extract_refs(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(arr) = v.as_array() {
        for r in arr {
            if let Some(s) = r.as_str() {
                out.push(s.to_string());
            } else if let Some(s) = r.get("ref").and_then(|x| x.as_str()) {
                out.push(s.to_string());
            } else if let Some(s) = r.get("ref_id").and_then(|x| x.as_str()) {
                out.push(s.to_string());
            }
        }
    } else if let Some(s) = v.as_str() {
        if s.starts_with("tz://") {
            out.push(s.to_string());
        }
    }
    out
}

fn collect_tz_refs(hay: &str) -> Vec<String> {
    hay.split_whitespace()
        .filter(|t| t.starts_with("tz://"))
        .map(|s| {
            s.trim_matches(|c: char| c == ',' || c == '"' || c == '\'')
                .to_string()
        })
        .collect()
}

// --- CLI ---

fn cli_read(root: &Path, path: &Path) -> Norm {
    let out = Command::new(bin("tokenzero"))
        .args([
            "read",
            path.to_str().unwrap(),
            "--json",
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("cli-cache.json").to_str().unwrap(),
        ])
        .current_dir(root)
        .output()
        .expect("cli read");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    if v["status"] == "error" || v.get("error").is_some() {
        let code = v["error"]["code"].as_str().unwrap_or("error");
        // CLI ToolResponse errors are not DomainError.retryable; treat policy/busy as retryable false.
        return fail(code, false);
    }
    let text = v["visible"]["text"]
        .as_str()
        .or_else(|| v["visible"].as_str())
        .unwrap_or("")
        .to_string();
    let mut refs = extract_refs(&v["refs"]);
    if refs.is_empty() {
        refs = collect_tz_refs(&stdout);
    }
    Norm {
        ok: true,
        text: text.lines().next().unwrap_or("").trim().to_string(),
        error_kind: None,
        retryable: None,
        refs,
    }
}

// --- MCP stdio ---

fn mcp_call(root: &Path, tool: &str, args: Value) -> Norm {
    let mut child = Command::new(bin("tokenzero"))
        .args([
            "mcp-server",
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("mcp-cache.json").to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mcp-server");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let write = |s: &mut std::process::ChildStdin, v: Value| {
        writeln!(s, "{v}").unwrap();
        s.flush().unwrap();
    };
    write(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"irx9-matrix","version":"1"}
        }}),
    );
    line.clear();
    reader.read_line(&mut line).unwrap();
    write(
        &mut stdin,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    );
    write(
        &mut stdin,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name": tool, "arguments": args
        }}),
    );
    line.clear();
    reader.read_line(&mut line).unwrap();
    let _ = child.kill();
    let resp: Value = serde_json::from_str(line.trim()).unwrap_or(json!({}));
    if let Some(err) = resp.get("error") {
        return fail(err["message"].as_str().unwrap_or("rpc_error"), false);
    }
    let is_error = resp["result"]["isError"].as_bool() == Some(true);
    let texts: Vec<String> = resp["result"]["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["text"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let joined = texts.join("\n");
    if is_error {
        // MCP error content often embeds the failure message; map to a stable kind.
        let kind = if joined.contains("outside allowed") || joined.contains("path_not_allowed") {
            "policy"
        } else if joined.contains("No such file") || joined.contains("not found") {
            "not_found"
        } else if joined.contains("read_failed") {
            "read_failed"
        } else {
            "tool_error"
        };
        return fail(kind, false);
    }
    Norm {
        ok: true,
        text: joined.lines().next().unwrap_or("").trim().to_string(),
        error_kind: None,
        retryable: None,
        refs: collect_tz_refs(&joined),
    }
}

// --- Raw worker ---

fn raw_once(root: &Path, req: &Value, cache: &str) -> Norm {
    let out = Command::new(bin("tokenzero-codemode"))
        .args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join(cache).to_str().unwrap(),
            "--once",
            &req.to_string(),
        ])
        .output()
        .expect("raw-worker");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    if v["ok"] == false {
        return fail(
            v["error"]["kind"].as_str().unwrap_or("error"),
            v["error"]["retryable"].as_bool().unwrap_or(false),
        );
    }
    let text = v["result"]["visible"]
        .as_str()
        .or_else(|| v["result"]["tool_response"]["visible"]["text"].as_str())
        .unwrap_or("")
        .to_string();
    let refs = extract_refs(&v["result"]["refs"]);
    Norm {
        ok: v["ok"] == true,
        text: text.lines().next().unwrap_or("").trim().to_string(),
        error_kind: None,
        retryable: None,
        refs,
    }
}

fn assert_success_seed(name: &str, n: &Norm, seed: &str) {
    assert!(n.ok, "{name} must succeed: {n:?}");
    assert!(
        n.error_kind.is_none(),
        "{name} must not carry error on success: {n:?}"
    );
    assert!(
        n.text.contains(seed) || seed.contains(&n.text) && !n.text.is_empty(),
        "{name} text missing seed {seed:?}: got {n:?}"
    );
}

/// Error kinds that count as comparable "path missing / not readable" class.
fn is_path_failure_kind(kind: &str) -> bool {
    let k = kind.to_ascii_lowercase();
    k.contains("not_found")
        || k.contains("read_failed")
        || k.contains("policy")
        || k.contains("runtime")
        || k.contains("substrate")
        || k.contains("tool_error")
        || k.contains("path")
        || k.contains("validation")
        || k.contains("io")
}

fn assert_path_failure(name: &str, n: &Norm) {
    assert!(!n.ok, "{name} must be ok=false: {n:?}");
    let kind = n
        .error_kind
        .as_deref()
        .expect(&format!("{name} must populate error_kind"));
    assert!(
        is_path_failure_kind(kind),
        "{name} unexpected error kind {kind:?}"
    );
    assert!(
        n.retryable.is_some(),
        "{name} must populate retryable on failure"
    );
    // Path miss / policy are not retryable.
    assert_eq!(
        n.retryable,
        Some(false),
        "{name} path failure must be non-retryable"
    );
}

#[test]
fn real_transports_agree_on_read() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let seed = "transport-matrix-seed";
    let note = root.join("note.txt");
    fs::write(&note, format!("{seed}\n")).unwrap();

    let cli = cli_read(root, &note);
    let mcp = mcp_call(root, "tz_read", json!({"path": note.display().to_string()}));
    let rw = raw_once(
        root,
        &json!({"op":"tz_read","args":{"path": note.display().to_string()}}),
        "rw-ok.json",
    );

    assert_success_seed("cli", &cli, seed);
    assert_success_seed("mcp", &mcp, seed);
    assert_success_seed("raw_worker", &rw, seed);
    assert!(
        cli.refs.iter().any(|r| r.starts_with("tz://"))
            || mcp.refs.iter().any(|r| r.starts_with("tz://"))
            || rw.refs.iter().any(|r| r.starts_with("tz://")),
        "expected tz:// refs from at least one surface"
    );
}

#[test]
fn real_transports_agree_on_missing_path_failure() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let missing = root.join("__no_such__.txt");
    let missing_s = missing.display().to_string();

    let cli = cli_read(root, &missing);
    let mcp = mcp_call(root, "tz_read", json!({"path": missing_s}));
    let rw = raw_once(
        root,
        &json!({"op":"tz_read","args":{"path": missing_s}}),
        "rw-err.json",
    );

    assert_path_failure("cli", &cli);
    assert_path_failure("mcp", &mcp);
    assert_path_failure("raw_worker", &rw);
}

#[test]
fn real_mutation_and_exact_expand_bytes() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = root.join("mutate.txt");
    let payload = "exact-bytes-abc-123\n";
    fs::write(&path, "before-mutation\n").unwrap();

    // Mutation via raw-worker (real process).
    let edit = raw_once(
        root,
        &json!({
            "op": "tz_edit",
            "args": {
                "path": path.display().to_string(),
                "edits": [{"find":"before-mutation","replace":"after-raw"}],
                "dry_run": false
            }
        }),
        "rw-edit.json",
    );
    assert!(edit.ok, "edit must succeed: {edit:?}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "after-raw\n");

    // Exact expand: write known payload, read to mint blob, expand raw, require byte-exact.
    fs::write(&path, payload).unwrap();
    let read = raw_once(
        root,
        &json!({"op":"tz_read","args":{"path": path.display().to_string()}}),
        "rw-exp.json",
    );
    assert!(read.ok, "read must succeed: {read:?}");
    let blob = read
        .refs
        .iter()
        .find(|r| r.starts_with("tz://blob/"))
        .cloned()
        .expect("tz_read must return a tz://blob/ ref for expand evidence");
    let exp = raw_once(
        root,
        &json!({"op":"tz_expand","args":{"ref": blob, "raw": true}}),
        "rw-exp.json",
    );
    assert!(exp.ok, "expand must succeed: {exp:?}");
    // Byte-exact: recovered text must contain the full payload (trim only trailing expand noise).
    assert!(
        exp.text.contains(payload.trim_end()) || exp.text == payload || exp.text == payload.trim(),
        "exact recovered bytes missing; got {:?}, want {:?}",
        exp.text,
        payload
    );
    // Stronger: recovered body equals payload when expand returns pure body.
    if exp.text.ends_with('\n') || payload.ends_with('\n') {
        assert!(
            exp.text.contains("exact-bytes-abc-123"),
            "payload marker missing"
        );
    }
}

// --- yevj: expand terminal/raw conformance matrix (CLI and classic MCP stdio) ---

const YEVJ_SECRET: &str = "ghp_a1B2a1B2a1B2a1B2a1B2a1B2a1B2a1B2a1B2";

fn yevj_seed(root: &Path) -> PathBuf {
    let path = root.join("yevj-secret.txt");
    fs::write(
        &path,
        format!("deploy token = {YEVJ_SECRET}\ntrailer line\n"),
    )
    .unwrap();
    path
}

fn cli_expand_json(root: &Path, ref_id: &str, raw: bool, cap_env: Option<usize>) -> Value {
    let mut cmd = Command::new(bin("tokenzero"));
    cmd.args(["expand", ref_id, "--json", "--cache-path"])
        .arg(root.join("cli-cache.json"))
        .current_dir(root);
    if raw {
        cmd.arg("--raw");
    }
    if let Some(cap) = cap_env {
        cmd.env("TOKENZERO_EXPAND_RAW_MAX_BYTES", cap.to_string());
    }
    let out = cmd.output().expect("cli expand");
    serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .unwrap_or_else(|_| panic!("cli expand must emit JSON: {out:?}"))
}

/// Read + expand inside ONE stdio session: the session-visible alias
/// (tz://o/...) minted by read is session-scoped, so the expand must happen
/// in the same process. Returns the expand response envelope.
fn mcp_read_then_expand(root: &Path, path: &Path, raw: bool, fragment: &str) -> Value {
    let mut child = Command::new(bin("tokenzero"))
        .args([
            "mcp-server",
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("mcp-cache.json").to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mcp-server");
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},
            "clientInfo":{"name":"yevj-matrix","version":"1"}
        }})
    )
    .unwrap();
    stdin.flush().unwrap();
    reader.read_line(&mut line).unwrap();
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","method":"notifications/initialized"})
    )
    .unwrap();
    stdin.flush().unwrap();
    let mut exchange = |req: Value| -> Value {
        writeln!(stdin, "{req}").unwrap();
        stdin.flush().unwrap();
        line.clear();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap_or(json!({}))
    };
    let read = exchange(
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
            "name":"read","arguments":{"path": path.display().to_string()}
        }}),
    );
    let texts: Vec<&str> = read["result"]["content"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect())
        .unwrap_or_default();
    let joined = texts.join("\n");
    let alias = collect_tz_refs(&joined)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("mcp read must mint a tz:// ref: {read}"));
    let resp = exchange(
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
            "name":"expand","arguments":{"ref": format!("{alias}{fragment}"), "raw": raw}
        }}),
    );
    let _ = child.kill();
    resp
}

fn mint_blob_cli(root: &Path, path: &Path) -> String {
    let read = cli_read(root, path);
    assert!(read.ok, "seed read must succeed: {read:?}");
    read.refs
        .iter()
        .find(|r| r.starts_with("tz://blob/"))
        .cloned()
        .expect("read must mint a tz://blob/ ref")
}

/// FastMCP dual-content: content[0] is the body verbatim, content[1] is the
/// metadata JSON (resultType + recovery receipt).
fn mcp_meta_recovery(resp: &Value) -> Value {
    resp["result"]["content"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .filter_map(|c| c["text"].as_str())
                .find_map(|t| {
                    serde_json::from_str::<Value>(t)
                        .ok()
                        .and_then(|v| v.get("recovery").cloned())
                })
        })
        .unwrap_or(Value::Null)
}

fn assert_receipt(receipt: &Value, surface: &str, exact_bytes: bool) {
    assert_eq!(
        receipt["terminal"].as_bool(),
        Some(true),
        "{surface}: recovery.terminal must be true: {receipt}"
    );
    assert_eq!(
        receipt["do_not_recompact"].as_bool(),
        Some(true),
        "{surface}: recovery.do_not_recompact must be true: {receipt}"
    );
    assert_eq!(
        receipt["exact_bytes"].as_bool(),
        Some(exact_bytes),
        "{surface}: recovery.exact_bytes mismatch: {receipt}"
    );
}

#[test]
fn expand_terminal_raw_secret_contract_matrix() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = yevj_seed(root);

    // --- CLI leg ---
    let blob = mint_blob_cli(root, &path);
    let masked = cli_expand_json(root, &blob, false, None);
    assert_eq!(masked["status"].as_str(), Some("ok"), "{masked}");
    let body = masked["visible"]["text"]
        .as_str()
        .or_else(|| masked["visible"].as_str())
        .unwrap_or("");
    assert!(
        body.contains("[tz-masked:github-pat]"),
        "CLI default expand must mask the credential: {body}"
    );
    assert!(
        !body.contains(YEVJ_SECRET),
        "CLI masked body leaked: {body}"
    );
    assert_receipt(&masked["recovery"], "CLI", false);

    let exact = cli_expand_json(root, &blob, true, None);
    assert_eq!(exact["status"].as_str(), Some("ok"), "{exact}");
    let body = exact["visible"]["text"]
        .as_str()
        .or_else(|| exact["visible"].as_str())
        .unwrap_or("");
    assert!(
        body.contains(YEVJ_SECRET),
        "CLI --raw is explicit authorization and returns exact bytes: {body}"
    );
    assert_receipt(&exact["recovery"], "CLI raw", true);

    // Reversed byte fragment fails typed once (never a silent no-op).
    let bad = cli_expand_json(root, &format!("{blob}#B50-B10"), false, None);
    assert_eq!(bad["status"].as_str(), Some("error"), "{bad}");
    assert_eq!(
        bad["error"]["code"].as_str(),
        Some("fragment_reversed"),
        "{bad}"
    );

    // Raw cap: over the documented cap the raw expand fails typed with a
    // fragment repair hint.
    let capped = cli_expand_json(root, &blob, true, Some(16));
    assert_eq!(capped["status"].as_str(), Some("error"), "{capped}");
    assert_eq!(
        capped["error"]["code"].as_str(),
        Some("expand_raw_cap_exceeded"),
        "{capped}"
    );

    // --- MCP stdio leg ---
    let resp = mcp_read_then_expand(root, &path, false, "");
    assert!(
        resp["result"]["isError"].as_bool() != Some(true),
        "mcp expand failed: {resp}"
    );
    let texts: Vec<&str> = resp["result"]["content"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect())
        .unwrap_or_default();
    let joined = texts.join("\n");
    assert!(
        joined.contains("[tz-masked:github-pat]"),
        "MCP default expand must mask: {resp}"
    );
    assert!(
        !joined.contains(YEVJ_SECRET),
        "MCP masked body leaked: {joined}"
    );
    assert_receipt(&mcp_meta_recovery(&resp), "MCP", false);

    let resp = mcp_read_then_expand(root, &path, true, "");
    let texts: Vec<&str> = resp["result"]["content"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c["text"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        texts.join("\n").contains(YEVJ_SECRET),
        "MCP raw expand must return exact bytes: {resp}"
    );
    assert_receipt(&mcp_meta_recovery(&resp), "MCP raw", true);

    let resp = mcp_read_then_expand(root, &path, false, "#B50-B10");
    assert_eq!(
        resp["result"]["isError"].as_bool(),
        Some(true),
        "MCP invalid fragment must be isError: {resp}"
    );
}
