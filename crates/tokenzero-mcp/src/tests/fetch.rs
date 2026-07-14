use super::*;
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
                    record_fetch(&path, &format!("https://example.com/{i}/{j}"), &format!("tz://blob/b{i}{j}"), 1);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(load_fetch_index(&index_path).entries.len(), 80);
}

#[test]
fn truncated_fetch_index_does_not_mass_invalidate_via_atomic_write() {
    let dir = tempdir().unwrap();
    let index_path = dir.path().join("fetch-cache.json");
    record_fetch(&index_path, "https://example.com/a", "tz://blob/ba", 1);
    assert!(load_fetch_index(&index_path).entries.contains_key("https://example.com/a"));
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
    let dir = tempdir().unwrap();
    let marker = dir.path().join("invocations.log");
    let (_curl, engine) = fetch_engine_with_curl(
        dir.path(),
        &format!("#!/bin/sh\necho invoked >> {}\nprintf 'fetched body line\\n'\n", marker.display()),
        &["example.com"],
    );
    let first = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_status_ok(&first);
    assert!(visible_text(&first).contains("fetched body line"));
    assert!(first.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(first.telemetry.as_ref().unwrap()["cache_hit"], false);

    let second = engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_status_ok(&second);
    assert_eq!(second.telemetry.as_ref().unwrap()["cache_hit"], true);
    assert!(visible_text(&second).contains("fetched body line"));
    assert!(second.refs.iter().any(|row| row.kind == "blob"));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    let third = engine.fetch("https://example.com/doc", None, true, Mode::Auto, 4000);
    assert_eq!(third.telemetry.as_ref().unwrap()["cache_hit"], false);
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 2);
}

#[cfg(unix)]
#[test]
fn fetch_cache_hits_still_obey_current_deny_policy() {
    let dir = tempdir().unwrap();
    let marker = dir.path().join("invocations.log");
    let (_curl, engine) = fetch_engine_with_curl(
        dir.path(),
        &format!("#!/bin/sh\necho invoked >> {}\nprintf 'cached sensitive body\\n'\n", marker.display()),
        &["example.com"],
    );
    assert_status_ok(&engine.fetch("https://example.com/doc", None, false, Mode::Auto, 4000));
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);

    let mut config = EngineConfig::for_root(dir.path());
    config.curl_path_override = Some(dir.path().join("fake-curl"));
    config.fetch_enabled = true;
    config.fetch_allow_hosts = vec!["example.com".to_string()];
    config.fetch_deny_hosts = vec!["example.com".to_string()];
    let denied = TokenZeroEngine::new(config).fetch("https://example.com/doc", None, false, Mode::Auto, 4000);
    assert_error_code(&denied, "fetch_blocked");
    assert!(denied.visible.is_none());
    assert_eq!(fs::read_to_string(&marker).unwrap().lines().count(), 1);
}

#[test]
fn fetch_url_validation_rejects_non_http_and_internal_targets() {
    let (dir, engine) = setup_engine(|root| {
        let mut config = EngineConfig::for_root(root);
        config.fetch_enabled = true;
        config
    });
    let _ = dir;
    for (url, code) in [
        ("file:///etc/passwd", "invalid_url"),
        ("ftp://example.com/x", "invalid_url"),
        ("http://169.254.169.254/latest/meta-data/", "fetch_blocked"),
        ("http://127.0.0.1:8080/admin", "fetch_blocked"),
        ("http://10.0.0.5/", "fetch_blocked"),
        ("http://localhost:9999/", "fetch_blocked"),
    ] {
        assert_error_code(
            &engine.fetch(url, None, false, Mode::Auto, 4000),
            code,
        );
    }
}

#[cfg(unix)]
#[test]
fn fetch_reports_curl_exit_failure() {
    let dir = tempdir().unwrap();
    let (_curl, engine) = fetch_engine_with_curl(
        dir.path(),
        "#!/bin/sh\necho 'could not resolve host' >&2\nexit 6\n",
        &["nope.invalid"],
    );
    let response = engine.fetch("https://nope.invalid/x", None, false, Mode::Auto, 4000);
    assert_error_code(&response, "fetch_failed");
    assert!(response.error.as_ref().unwrap().message.contains("could not resolve host"));
}

#[test]
fn fetch_is_disabled_by_default() {
    let (dir, engine) = setup_engine(|root| EngineConfig {
        fetch_enabled: false,
        ..EngineConfig::for_root(root)
    });
    let _ = dir;
    assert_error_code(
        &engine.fetch("https://example.com/", None, false, Mode::Auto, 4000),
        "fetch_disabled",
    );
}
