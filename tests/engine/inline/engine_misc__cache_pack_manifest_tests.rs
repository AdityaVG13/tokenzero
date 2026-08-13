use super::*;

#[test]
fn cache_pack_manifest_replaces_atomically_without_temp_residue() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cache-packs/agent.json");

    write_cache_pack_manifest(&path, b"first\n").unwrap();
    write_cache_pack_manifest(&path, b"second\n").unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"second\n");
    let siblings = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(siblings, [std::ffi::OsString::from("agent.json")]);
}

#[test]
fn cache_pack_reports_manifest_publication_failure() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("cache-packs"), b"not a directory").unwrap();
    let mut config = EngineConfig::for_root(dir.path());
    config.cache_path = dir.path().join("recovery-cache.json");
    let response = TokenZeroEngine::new(config).cache_pack("agent");

    assert_eq!(response.status, "error");
    assert_eq!(
        response.error.as_ref().map(|error| error.code.as_str()),
        Some("manifest_write_failed")
    );
}
