use super::*;
use tempfile::tempdir;

fn engine_for_fetch(dir: &tempfile::TempDir, enabled: bool) -> TokenZeroEngine {
    let mut config = EngineConfig::for_root(dir.path());
    config.session_dedup = false;
    config.fetch_enabled = enabled;
    TokenZeroEngine::new(config)
}

#[test]
fn fetch_rejects_non_http_url() {
    let dir = tempdir().unwrap();
    let engine = engine_for_fetch(&dir, true);
    let response = engine.fetch("ftp://example.com/a", None, false, Mode::Auto, 4000);
    let error = response.error.expect("non-http URL must fail");
    assert_eq!(error.code, "invalid_url");
    assert!(error.message.contains("http(s) URL"), "{}", error.message);
}

#[test]
fn fetch_disabled_by_default_config() {
    let dir = tempdir().unwrap();
    let engine = engine_for_fetch(&dir, false);
    let response = engine.fetch("https://example.com/", None, false, Mode::Auto, 4000);
    let error = response.error.expect("disabled fetch must fail");
    assert_eq!(error.code, "fetch_disabled");
    assert!(
        error.message.contains("disabled by default"),
        "{}",
        error.message
    );
}
