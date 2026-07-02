#![allow(missing_docs)] // benchmark/integration-test harness: not public API
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_state_service::StateStore;
use neo_storage::{
    DataCache, StorageItem, StorageKey,
    mdbx::MdbxStoreProvider,
    persistence::{Store, storage::StorageConfig},
    rocksdb::RocksDBStoreProvider,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

fn make_snapshot(change_count: usize) -> DataCache {
    let snapshot = DataCache::new(false);

    for index in 0..change_count {
        let key = StorageKey::new(5, (index as u32).to_le_bytes().to_vec());
        let value = StorageItem::from_bytes(vec![index as u8, 0xAA, 0x55]);
        snapshot.add(key, value);
    }

    snapshot
}

static ROCKSDB_BENCH_SEQ: AtomicU64 = AtomicU64::new(0);
static MDBX_BENCH_SEQ: AtomicU64 = AtomicU64::new(0);

struct BenchTempDir {
    path: PathBuf,
}

impl BenchTempDir {
    fn new(prefix: &str, sequence: &AtomicU64) -> Self {
        let path = std::env::temp_dir().join(format!(
            "{prefix}-{}-{}",
            std::process::id(),
            sequence.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }
}

impl Drop for BenchTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn make_rocksdb_state_store() -> (StateStore, BenchTempDir) {
    let tempdir = BenchTempDir::new("neo-state-service-rocksdb-bench", &ROCKSDB_BENCH_SEQ);
    let backing: Arc<dyn Store> = Arc::new(
        RocksDBStoreProvider::new(StorageConfig {
            path: tempdir.path.clone(),
            ..Default::default()
        })
        .with_read_ahead(true)
        .get_rocksdb_store("")
        .expect("open rocksdb"),
    );
    (
        StateStore::with_mpt_store(false, backing).expect("rocksdb-backed state store"),
        tempdir,
    )
}

fn make_mdbx_state_store() -> (StateStore, BenchTempDir) {
    let tempdir = BenchTempDir::new("neo-state-service-mdbx-bench", &MDBX_BENCH_SEQ);
    let backing: Arc<dyn Store> = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: tempdir.path.clone(),
            mdbx_geometry_upper_bytes: Some(8 * 1024 * 1024 * 1024),
            mdbx_geometry_growth_bytes: Some(64 * 1024 * 1024),
            ..Default::default()
        })
        .get_mdbx_store("")
        .expect("open mdbx"),
    );
    (
        StateStore::with_mpt_store(false, backing).expect("mdbx-backed state store"),
        tempdir,
    )
}

fn bench_state_service_apply_snapshot_changes(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_service/apply_snapshot_changes");
    for &change_count in &[1usize, 10, 100, 500, 2000] {
        let snapshot = make_snapshot(change_count);
        group.bench_function(format!("{change_count}_changes"), |b| {
            let store = StateStore::with_mpt(false);
            let mut next_index = 0u32;
            b.iter(|| {
                let index = next_index;
                next_index = next_index.wrapping_add(1);
                black_box(store.apply_snapshot_changes(index, &snapshot).unwrap());
            });
        });
    }
    group.finish();

    let mut rocksdb_group = c.benchmark_group("state_service/apply_snapshot_changes_rocksdb");
    for &change_count in &[1usize, 10, 100, 500, 2000] {
        let snapshot = make_snapshot(change_count);
        rocksdb_group.bench_function(format!("{change_count}_changes"), |b| {
            let (store, _tempdir) = make_rocksdb_state_store();
            let mut next_index = 0u32;
            b.iter(|| {
                let index = next_index;
                next_index = next_index.wrapping_add(1);
                black_box(store.apply_snapshot_changes(index, &snapshot).unwrap());
            });
        });
    }
    rocksdb_group.finish();

    let mut mdbx_group = c.benchmark_group("state_service/apply_snapshot_changes_mdbx");
    for &change_count in &[1usize, 10, 100, 500, 2000] {
        let snapshot = make_snapshot(change_count);
        mdbx_group.bench_function(format!("{change_count}_changes"), |b| {
            let (store, _tempdir) = make_mdbx_state_store();
            let mut next_index = 0u32;
            b.iter(|| {
                let index = next_index;
                next_index = next_index.wrapping_add(1);
                black_box(store.apply_snapshot_changes(index, &snapshot).unwrap());
            });
        });
    }
    mdbx_group.finish();
}

criterion_group!(benches, bench_state_service_apply_snapshot_changes,);
criterion_main!(benches);
