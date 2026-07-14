use super::*;

pub(crate) fn hunk(find: &str, replace: &str, replace_all: bool) -> EditHunk {
    EditHunk { find: find.to_string(), replace: replace.to_string(), replace_all }
}

pub(crate) fn engine_with_backend(root: &Path, backend: SearchBackend) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.search_backend = backend;
    config.rg_path_override = None; // pin PATH lookup regardless of ambient TOKENZERO_RG_PATH
    TokenZeroEngine::new(config)
}

pub(crate) fn search_backend_fixture(root: &Path) {
    fs::write(root.join("alpha.rs"), "fn alpha() {}
let needle = 1;
").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(root.join("sub/beta.rs"), "needle here
no match
needle again
").unwrap();
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/skip.rs"), "needle hidden
").unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target/skip.rs"), "needle target
").unwrap();
}

pub(crate) fn expanded_flat_output(engine: &TokenZeroEngine, response: &ToolResponse) -> String {
    let expanded = engine.expand(&blob_ref(response), None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    expanded.visible.as_ref().unwrap().text.clone()
}

pub(crate) fn rg_or_skip(test: &str) -> bool {
    if find_rg_in_path().is_some() { return true; }
    eprintln!("skipping {test}: rg not found in PATH");
    false
}

pub(crate) fn dedup_fixture_content() -> String {
    (1..=40).map(|i| format!("line {i:02}: session redundancy fixture content wide enough to out-cost a note")).collect::<Vec<_>>().join("
") + "
"
}

pub(crate) fn read_ok(engine: &TokenZeroEngine, file: &Path) -> ToolResponse {
    let response = engine.read(&[file.to_path_buf()], Mode::Auto, None, None, false, 20, 4000);
    assert_eq!(response.status, "ok", "{:?}", response.error);
    response
}

pub(crate) fn visible_text(response: &ToolResponse) -> String { response.visible.as_ref().unwrap().text.clone() }

pub(crate) fn visible_tokens(response: &ToolResponse) -> usize { response.accounting.as_ref().unwrap().visible_tokens }

pub(crate) fn default_engine(root: &Path) -> TokenZeroEngine { TokenZeroEngine::new(EngineConfig::for_root(root)) }

pub(crate) fn setup_default() -> (tempfile::TempDir, TokenZeroEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = default_engine(dir.path());
    (dir, engine)
}

pub(crate) fn setup_engine(configure: impl FnOnce(&Path) -> EngineConfig) -> (tempfile::TempDir, TokenZeroEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = TokenZeroEngine::new(configure(dir.path()));
    (dir, engine)
}

pub(crate) fn setup_file(name: &str, content: impl AsRef<[u8]>) -> (tempfile::TempDir, PathBuf, TokenZeroEngine) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    fs::write(&path, content).unwrap();
    let engine = default_engine(dir.path());
    (dir, path, engine)
}

pub(crate) fn setup_dedup(name: &str) -> (tempfile::TempDir, PathBuf, TokenZeroEngine, String) {
    let content = dedup_fixture_content();
    let (dir, path, engine) = setup_file(name, &content);
    (dir, path, engine, content)
}

pub(crate) fn setup_dedup_off(name: &str) -> (tempfile::TempDir, PathBuf, TokenZeroEngine, String) {
    let content = dedup_fixture_content();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, &content).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    (dir, path, TokenZeroEngine::new(config), content)
}

pub(crate) fn engine_with_unwritable_cache(root: &Path) -> TokenZeroEngine {
    let cache_dir = root.join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(root);
    config.cache_path = cache_dir;
    TokenZeroEngine::new(config)
}

pub(crate) fn setup_unwritable(name: &str, content: impl AsRef<[u8]>) -> (tempfile::TempDir, PathBuf, TokenZeroEngine) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    fs::write(&path, content).unwrap();
    let engine = engine_with_unwritable_cache(dir.path());
    (dir, path, engine)
}

pub(crate) fn blob_ref(response: &ToolResponse) -> String {
    response
        .refs
        .iter()
        .find(|row| row.kind == "blob")
        .unwrap_or_else(|| panic!("missing blob ref in {:?}", response.refs))
        .ref_id
        .clone()
}

pub(crate) fn ref_of(response: &ToolResponse, kind: &str) -> String {
    response
        .refs
        .iter()
        .find(|row| row.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind} ref in {:?}", response.refs))
        .ref_id
        .clone()
}

pub(crate) fn expand_raw(engine: &TokenZeroEngine, ref_id: &str) -> ToolResponse { engine.expand(ref_id, Some("raw"), None, None, None, None) }

pub(crate) fn expand_ok(engine: &TokenZeroEngine, ref_id: &str) -> String {
    let expanded = expand_raw(engine, ref_id);
    assert_eq!(expanded.status, "ok", "{:?}", expanded.error);
    expanded.visible.as_ref().unwrap().text.clone()
}

pub(crate) fn ingest_blob(engine: &TokenZeroEngine, text: &str, source: &str) -> String {
    blob_ref(&engine.ingest(text, tokenzero_core::ContentType::Unknown, Mode::Exact, source))
}

pub(crate) fn response_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("invalid JSON-RPC response: {e}
{raw}"))
}

pub(crate) fn assert_structured_error<'a>(parsed: &'a Value, expected_code: i64, expected_error_type: Option<&str>) -> &'a Value {
    assert_eq!(parsed["error"]["code"], expected_code, "unexpected error code in {parsed:#}");
    let data = &parsed["error"]["data"];
    if let Some(et) = expected_error_type {
        assert_eq!(data["error_type"], et, "unexpected error_type in {data:#}");
    }
    data
}

pub(crate) fn rpc(engine: &TokenZeroEngine, raw: &str) -> Value { response_json(&handle_jsonrpc(engine, raw).unwrap()) }

pub(crate) fn rpc_json(engine: &TokenZeroEngine, request: &Value) -> Value { rpc(engine, &request.to_string()) }

pub(crate) fn tools_call(engine: &TokenZeroEngine, id: Value, name: &str, arguments: Value) -> Value {
    rpc_json(
        engine,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }),
    )
}

pub(crate) fn tools_list(engine: &TokenZeroEngine, id: i64) -> Value {
    rpc(engine, &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{}}}}"#))
}

pub(crate) fn resource_read(engine: &TokenZeroEngine, id: i64, uri: &str) -> Value {
    rpc(
        engine,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"resources/read","params":{{"uri":"{uri}"}}}}"#
        ),
    )
}

pub(crate) fn find_tool_by_name<'a>(tools: &'a [Value], name: &str) -> &'a Value {
    tools.iter().find(|tool| tool["name"] == name).unwrap_or_else(|| panic!("tool {name} not found in tools list"))
}

pub(crate) fn find_tool<'a>(listed: &'a Value, name: &str) -> &'a Value {
    find_tool_by_name(listed["result"]["tools"].as_array().unwrap(), name)
}

pub(crate) fn assert_no_schema_combinators(listed: &Value) {
    for tool in listed["result"]["tools"].as_array().unwrap() {
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "tool {}", tool["name"]);
        for key in ["anyOf", "oneOf", "allOf"] {
            assert!(schema.get(key).is_none(), "tool {} advertises top-level {key}", tool["name"]);
        }
    }
}

pub(crate) fn codemode_engine(root: &Path) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.tool_surface = tokenzero_core::McpToolSurface::CodeMode;
    TokenZeroEngine::new(config)
}

#[cfg(unix)]
pub(crate) fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
pub(crate) fn fetch_engine_with_curl(root: &Path, curl_body: &str, allow: &[&str]) -> (PathBuf, TokenZeroEngine) {
    let fake_curl = root.join("fake-curl");
    write_executable(&fake_curl, curl_body);
    let mut config = EngineConfig::for_root(root);
    config.curl_path_override = Some(fake_curl.clone());
    config.fetch_enabled = true;
    config.fetch_allow_hosts = allow.iter().map(|s| (*s).to_string()).collect();
    (fake_curl, TokenZeroEngine::new(config))
}

pub(crate) fn assert_status_ok(response: &ToolResponse) { assert_eq!(response.status, "ok", "{:?}", response.error); }

pub(crate) fn assert_error_code(response: &ToolResponse, code: &str) {
    assert_eq!(response.status, "error");
    assert_eq!(response.error.as_ref().unwrap().code, code);
}

pub(crate) fn mem_status(engine: &TokenZeroEngine) -> Value { serde_json::from_str(&engine.mem().visible.unwrap().text).unwrap() }

pub(crate) fn seed_blob_ref(engine: &TokenZeroEngine, file: &Path) -> String {
    read_ok(engine, file).refs.iter().find(|r| r.kind == "blob" || r.kind == "file").expect("read emits a recovery ref").ref_id.clone()
}

pub(crate) fn shell_ok(engine: &TokenZeroEngine, command: &str, argv: Option<Vec<String>>, cwd: Option<&Path>) -> ToolResponse {
    let response = engine.shell(command, argv, cwd, Mode::Auto, None, false, None, None, None);
    assert_status_ok(&response);
    response
}

pub(crate) fn shell_ok_exact(engine: &TokenZeroEngine, command: &str, argv: Option<Vec<String>>, cwd: Option<&Path>, env: Option<std::collections::BTreeMap<String, String>>) -> ToolResponse {
    let response = engine.shell(command, argv, cwd, Mode::Auto, None, true, env, None, None);
    assert_status_ok(&response);
    response
}

pub(crate) fn recovery_state_json(blobs: Value, order: Value) -> Value {
    json!({
        "version": 1, "max_blobs": 1024, "max_files": 1024, "max_units": 1024,
        "max_search_hits": 1024, "max_bytes": 64 * 1024 * 1024,
        "blobs": blobs, "files": {}, "units": {}, "search_hits": {},
        "aliases": {}, "order": order, "shell_outcomes": {}, "shell_outcome_seq": 0
    })
}
