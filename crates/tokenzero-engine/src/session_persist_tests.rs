use super::*;
use crate::session::{SeenState, ServeKey, ServedRecord, SessionMemory};
use std::time::SystemTime;

fn test_record(blob_ref: &str) -> ServedRecord {
    ServedRecord {
        content_sha256: "same-content".to_string(),
        blob_ref: blob_ref.to_string(),
        file_ref: blob_ref.to_string(),
        raw_tokens: 1,
        line_count: 1,
        byte_len: 4,
        served_at: SystemTime::UNIX_EPOCH,
        serve_count: 1,
    }
}

#[test]
fn v1_state_does_not_replay_v2_journal_into_seen_set() {
    let temp = tempfile::tempdir().unwrap();
    let state_path = temp.path().join("session-memory.json");
    let key = ServeKey::File {
        path: PathBuf::from("notes.txt"),
        start: None,
        end: None,
    };
    let record = test_record("tz://blob/missing");
    let state = SessionMemoryState {
        version: 1,
        scopes: BTreeMap::from([(
            "__user_global__".to_string(),
            PersistedScope {
                records: vec![PersistedRecordEntry {
                    key: key.clone(),
                    record: record.clone(),
                    seq: 1,
                }],
                rollup: SessionRollup::default(),
                session_hwm: 0,
            },
        )]),
    };
    fs::write(&state_path, serde_json::to_string(&state).unwrap()).unwrap();

    let delta = PersistedDelta {
        version: STATE_VERSION,
        scope_id: "__user_global__".to_string(),
        records: vec![PersistedRecordEntry {
            key: key.clone(),
            record,
            seq: 1,
        }],
        rollup: SessionRollup::default(),
        session_hwm: 1,
    };
    fs::write(
        session_journal_path(&state_path),
        format!("{}\n", serde_json::to_string(&delta).unwrap()),
    )
    .unwrap();

    let persisted = load_state(&state_path).unwrap();
    assert_eq!(persisted.version, 1);

    let persistence = SessionPersistence {
        path: state_path,
        cache_path: temp.path().join("cache.json"),
        scope_id: "__user_global__".to_string(),
        last_persisted: Arc::new(Mutex::new(None)),
    };
    let mut memory = SessionMemory::default();
    persistence.load_into(&mut memory);
    assert!(matches!(
        memory.lookup(&key, "same-content"),
        SeenState::Miss
    ));
}
