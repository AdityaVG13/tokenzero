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
