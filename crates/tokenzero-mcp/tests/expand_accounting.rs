//! Integration test: expand responses carry Accounting.

use std::fs;
use std::path::Path;
use tempfile::TempDir;
use tokenzero_core::{Mode, ToolResponse};
use tokenzero_mcp::EngineConfig;
use tokenzero_mcp::TokenZeroEngine;

fn read_and_expand(dir: &Path, cache: &Path) -> (ToolResponse, ToolResponse) {
    let mut config = EngineConfig::for_root(dir);
    config.cache_path = cache.to_path_buf();
    let engine = TokenZeroEngine::new(config);

    let file = dir.join("test.txt");
    fs::write(&file, "hello world\nneedle in a haystack\n").unwrap();

    let read_resp = engine.read(&[file.clone()], Mode::Auto, None, None, false, 20, 4000);
    assert_eq!(read_resp.status, "ok", "read failed: {:?}", read_resp.error);
    assert!(
        read_resp.accounting.is_some(),
        "read response should have accounting"
    );

    let blob_ref = read_resp
        .refs
        .iter()
        .find(|r| r.kind == "blob")
        .expect("read response should have a blob ref")
        .ref_id
        .clone();
    assert!(
        blob_ref.starts_with("tz://"),
        "blob ref should start with tz://"
    );

    let expand_resp = engine.expand(&blob_ref, None, None, None, None, None);
    assert_eq!(
        expand_resp.status, "ok",
        "expand failed: {:?}",
        expand_resp.error
    );

    (read_resp, expand_resp)
}

#[test]
fn expand_response_has_accounting() {
    let dir = TempDir::new().unwrap();
    let cache = dir.path().join("recovery-cache.json");

    let (_read_resp, expand_resp) = read_and_expand(dir.path(), &cache);

    let accounting = expand_resp
        .accounting
        .as_ref()
        .expect("expand response should have Accounting");
    assert!(
        accounting.visible_tokens > 0,
        "expand should report non-zero visible tokens"
    );
    assert!(
        accounting.raw_tokens > 0,
        "expand should report non-zero raw tokens"
    );
    assert!(
        accounting.exact_ref_tokens.is_some(),
        "expand should report exact_ref_tokens"
    );
}

#[test]
fn expand_error_path_lacks_accounting() {
    let dir = TempDir::new().unwrap();
    let cache = dir.path().join("recovery-cache.json");

    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = cache.clone();
    let engine = TokenZeroEngine::new(config);

    let resp = engine.expand("tz://nonexistent/ref", None, None, None, None, None);

    assert_eq!(resp.status, "error");
    assert!(
        resp.accounting.is_none(),
        "error expand should have no accounting"
    );
}
