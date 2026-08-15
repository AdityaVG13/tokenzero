use super::*;
use tempfile::tempdir;

#[test]
fn tzgd0b_admit_refuses_newer_major_and_degrades_older_minor() {
    let current = StoreSchemaVersion::CURRENT.stamp();
    assert_eq!(admit_store_schema(&current).unwrap(), SchemaAdmit::Accept);

    let reader = StoreSchemaVersion { major: 1, minor: 1 };
    let older_minor = StoreSchemaStamp {
        schema: STORE_SCHEMA_NAME,
        major: 1,
        minor: 0,
    };
    assert_eq!(
        admit_store_schema_against(&older_minor, reader).unwrap(),
        SchemaAdmit::DegradeMinor
    );

    let newer_minor = StoreSchemaStamp {
        schema: STORE_SCHEMA_NAME,
        major: STORE_SCHEMA_MAJOR,
        minor: STORE_SCHEMA_MINOR.saturating_add(1),
    };
    assert_eq!(
        admit_store_schema(&newer_minor).unwrap(),
        SchemaAdmit::DegradeMinor
    );

    let older_major = StoreSchemaStamp {
        schema: STORE_SCHEMA_NAME,
        major: 0,
        minor: 9,
    };
    assert_eq!(
        admit_store_schema(&older_major).unwrap(),
        SchemaAdmit::DegradeOlderMajor
    );

    let newer_major = StoreSchemaStamp {
        schema: STORE_SCHEMA_NAME,
        major: STORE_SCHEMA_MAJOR + 1,
        minor: 0,
    };
    let err = admit_store_schema(&newer_major).unwrap_err();
    assert!(
        matches!(err, SchemaSkewError::NewerMajor { found } if found.major == STORE_SCHEMA_MAJOR + 1)
    );
    assert!(err.to_string().contains("refuse newer major"));
}

#[test]
fn tzgd0b_shadow_jsonl_is_a_fixed_ring() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("shadow.jsonl");
    for idx in 0..(SHADOW_JSONL_RING_CAP + 40) {
        append_shadow_jsonl(&path, &format!("{{\"n\":{idx}}}")).unwrap();
    }
    let lines: Vec<String> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), SHADOW_JSONL_RING_CAP);
    assert_eq!(lines[0], format!("{{\"n\":{}}}", 40));
    assert_eq!(
        lines.last().unwrap(),
        &format!("{{\"n\":{}}}", SHADOW_JSONL_RING_CAP + 39)
    );
}

#[test]
fn tzgd0b_torn_actioncache_write_is_not_promoted() {
    let dir = tempdir().unwrap();
    let dest = dir.path().join("actions").join("seg-1.json");
    let payload = br#"{"schema":"tokenzero.store","major":1,"minor":0,"k":1}"#;
    write_actioncache_segment(&dest, payload).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), payload);

    // Crash before commit: leftover unique tmp without marker is discarded.
    let tmp = unique_tmp_path(&dest);
    fs::remove_file(&dest).unwrap();
    fs::write(&tmp, b"torn-partial").unwrap();
    assert!(recover_actioncache_segment(&dest).unwrap().is_none());
    assert!(!tmp.exists());
    assert!(!dest.exists());

    // Commit marker present: unique temp is promoted.
    let tmp = unique_tmp_path(&dest);
    let commit = commit_for_tmp(&tmp);
    fs::write(&tmp, payload).unwrap();
    fs::write(&commit, hex_sha256(payload)).unwrap();
    let recovered = recover_actioncache_segment(&dest).unwrap().unwrap();
    assert_eq!(recovered, dest);
    assert_eq!(fs::read(&dest).unwrap(), payload);
    assert!(!commit.exists());
}
