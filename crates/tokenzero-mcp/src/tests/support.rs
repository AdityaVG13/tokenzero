use super::*;

pub(crate) fn hunk(find: &str, replace: &str, replace_all: bool) -> EditHunk {
    EditHunk {
        find: find.to_string(),
        replace: replace.to_string(),
        replace_all,
    }
}

pub(crate) fn engine_with_backend(root: &Path, backend: SearchBackend) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(root);
    config.search_backend = backend;
    // Pin the PATH lookup regardless of ambient TOKENZERO_RG_PATH.
    config.rg_path_override = None;
    TokenZeroEngine::new(config)
}

pub(crate) fn search_backend_fixture(root: &Path) {
    fs::write(root.join("alpha.rs"), "fn alpha() {}\nlet needle = 1;\n").unwrap();
    fs::create_dir_all(root.join("sub")).unwrap();
    fs::write(
        root.join("sub/beta.rs"),
        "needle here\nno match\nneedle again\n",
    )
    .unwrap();
    fs::create_dir_all(root.join(".hidden")).unwrap();
    fs::write(root.join(".hidden/skip.rs"), "needle hidden\n").unwrap();
    fs::create_dir_all(root.join("target")).unwrap();
    fs::write(root.join("target/skip.rs"), "needle target\n").unwrap();
}

pub(crate) fn expanded_flat_output(engine: &TokenZeroEngine, response: &ToolResponse) -> String {
    let blob_ref = response
        .refs
        .iter()
        .find(|row| row.kind == "blob")
        .unwrap()
        .ref_id
        .clone();
    let expanded = engine.expand(&blob_ref, None, None, None, None, None);
    assert_eq!(expanded.status, "ok");
    expanded.visible.as_ref().unwrap().text.clone()
}
pub(crate) fn rg_or_skip(test: &str) -> bool {
    if find_rg_in_path().is_some() {
        return true;
    }
    eprintln!("skipping {test}: rg not found in PATH");
    false
}

pub(crate) fn dedup_fixture_content() -> String {
    (1..=40)
        .map(|index| {
            format!(
                "line {index:02}: session redundancy fixture content wide enough to out-cost a note"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub(crate) fn read_ok(engine: &TokenZeroEngine, file: &Path) -> ToolResponse {
    let response = engine.read(
        &[file.to_path_buf()],
        Mode::Auto,
        None,
        None,
        false,
        20,
        4000,
    );
    assert_eq!(response.status, "ok", "{:?}", response.error);
    response
}

pub(crate) fn visible_text(response: &ToolResponse) -> String {
    response.visible.as_ref().unwrap().text.clone()
}

pub(crate) fn visible_tokens(response: &ToolResponse) -> usize {
    response.accounting.as_ref().unwrap().visible_tokens
}

/// Create a tempdir + default engine. The `TempDir` must be kept alive
/// for the duration of the test; the engine borrows its path.
pub(crate) fn test_engine() -> (tempfile::TempDir, TokenZeroEngine) {
    let dir = tempfile::tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig::for_root(dir.path()));
    engine.mark_lifecycle_ready_for_tests();
    (dir, engine)
}

/// Create an engine whose recovery cache path is a directory (unwritable
/// as a file), forcing every tool into degraded mode.
pub(crate) fn engine_with_unwritable_cache(root: &Path) -> TokenZeroEngine {
    let cache_dir = root.join("cache-as-directory");
    fs::create_dir_all(&cache_dir).unwrap();
    let mut config = EngineConfig::for_root(root);
    config.cache_path = cache_dir;
    TokenZeroEngine::new(config)
}

/// Parse a JSON-RPC response string into a serde_json::Value.
pub(crate) fn response_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|e| panic!("invalid JSON-RPC response: {e}\n{raw}"))
}

/// Assert that a parsed JSON-RPC response carries an error with the
/// expected code and optional `data.error_type`. Returns the `data` object
/// for further site-specific assertions.
pub(crate) fn assert_structured_error<'a>(
    parsed: &'a Value,
    expected_code: i64,
    expected_error_type: Option<&str>,
) -> &'a Value {
    assert_eq!(
        parsed["error"]["code"], expected_code,
        "unexpected error code in {parsed:#}"
    );
    let data = &parsed["error"]["data"];
    if let Some(et) = expected_error_type {
        assert_eq!(data["error_type"], et, "unexpected error_type in {data:#}");
    }
    data
}
