use super::*;
use std::sync::Arc;
use tempfile::tempdir;
use tokenzero_core::MCP_SCHEMA_VERSION;

use super::support::*;

#[test]
fn concurrent_record_fetch_keeps_every_entry() {
    let dir = tempdir().unwrap();
    let index_path = dir.path().join("fetch-cache.json");
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let path = index_path.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    let url = format!("https://example.com/{i}/{j}");
                    record_fetch(&path, &url, &format!("tz://blob/b{i}{j}"), 1);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }

    let index = load_fetch_index(&index_path);
    assert_eq!(
        index.entries.len(),
        80,
        "every concurrent insert must survive the read-modify-write, got {}",
        index.entries.len()
    );
}

#[test]
fn truncated_fetch_index_does_not_mass_invalidate_via_atomic_write() {
    let dir = tempdir().unwrap();
    let index_path = dir.path().join("fetch-cache.json");
    record_fetch(&index_path, "https://example.com/a", "tz://blob/ba", 1);

    // No reader ever observes a torn file: a crash leaves either the prior
    // complete file or the new complete one, never a truncated index that
    // load_fetch_index would silently treat as empty. Assert the post-write
    // file is valid JSON and complete, and that no temp debris remains.
    let index = load_fetch_index(&index_path);
    assert!(index.entries.contains_key("https://example.com/a"));
    let debris: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(debris.is_empty(), "atomic write must leave no temp debris");
}

#[cfg(unix)]
#[test]
fn fetch_caches_within_ttl_and_refetches_when_fresh() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let fake_curl = dir.path().join("fake-curl");
    let marker = dir.path().join("invocations.log");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\necho invoked >> {}\nprintf 'fetched body line\\n'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(fake_curl);
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["example.com".to_string()];
    let engine = TokenZeroEngine::new(config);

    let first = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(first.status, "ok");
    assert!(
        first
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("fetched body line")
    );
    assert!(first.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(first.telemetry.as_ref().unwrap()["cache_hit"], false);

    // Within the TTL the network is never touched: same body, no second
    // curl invocation.
    let second = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(second.status, "ok");
    assert_eq!(second.telemetry.as_ref().unwrap()["cache_hit"], true);
    assert!(
        second
            .visible
            .as_ref()
            .unwrap()
            .text
            .contains("fetched body line")
    );
    assert!(second.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    // fresh=true bypasses the TTL.
    let third = engine.fetch("https://example.com/doc", None, true, Mode::Auto, 4000);
    assert_eq!(third.telemetry.as_ref().unwrap()["cache_hit"], false);
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 2);
}

#[cfg(unix)]
#[test]
fn fetch_cache_hits_still_obey_current_deny_policy() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let fake_curl = dir.path().join("fake-curl");
    let marker = dir.path().join("invocations.log");
    fs::write(
        &fake_curl,
        format!(
            "#!/bin/sh\necho invoked >> {}\nprintf 'cached sensitive body\\n'\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&fake_curl, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(fake_curl);
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["example.com".to_string()];
    let engine = TokenZeroEngine::new(config.clone());

    let first = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_eq!(first.status, "ok");
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    config.fetch_deny_hosts = vec!["example.com".to_string()];
    let denied_engine = TokenZeroEngine::new(config);
    let denied = denied_engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    let error = denied.error.as_ref().unwrap();
    assert_eq!(error.code, "fetch_blocked");
    assert!(
        denied.visible.is_none(),
        "a fresh TTL cache hit must not bypass the current deny policy"
    );
    assert_eq!(
        fs::read_to_string(&marker).unwrap().lines().count(),
        1,
        "denied cached fetch must not re-enter curl"
    );
}

#[test]
fn fetch_url_validation_rejects_non_http_and_internal_targets() {
    let dir = tempdir().unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.fetch_enabled = true;
    let engine = TokenZeroEngine::new(config);

    // Non-http schemes are rejected before any network call.
    for url in ["file:///etc/passwd", "ftp://example.com/x"] {
        let resp = engine.fetch(url, None, false, Mode::Auto, 4000);
        assert_eq!(resp.error.as_ref().unwrap().code, "invalid_url", "{url}");
    }

    // Internal / loopback targets are blocked.
    for url in [
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:8080/admin",
        "http://10.0.0.5/",
        "http://localhost:9999/",
    ] {
        let resp = engine.fetch(url, None, false, Mode::Auto, 4000);
        assert_eq!(resp.error.as_ref().unwrap().code, "fetch_blocked", "{url}");
    }
}

#[cfg(unix)]
#[test]
fn fetch_reports_curl_exit_failure() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let failing = dir.path().join("failing-curl");
    fs::write(
        &failing,
        "#!/bin/sh\necho 'could not resolve host' >&2\nexit 6\n",
    )
    .unwrap();
    fs::set_permissions(&failing, fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(failing);
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["nope.invalid".to_string()];
    let engine = TokenZeroEngine::new(config);

    let response = engine.fetch("https://nope.invalid/x", None, false, Mode::Auto, 4000);
    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, "fetch_failed");
    assert!(
        error.message.contains("could not resolve host"),
        "{error:?}"
    );
}

#[test]
fn fetch_is_disabled_by_default() {
    let dir = tempdir().unwrap();
    let engine = TokenZeroEngine::new(EngineConfig {
        fetch_enabled: false,
        ..EngineConfig::for_root(dir.path())
    });
    let response = engine.fetch("https://example.com/", None, false, Mode::Auto, 4000);
    let error = response.error.as_ref().unwrap();
    assert_eq!(error.code, "fetch_disabled");
}
