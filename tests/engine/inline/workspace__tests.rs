use super::*;
use std::ffi::OsString;
use tempfile::tempdir;

fn env_of(pin: Option<&Path>, opt_in: bool) -> StoreEnv {
    StoreEnv::new(pin.map(OsString::from), opt_in)
}

#[test]
fn external_opted_in_pin_is_project_namespaced_in_cache_path() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let shared = tempdir().unwrap();

    let env = env_of(Some(shared.path()), true);
    let cache = default_recovery_cache_path_with_env(root, &env);

    let key = zero_store::project_key(root);
    // Hub resolution absolutizes/canonicalizes; match that spelling.
    let expected = shared
        .path()
        .canonicalize()
        .unwrap()
        .join("projects")
        .join(&key)
        .join("tokenzero")
        .join("recovery-cache.json");
    assert_eq!(cache, expected, "external pin must be project-namespaced");

    // The embedded handle must choose the same path for the same env.
    let store =
        tokenzero_recovery::embedded_store::TokenZeroStore::try_open_with_env(root, env).unwrap();
    assert_eq!(
        store.recovery().persistence_path.as_deref().unwrap(),
        &expected,
        "embedded and engine facade must agree on the namespaced cache path"
    );
    assert_eq!(store.store_mode(), Some("shared_namespaced"));
}

#[test]
fn local_zerostack_and_legacy_defaults_stay_compatible() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let canonical_root = root.canonicalize().unwrap();
    // Explicit empty env: never read live process store env in tests.
    let env = StoreEnv::new(None, false);

    // No marker, no pin: legacy per-repo directory.
    let legacy = default_recovery_cache_path_with_env(root, &env);
    assert_eq!(
        legacy,
        canonical_root
            .join(".tokenzero")
            .join("recovery-cache.json")
    );

    // Project-local .zerostack marker: unified engine file.
    std::fs::create_dir_all(root.join(".zerostack").join("tokenzero")).unwrap();
    let unified = default_recovery_cache_path_with_env(root, &env);
    assert_eq!(
        unified,
        canonical_root
            .join(".zerostack")
            .join("tokenzero")
            .join("recovery-cache.json")
    );
}
