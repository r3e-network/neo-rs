#![allow(missing_docs)] // benchmark harness: not public API
use criterion::{BenchmarkId, Throughput};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_storage::mdbx::{MdbxStore, MdbxStoreProvider};
use neo_storage::{
    StorageItem, StorageKey, StorageResult,
    persistence::{
        RawReadOnlyStore, ReadOnlyStore, ReadOnlyStoreGeneric, SeekDirection, Store, StoreCache,
        WriteStore, storage::StorageConfig,
    },
    rocksdb::{RocksDBStoreProvider, RocksDbStore},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static ROCKSDB_BENCH_SEQ: AtomicU64 = AtomicU64::new(0);

struct BenchTempDir {
    path: PathBuf,
}

impl BenchTempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "neo-storage-commit-bench-{label}-{}-{}",
            std::process::id(),
            ROCKSDB_BENCH_SEQ.fetch_add(1, Ordering::Relaxed)
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

fn open_rocksdb(label: &str) -> (Arc<RocksDbStore>, BenchTempDir) {
    let tempdir = BenchTempDir::new(label);
    let store = Arc::new(
        RocksDBStoreProvider::new(StorageConfig {
            path: tempdir.path.clone(),
            ..Default::default()
        })
        .get_rocksdb_store("")
        .expect("open RocksDB"),
    );
    store.enable_fast_sync_mode();
    (store, tempdir)
}

fn open_mdbx(label: &str) -> (Arc<MdbxStore>, BenchTempDir) {
    let tempdir = BenchTempDir::new(label);
    let store = Arc::new(
        MdbxStoreProvider::new(StorageConfig {
            path: tempdir.path.clone(),
            ..Default::default()
        })
        .get_mdbx_store("")
        .expect("open MDBX"),
    );
    (store, tempdir)
}

#[derive(Debug)]
struct SnapshotOnlyStore {
    inner: Arc<RocksDbStore>,
}

impl SnapshotOnlyStore {
    fn new(inner: Arc<RocksDbStore>) -> Self {
        Self { inner }
    }
}

impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for SnapshotOnlyStore {
    type FindIterator<'a> =
        <RocksDbStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::FindIterator<'a>;

    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&Vec<u8>>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for SnapshotOnlyStore {
    type FindIterator<'a> =
        <RocksDbStore as ReadOnlyStoreGeneric<StorageKey, StorageItem>>::FindIterator<'a>;

    fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
        self.inner.try_get(key)
    }

    fn find(
        &self,
        key_prefix: Option<&StorageKey>,
        direction: SeekDirection,
    ) -> Self::FindIterator<'_> {
        self.inner.find(key_prefix, direction)
    }
}

impl WriteStore<Vec<u8>, Vec<u8>> for SnapshotOnlyStore {
    fn delete(&mut self, key: Vec<u8>) -> StorageResult<()> {
        let inner = Arc::make_mut(&mut self.inner);
        inner.delete(key)
    }

    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) -> StorageResult<()> {
        let inner = Arc::make_mut(&mut self.inner);
        inner.put(key, value)
    }
}

impl ReadOnlyStore for SnapshotOnlyStore {}

impl RawReadOnlyStore for SnapshotOnlyStore {
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner.try_get_bytes(key)
    }
}

impl Store for SnapshotOnlyStore {
    type Snapshot = <RocksDbStore as Store>::Snapshot;

    fn snapshot(&self) -> Arc<Self::Snapshot> {
        self.inner.snapshot()
    }

    fn flush(&self) -> StorageResult<()> {
        self.inner.flush()
    }

    fn backend_kind(&self) -> neo_storage::persistence::StoreBackendKind {
        self.inner.backend_kind()
    }

    fn rocksdb_batch_metrics(&self) -> Option<neo_storage::persistence::RocksDbBatchMetrics> {
        self.inner.rocksdb_batch_metrics()
    }

    fn supports_fast_sync_mode(&self) -> bool {
        self.inner.supports_fast_sync_mode()
    }

    fn enable_fast_sync_mode(&self) {
        self.inner.enable_fast_sync_mode();
    }

    fn disable_fast_sync_mode(&self) {
        self.inner.disable_fast_sync_mode();
    }

    fn discard_pending_fast_sync_writes(&self) {
        self.inner.discard_pending_fast_sync_writes();
    }

    fn has_pending_fast_sync_writes(&self) -> bool {
        self.inner.has_pending_fast_sync_writes()
    }

    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        self.inner.try_commit_raw_overlay(overlay)
    }

    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: neo_storage::persistence::RawOverlaySource + ?Sized,
    {
        self.inner.try_commit_borrowed_raw_overlay(overlay)
    }
}

fn stage_changes<S: Store>(cache: &mut StoreCache<S>, iteration: u64, change_count: usize) {
    for index in 0..change_count {
        let key = StorageKey::new(
            77,
            ((iteration as usize * change_count + index) as u64)
                .to_le_bytes()
                .to_vec(),
        );
        let value = StorageItem::from_bytes(vec![index as u8, 0xAA, 0x55, 0x11]);
        cache.add(key, value);
    }
}

fn stage_import_batch_changes<S: Store>(
    cache: &mut StoreCache<S>,
    batch: u64,
    blocks_per_batch: usize,
    changes_per_block: usize,
) {
    for block in 0..blocks_per_batch {
        for change in 0..changes_per_block {
            let ordinal =
                ((batch as usize * blocks_per_batch + block) * changes_per_block + change) as u64;
            let key = StorageKey::new(77, ordinal.to_le_bytes().to_vec());
            let value = StorageItem::from_bytes(vec![batch as u8, block as u8, change as u8]);
            cache.add(key, value);
        }
    }
}

fn black_box_unit<E: std::fmt::Debug>(result: Result<(), E>) {
    result.unwrap();
    black_box(());
}

fn bench_store_cache_commit(c: &mut Criterion) {
    let mut direct_group = c.benchmark_group("storage/store_cache_commit_rocksdb_direct");
    for &change_count in &[1usize, 10, 100, 500] {
        direct_group.bench_function(format!("{change_count}_changes"), |b| {
            let (store, _tempdir) = open_rocksdb("direct");
            let mut iteration = 0u64;
            b.iter(|| {
                let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
                stage_changes(&mut cache, iteration, change_count);
                iteration = iteration.wrapping_add(1);
                black_box_unit(cache.try_commit());
            });
        });
    }
    direct_group.finish();

    let mut snapshot_group = c.benchmark_group("storage/store_cache_commit_rocksdb_snapshot");
    for &change_count in &[1usize, 10, 100, 500] {
        snapshot_group.bench_function(format!("{change_count}_changes"), |b| {
            let (store, _tempdir) = open_rocksdb("snapshot");
            let store = Arc::new(SnapshotOnlyStore::new(store));
            let mut iteration = 0u64;
            b.iter(|| {
                let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
                stage_changes(&mut cache, iteration, change_count);
                iteration = iteration.wrapping_add(1);
                black_box_unit(cache.try_commit());
            });
        });
    }
    snapshot_group.finish();

    let mut mdbx_group = c.benchmark_group("storage/store_cache_commit_mdbx_direct");
    for &change_count in &[1usize, 10, 100, 500] {
        mdbx_group.bench_function(format!("{change_count}_changes"), |b| {
            let (store, _tempdir) = open_mdbx("direct-mdbx");
            let mut iteration = 0u64;
            b.iter(|| {
                let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
                stage_changes(&mut cache, iteration, change_count);
                iteration = iteration.wrapping_add(1);
                black_box_unit(cache.try_commit());
            });
        });
    }
    mdbx_group.finish();
}

fn bench_import_shaped_store_cache_commit(c: &mut Criterion) {
    const BLOCKS_PER_IMPORT_BATCH: usize = 500;
    const CHANGES_PER_BLOCK: usize = 4;
    const BUFFERED_LOGICAL_BATCHES: usize = 8;

    let mut immediate_group =
        c.benchmark_group("storage/store_cache_import_shaped_rocksdb_immediate");
    immediate_group.throughput(Throughput::Elements(BLOCKS_PER_IMPORT_BATCH as u64));
    immediate_group.bench_function("1x500_blocks_4_changes", |b| {
        let (store, _tempdir) = open_rocksdb("import-immediate");
        store.disable_fast_sync_mode();
        let mut batch = 0u64;
        b.iter(|| {
            let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
            stage_import_batch_changes(
                &mut cache,
                batch,
                BLOCKS_PER_IMPORT_BATCH,
                CHANGES_PER_BLOCK,
            );
            black_box_unit(cache.try_commit());
            black_box_unit(store.flush());
            batch = batch.wrapping_add(1);
        });
    });
    immediate_group.finish();

    let mut buffered_group =
        c.benchmark_group("storage/store_cache_import_shaped_rocksdb_fast_sync_buffered");
    buffered_group.throughput(Throughput::Elements(
        (BUFFERED_LOGICAL_BATCHES * BLOCKS_PER_IMPORT_BATCH) as u64,
    ));
    buffered_group.bench_with_input(
        BenchmarkId::new(
            "logical_import_batches_then_flush",
            format!("{BUFFERED_LOGICAL_BATCHES}x{BLOCKS_PER_IMPORT_BATCH}_blocks_4_changes"),
        ),
        &BUFFERED_LOGICAL_BATCHES,
        |b, &logical_batches| {
            let (store, _tempdir) = open_rocksdb("import-buffered");
            let mut batch = 0u64;
            b.iter(|| {
                for _ in 0..logical_batches {
                    let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
                    stage_import_batch_changes(
                        &mut cache,
                        batch,
                        BLOCKS_PER_IMPORT_BATCH,
                        CHANGES_PER_BLOCK,
                    );
                    black_box_unit(cache.try_commit());
                    batch = batch.wrapping_add(1);
                }
                black_box_unit(store.flush());
            });
        },
    );
    buffered_group.finish();

    let mut mdbx_group = c.benchmark_group("storage/store_cache_import_shaped_mdbx_direct");
    mdbx_group.throughput(Throughput::Elements(BLOCKS_PER_IMPORT_BATCH as u64));
    mdbx_group.bench_function("1x500_blocks_4_changes", |b| {
        let (store, _tempdir) = open_mdbx("import-mdbx");
        let mut batch = 0u64;
        b.iter(|| {
            let mut cache = StoreCache::new_from_store(Arc::clone(&store), false);
            stage_import_batch_changes(
                &mut cache,
                batch,
                BLOCKS_PER_IMPORT_BATCH,
                CHANGES_PER_BLOCK,
            );
            black_box_unit(cache.try_commit());
            black_box_unit(store.flush());
            batch = batch.wrapping_add(1);
        });
    });
    mdbx_group.finish();
}

criterion_group!(
    benches,
    bench_store_cache_commit,
    bench_import_shaped_store_cache_commit,
);
criterion_main!(benches);
