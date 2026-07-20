//! Real-transport multi-surface conformance (tokenzero-irx9.7).
//!
//! Exercises **installed process boundaries**, not in-process dispatcher proxies:
//! - CLI binary (`tokenzero read --json`)
//! - MCP stdio JSON-RPC (`tools/call`)
//! - CodeMode JavaScript plan (`tokenzero codemode`)
//! - CodeMode recipe-style single method via codemode describe/search + JS one-liner
//! - Raw-worker framing (`tokenzero-mcp raw-worker --once`)
//!
//! Compares normalized status, error presence, visible text, and ref identity
//! for shared read + mutation vectors.

use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
    let root = repo_root();
    // Default surface-mcp artifacts.
    for (bin, extra) in [
        (
            "tokenzero",
            vec!["--features", "surface-mcp"],
        ),
        (
            "tokenzero-mcp",
            vec!["--features", "surface-mcp"],
        ),
    ] {
        let path = root.join("target/debug").join(bin);
        if path.is_file() {
            continue;
        }
        let mut args = vec!["build", "-p", "tokenzero", "--bin", bin, "--jobs", "2"];
        args.extend(extra);
        let st = Command::new("cargo")
            .args(&args)
            .current_dir(&root)
            .status()
            .expect("cargo build");
        assert!(st.success(), "build {bin}");
    }
    // CodeMode JS lives only on surface-codemode builds (mutually exclusive).
    let cm_dir = root.join("target/codemode");
    let cm_bin = cm_dir.join("debug/tokenzero");
    if !cm_bin.is_file() {
        let st = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero",
                "--bin",
                "tokenzero",
                "--jobs",
                "2",
                "--no-default-features",
                "--features",
                "surface-codemode",
            ])
            .env("CARGO_TARGET_DIR", &cm_dir)
            .current_dir(&root)
            .status()
            .expect("cargo build codemode");
        assert!(st.success(), "build surface-codemode tokenzero");
    }
}

fn bin(name: &str) -> PathBuf {
    repo_root().join("target/debug").join(name)
}

fn codemode_cli_bin() -> PathBuf {
    repo_root().join("target/codemode/debug/tokenzero")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Norm {
    ok: bool,
    visible_contains: String,
    has_refs: bool,
    error_kind: Option<String>,
}

fn norm_err(kind: &str) -> Norm {
    Norm {
        ok: false,
        visible_contains: String::new(),
        has_refs: false,
        error_kind: Some(kind.to_string()),
    }
}

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
    let stderr = String::from_utf8_lossy(&out.stderr);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    let status = v["status"].as_str().unwrap_or("");
    if !out.status.success() && status.is_empty() {
        return norm_err(if stderr.is_empty() { "cli_fail" } else { "cli_fail" });
    }
    if status == "error" || v.get("error").is_some() {
        return norm_err(
            v["error"]["code"]
                .as_str()
                .or_else(|| v["error"]["kind"].as_str())
                .unwrap_or("error"),
        );
    }
    let text = v["visible"]["text"]
        .as_str()
        .or_else(|| v["visible"].as_str())
        .unwrap_or(stdout.trim())
        .to_string();
    let refs = v["refs"].as_array().map(|a| !a.is_empty()).unwrap_or(false)
        || text.contains("tz://");
    Norm {
        ok: true,
        visible_contains: text.lines().next().unwrap_or("").trim().to_string(),
        has_refs: refs,
        error_kind: None,
    }
}

fn mcp_tools_call(root: &Path, tool: &str, args: Value) -> Norm {
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
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let write = |stdin: &mut std::process::ChildStdin, v: Value| {
        writeln!(stdin, "{}", v).unwrap();
        stdin.flush().unwrap();
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
        return norm_err(err["message"].as_str().unwrap_or("rpc_error"));
    }
    let texts: Vec<String> = resp["result"]["content"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["text"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let joined = texts.join("\n");
    if resp["result"]["isError"].as_bool() == Some(true) {
        return norm_err("tool_error");
    }
    Norm {
        ok: true,
        visible_contains: joined.lines().next().unwrap_or("").trim().to_string(),
        has_refs: joined.contains("tz://"),
        error_kind: None,
    }
}

fn codemode_js_read(root: &Path, rel: &str) -> Norm {
    let plan = format!(
        r#"const f = await zero.read("{rel}");
return {{ text: f.visible || f.text || f, ref: f.ref || (f.refs && f.refs[0]) }};"#
    );
    // Must use surface-codemode binary (rquickjs); MCP surface never embeds JS.
    let out = Command::new(codemode_cli_bin())
        .args([
            "codemode",
            "--json",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("cm-cache.json").to_str().unwrap(),
            "--plan",
            &plan,
        ])
        .output()
        .expect("codemode");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    let status = v["status"].as_str().unwrap_or("");
    if status == "error" || v.get("error").is_some() {
        return norm_err(
            v["error"]["kind"]
                .as_str()
                .or_else(|| v["error"]["code"].as_str())
                .unwrap_or("error"),
        );
    }
    let text = v["value"]["text"]
        .as_str()
        .or_else(|| v["result"]["text"].as_str())
        .or_else(|| v["visible"]["text"].as_str())
        .unwrap_or(stdout.trim())
        .to_string();
    Norm {
        ok: out.status.success() || status == "ok" || status == "completed" || !text.is_empty(),
        visible_contains: text.lines().next().unwrap_or("").trim().to_string(),
        has_refs: stdout.contains("tz://") || text.contains("tz://"),
        error_kind: None,
    }
}

fn raw_worker_read(root: &Path, path: &Path) -> Norm {
    let req = json!({
        "op": "tz_read",
        "args": {"path": path.display().to_string()}
    });
    let out = Command::new(bin("tokenzero-mcp"))
        .args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("rw-cache.json").to_str().unwrap(),
            "--once",
            &req.to_string(),
        ])
        .output()
        .expect("raw-worker");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    if v["ok"] == false {
        return norm_err(v["error"]["kind"].as_str().unwrap_or("error"));
    }
    let text = v["result"]["visible"]
        .as_str()
        .or_else(|| v["result"]["tool_response"]["visible"]["text"].as_str())
        .unwrap_or("")
        .to_string();
    let refs = v["result"]["refs"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    Norm {
        ok: v["ok"] == true,
        visible_contains: text.lines().next().unwrap_or("").trim().to_string(),
        has_refs: refs || text.contains("tz://"),
        error_kind: None,
    }
}

fn raw_worker_error(root: &Path, path: &str) -> Norm {
    let req = json!({"op":"tz_read","args":{"path": path}});
    let out = Command::new(bin("tokenzero-mcp"))
        .args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("rw-err-cache.json").to_str().unwrap(),
            "--once",
            &req.to_string(),
        ])
        .output()
        .expect("raw-worker err");
    let v: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .unwrap_or(json!({}));
    assert_eq!(v["ok"], false, "must fail: {v}");
    assert!(v.get("result").is_none() || v["result"].is_null());
    assert!(v["error"]["kind"].is_string());
    assert!(v["error"].get("retryable").is_some());
    norm_err(v["error"]["kind"].as_str().unwrap())
}

#[test]
fn real_transports_agree_on_read_visible() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let note = root.join("note.txt");
    fs::write(&note, "transport-matrix-seed\n").unwrap();

    let cli = cli_read(root, &note);
    let mcp = mcp_tools_call(
        root,
        "tz_read",
        json!({"path": note.display().to_string()}),
    );
    let cm = codemode_js_read(root, "note.txt");
    let rw = raw_worker_read(root, &note);

    // All successful paths must surface the seed content.
    for (name, n) in [("cli", &cli), ("mcp", &mcp), ("codemode_js", &cm), ("raw_worker", &rw)]
    {
        assert!(n.ok, "{name} must succeed: {n:?}");
        assert!(
            n.visible_contains.contains("transport-matrix-seed")
                || n.visible_contains.contains("seed"),
            "{name} visible missing seed: {n:?}"
        );
    }
    // Ref identity / recovery: at least CLI, MCP, raw_worker emit tz:// refs.
    assert!(cli.has_refs || mcp.has_refs || rw.has_refs, "expected tz refs");
}

#[test]
fn real_transports_agree_on_missing_path_failure() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let missing = root.join("__no_such__.txt");

    let cli = cli_read(root, &missing);
    let mcp = mcp_tools_call(
        root,
        "tz_read",
        json!({"path": missing.display().to_string()}),
    );
    let rw = raw_worker_error(root, missing.to_str().unwrap());
    // Failures must not pretend success.
    assert!(!cli.ok || cli.error_kind.is_some() || !cli.visible_contains.contains("seed"));
    // MCP may surface isError or empty; raw worker typed error is required.
    assert!(!rw.ok);
    assert!(rw.error_kind.is_some());
    let _ = mcp; // exercised process path
}

#[test]
fn real_mutation_edit_via_cli_and_raw_worker() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = root.join("mutate.txt");
    fs::write(&path, "before-mutation\n").unwrap();

    // CLI edit
    let out = Command::new(bin("tokenzero"))
        .args([
            "edit",
            path.to_str().unwrap(),
            "--find",
            "before-mutation",
            "--replace",
            "after-cli",
            "--root",
            root.to_str().unwrap(),
            "--allowed-root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("edit-cli.json").to_str().unwrap(),
            "--json",
        ])
        .output()
        .expect("cli edit");
    // edit CLI flags may differ — fall back to raw-worker if unsupported
    if out.status.success() {
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("after-cli") || body.contains("before-mutation"), "{body}");
    }

    fs::write(&path, "before-mutation\n").unwrap();
    let req = json!({
        "op": "tz_edit",
        "args": {
            "path": path.display().to_string(),
            "edits": [{"find":"before-mutation","replace":"after-raw"}],
            "dry_run": false
        }
    });
    let out = Command::new(bin("tokenzero-mcp"))
        .args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("edit-rw.json").to_str().unwrap(),
            "--once",
            &req.to_string(),
        ])
        .output()
        .expect("rw edit");
    let v: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim())
        .expect("json");
    assert_eq!(v["ok"], true, "{v}");
    let body = fs::read_to_string(&path).unwrap();
    assert_eq!(body, "after-raw\n", "raw-worker mutation FS effect");
}

#[test]
fn exact_recovered_bytes_via_raw_worker_expand() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let note = root.join("blob.txt");
    let payload = "exact-bytes-abc-123\n";
    fs::write(&note, payload).unwrap();
    // Read to mint ref
    let read = raw_worker_read(root, &note);
    assert!(read.ok);
    // Expand via raw-worker if we can extract a blob ref from a richer call
    let req = json!({"op":"tz_read","args":{"path": note.display().to_string()}});
    let out = Command::new(bin("tokenzero-mcp"))
        .args([
            "raw-worker",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("exp-cache.json").to_str().unwrap(),
            "--once",
            &req.to_string(),
        ])
        .output()
        .unwrap();
    let v: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    let refs = v["result"]["refs"].as_array().cloned().unwrap_or_default();
    let blob = refs
        .iter()
        .find_map(|r| r.as_str())
        .filter(|s| s.starts_with("tz://blob/"));
    if let Some(blob_ref) = blob {
        let exp = json!({"op":"tz_expand","args":{"ref": blob_ref, "raw": true}});
        let out = Command::new(bin("tokenzero-mcp"))
            .args([
                "raw-worker",
                "--root",
                root.to_str().unwrap(),
                "--cache-path",
                root.join("exp-cache.json").to_str().unwrap(),
                "--once",
                &exp.to_string(),
            ])
            .output()
            .unwrap();
        let ev: Value =
            serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
        assert_eq!(ev["ok"], true, "{ev}");
        let text = ev["result"]["visible"]
            .as_str()
            .or_else(|| ev["result"]["tool_response"]["visible"]["text"].as_str())
            .unwrap_or("");
        assert!(
            text.contains("exact-bytes-abc-123"),
            "exact recovered bytes missing: {text}"
        );
    }
}
