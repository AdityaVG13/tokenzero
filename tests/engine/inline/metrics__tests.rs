use super::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn concurrent_metrics_sidecars_keep_every_increment() {
    let dir = tempdir().unwrap();
    let cache = dir.path().join("recovery-cache.json");
    let a = Arc::new(ToolMetrics::new(&cache));
    let b = Arc::new(ToolMetrics::new(&cache));
    const N: u64 = 40;
    let ta = {
        let metrics = Arc::clone(&a);
        thread::spawn(move || {
            for _ in 0..N {
                metrics.record("read", Duration::from_millis(1), false);
                metrics.flush_persisted();
            }
        })
    };
    let tb = {
        let metrics = Arc::clone(&b);
        thread::spawn(move || {
            for _ in 0..N {
                metrics.record("read", Duration::from_millis(1), false);
                metrics.flush_persisted();
            }
        })
    };
    ta.join().unwrap();
    tb.join().unwrap();
    a.flush_persisted();
    b.flush_persisted();
    let loaded = load_persisted_from_path(&cache.with_file_name("tool-metrics.json"));
    assert_eq!(
        loaded.get("read").map(|stat| stat.calls),
        Some(N * 2),
        "P1 and P2 ToolMetrics instances must flock the same sidecar so RMW cannot drop a family"
    );
}
