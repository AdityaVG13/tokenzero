use super::*;
use tempfile::tempdir;

fn anchor(path: &str) -> SpanAnchor {
    SpanAnchor {
        path: PathBuf::from(path),
        symbol: Some("parse args".to_string()),
        start_line: 3,
        end_line: 9,
    }
}

fn large(label: &str) -> String {
    (0..160)
        .map(|index| format!("{label}-{index}"))
        .collect::<Vec<_>>()
        .join(" ")
        + "
"
}

fn replacement_tokens(path: &str) -> usize {
    count_tokens(&format_ref_line(
        format!("tz://blob/{}", "0".repeat(64)),
        &anchor(path),
    ))
}

#[test]
fn under_budget_is_a_noop() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let body = large("resident");
    let mut set = WorkingSet::new(count_tokens(&body));
    let admission = set
        .admit(&mut store, body.clone(), anchor("src/a.rs"))
        .unwrap();

    assert!(admission.evicted.is_empty());
    assert_eq!(set.visible_lines(), vec![body.as_str()]);
    assert_eq!(set.telemetry(), WorkingSetTelemetry::default());
}

#[test]
fn over_budget_replaces_oldest_with_documented_ref_line() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let first = large("first");
    let second = large("second");
    let mut set = WorkingSet::new(count_tokens(&first) + count_tokens(&second) + 256);
    let first_admission = set
        .admit(&mut store, first.clone(), anchor("src/a file.rs"))
        .unwrap();
    set.admit(&mut store, second, anchor("src/b.rs")).unwrap();
    let third = set
        .admit(&mut store, large("third"), anchor("src/c.rs"))
        .unwrap();

    assert_eq!(third.evicted.len(), 1);
    assert_eq!(third.evicted[0].id, first_admission.id);
    assert_eq!(
        third.evicted[0].replacement,
        format!(
            r#"TZ-EVICT/1 ref={} path="src/a file.rs" symbol="parse args" lines=3-9"#,
            third.evicted[0].ref_id
        )
    );
    assert_eq!(set.visible_lines()[0], third.evicted[0].replacement);
    assert_eq!(
        set.telemetry(),
        WorkingSetTelemetry {
            evictions: 1,
            bytes_evicted: first.len() as u64,
            refs_created: 1,
        }
    );
}

#[test]
fn expand_round_trips_crlf_and_trailing_newline_byte_exactly() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let payload = "alpha
beta
gamma
"
    .repeat(80);
    let mut set = WorkingSet::new(replacement_tokens("src/crlf.txt"));
    let admission = set
        .admit(&mut store, payload.clone(), anchor("src/crlf.txt"))
        .unwrap();
    let evicted = &admission.evicted[0];
    let expanded = store.expand(&evicted.ref_id, None, None, None, None, None);

    assert!(expanded.found, "{}", expanded.reason);
    assert_eq!(expanded.content.as_bytes(), payload.as_bytes());
    assert!(expanded.content.ends_with(
        "
"
    ));
}

#[test]
fn touch_makes_recency_order_deterministic() {
    let dir = tempdir().unwrap();
    let mut store = RecoveryStore::new(Some(dir.path().join("recovery-cache.json")));
    let body = large("same");
    let mut set = WorkingSet::new(count_tokens(&body) * 2 + 256);
    let a = set
        .admit(&mut store, body.clone(), anchor("src/a.rs"))
        .unwrap();
    let b = set
        .admit(&mut store, body.clone(), anchor("src/b.rs"))
        .unwrap();
    assert!(set.touch(a.id));
    let admission = set.admit(&mut store, body, anchor("src/c.rs")).unwrap();

    assert_eq!(admission.evicted.len(), 1);
    assert_eq!(admission.evicted[0].id, b.id);
}
