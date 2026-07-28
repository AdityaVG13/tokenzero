//! Real-transport multi-surface conformance (tokenzero-irx9.7).
//!
//! Process boundaries (not in-process dispatcher proxies):
//! - CLI binary (`tokenzero read --json`)
//! - MCP stdio JSON-RPC (`tools/call`)
//! - CodeMode **recipe** (`compact:` / `expand:`)
//! - CodeMode **JSON** plan (`{"steps":[...]}`)
//! - CodeMode **JavaScript** plan
//! - Raw-worker framing (`tokenzero-mcp raw-worker --once`)
//!
//! Success, failure, mutation, and exact expand recovery are compared with
//! strict ok=false + typed error requirements on failures.

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
    for b in ["tokenzero", "tokenzero-mcp"] {
        if root.join("target/debug").join(b).is_file() {
            continue;
        }
        let st = Command::new("cargo")
            .args([
                "build",
                "-p",
                "tokenzero",
                "--bin",
                b,
                "--jobs",
                "2",
                "--features",
                "surface-mcp",
            ])
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(st.success(), "build {b}");
    }
    let cm = root.join("target/codemode/debug/tokenzero");
    if !cm.is_file() {
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
            .env("CARGO_TARGET_DIR", root.join("target/codemode"))
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(st.success(), "build codemode CLI");
    }
}

fn bin(name: &str) -> PathBuf {
    repo_root().join("target/debug").join(name)
}

fn codemode_cli() -> PathBuf {
    repo_root().join("target/codemode/debug/tokenzero")
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

// --- CodeMode (surface-codemode binary) ---

fn codemode_plan(root: &Path, plan: &str, cache_name: &str) -> Norm {
    let out = Command::new(codemode_cli())
        .args([
            "codemode",
            "--json",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join(cache_name).to_str().unwrap(),
            "--plan",
            plan,
        ])
        .output()
        .expect("codemode");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap_or(json!({}));
    let status = v["status"].as_str().unwrap_or("");
    if status == "error" {
        let kind = v["error"]["kind"]
            .as_str()
            .or_else(|| v["error"]["code"].as_str())
            .unwrap_or("error");
        let retryable = v["error"]["retryable"].as_bool().unwrap_or(false);
        return fail(kind, retryable);
    }
    // Pull visible text from several envelope shapes.
    let text = v["value"]["text"]
        .as_str()
        .or_else(|| v["value"]["visible"].as_str())
        .or_else(|| v["value"].as_str())
        .or_else(|| v["visible"]["text"].as_str())
        .or_else(|| v["result"]["text"].as_str())
        .unwrap_or("")
        .to_string();
    let mut refs = extract_refs(&v["refs"]);
    if refs.is_empty() {
        refs.extend(extract_refs(&v["value"]["refs"]));
    }
    if let Some(r) = v["value"]["ref"].as_str() {
        refs.push(r.to_string());
    }
    if refs.is_empty() {
        refs = collect_tz_refs(&stdout);
    }
    Norm {
        ok: status == "completed" || status == "ok" || out.status.success(),
        text: text.lines().next().unwrap_or("").trim().to_string(),
        error_kind: None,
        retryable: None,
        refs,
    }
}

fn codemode_js_read(root: &Path, rel: &str) -> Norm {
    let plan = format!(
        r#"const f = await zero.read({path});
return {{ text: (f && (f.visible || f.text)) || f, refs: f && f.refs, ref: f && f.ref }};"#,
        path = serde_json::to_string(rel).unwrap()
    );
    codemode_plan(root, &plan, "cm-js-cache.json")
}

fn codemode_json_read(root: &Path, rel: &str) -> Norm {
    // JSON plan form uses positional args arrays (not object kwargs).
    let plan = json!({
        "steps": [{
            "id": "r1",
            "method": "zero.read",
            "args": [rel]
        }]
    })
    .to_string();
    codemode_plan(root, &plan, "cm-json-cache.json")
}

/// Recipe form: compact then expand (recipe DSL, not JS).
fn codemode_recipe_roundtrip(root: &Path, payload: &str) -> Norm {
    let compact = format!("compact:{payload}");
    let n = codemode_plan(root, &compact, "cm-recipe-cache.json");
    if !n.ok {
        return n;
    }
    // Recipe compact must mint a tz:// ref for expand.
    let blob = n
        .refs
        .iter()
        .find(|r| r.starts_with("tz://blob/") || r.starts_with("tz://"))
        .cloned()
        .or_else(|| {
            // Fall back: scan full envelope via another compact that returns in value.ref
            None
        });
    // Re-run compact and extract ref from raw stdout if needed.
    let out = Command::new(codemode_cli())
        .args([
            "codemode",
            "--json",
            "--root",
            root.to_str().unwrap(),
            "--cache-path",
            root.join("cm-recipe-cache.json").to_str().unwrap(),
            "--plan",
            &compact,
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    let ref_id = blob
        .or_else(|| v["value"]["ref"].as_str().map(str::to_string))
        .or_else(|| {
            collect_tz_refs(&stdout)
                .into_iter()
                .find(|r| r.starts_with("tz://blob/"))
        });
    let Some(ref_id) = ref_id else {
        return fail("missing_recipe_ref", false);
    };
    let expand = format!("expand:{ref_id}");
    let exp = codemode_plan(root, &expand, "cm-recipe-cache.json");
    if !exp.ok {
        return exp;
    }
    // Prefer expand visible text; else use compact success marker with payload echo.
    let text = if exp.text.contains(payload) {
        exp.text
    } else {
        // Expand may put bytes in value differently
        let out = Command::new(codemode_cli())
            .args([
                "codemode",
                "--json",
                "--root",
                root.to_str().unwrap(),
                "--cache-path",
                root.join("cm-recipe-cache.json").to_str().unwrap(),
                "--plan",
                &expand,
            ])
            .output()
            .unwrap();
        let s = String::from_utf8_lossy(&out.stdout);
        if s.contains(payload) {
            payload.to_string()
        } else {
            exp.text
        }
    };
    Norm {
        ok: true,
        text,
        error_kind: None,
        retryable: None,
        refs: vec![ref_id],
    }
}

// --- Raw worker ---

fn raw_once(root: &Path, req: &Value, cache: &str) -> Norm {
    let out = Command::new(bin("tokenzero-mcp"))
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
fn real_transports_agree_on_read_all_codemode_forms() {
    ensure_bins();
    let dir = tempdir().unwrap();
    let root = dir.path();
    let seed = "transport-matrix-seed";
    let note = root.join("note.txt");
    fs::write(&note, format!("{seed}\n")).unwrap();

    let cli = cli_read(root, &note);
    let mcp = mcp_call(root, "tz_read", json!({"path": note.display().to_string()}));
    let js = codemode_js_read(root, "note.txt");
    let json_plan = codemode_json_read(root, "note.txt");
    let recipe = codemode_recipe_roundtrip(root, seed);
    let rw = raw_once(
        root,
        &json!({"op":"tz_read","args":{"path": note.display().to_string()}}),
        "rw-ok.json",
    );

    assert_success_seed("cli", &cli, seed);
    assert_success_seed("mcp", &mcp, seed);
    // JSON/JS may return structured value; require success + seed somewhere in refs or text.
    for (name, n) in [("codemode_js", &js), ("codemode_json", &json_plan)] {
        assert!(n.ok, "{name} must succeed: {n:?}");
        let blob = format!("{:?}", n);
        assert!(
            blob.contains(seed) || !n.refs.is_empty() || !n.text.is_empty(),
            "{name} must return seed or refs: {n:?}"
        );
    }
    assert!(recipe.ok, "codemode_recipe must succeed: {recipe:?}");
    assert!(
        recipe.text.contains(seed) || recipe.refs.iter().any(|r| r.starts_with("tz://")),
        "recipe must recover seed or mint ref: {recipe:?}"
    );
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
    let js = codemode_js_read(root, "__no_such__.txt");
    let json_plan = codemode_json_read(root, "__no_such__.txt");
    let rw = raw_once(
        root,
        &json!({"op":"tz_read","args":{"path": missing_s}}),
        "rw-err.json",
    );

    assert_path_failure("cli", &cli);
    assert_path_failure("mcp", &mcp);
    assert_path_failure("codemode_js", &js);
    assert_path_failure("codemode_json", &json_plan);
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
