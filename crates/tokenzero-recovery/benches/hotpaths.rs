//! Microbenchmarks for TokenZero recovery hot-path functions.
//!
//! Run with: cargo bench -p tokenzero-recovery

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use tempfile::TempDir;
use tokenzero_core::ContentType;
use tokenzero_recovery::{RecoveryConfig, RecoveryStore};

fn bench_persist(c: &mut Criterion) {
    let dir = TempDir::new().expect("temp dir");

    // Persist empty state (exercises lock + write + dir sync)
    c.bench_function("persist_empty", |b| {
        b.iter(|| {
            let path = dir.path().join(format!(
                "bench_empty_{}_{}.json",
                std::process::id(),
                black_box(0usize)
            ));
            let mut store = RecoveryStore::with_config(Some(path), RecoveryConfig::default());
            store.store_payload_deferred("key", ContentType::Code, None, None, None);
            black_box(store.persist_pending())
        })
    });

    // Persist with merge (exercises lock + load + merge + write + dir sync)
    c.bench_function("persist_merge", |b| {
        let path = dir
            .path()
            .join(format!("bench_merge_{}.json", std::process::id()));
        // Seed the file so persist exercises the merge path
        {
            let mut store =
                RecoveryStore::with_config(Some(path.clone()), RecoveryConfig::default());
            store.store_payload_deferred("existing", ContentType::Code, None, None, None);
            store.persist_pending().expect("seed persist");
        }
        b.iter(|| {
            let mut store =
                RecoveryStore::with_config(Some(path.clone()), RecoveryConfig::default());
            store.store_payload_deferred("key", ContentType::Code, None, None, None);
            black_box(store.persist_pending())
        })
    });
}

criterion_group!(benches, bench_persist);
criterion_main!(benches);
