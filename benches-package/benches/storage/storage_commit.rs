#![allow(missing_docs)] // benchmark harness: not public API
use criterion::Throughput;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_storage::mdbx::{MdbxStore, MdbxStoreProvider};
use neo_storage::{
    StorageItem, StorageKey,
    persistence::{RawReadOnlyStore, Store, StoreCache, storage::StorageConfig},
};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static BENCH_SEQ: AtomicU64 = AtomicU64::new(0);

struct BenchTempDir {
    path: PathBuf,
}

impl BenchTempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "neo-storage-commit-bench-{label}-{}-{}",
            std::process::id(),
            BENCH_SEQ.fetch_add(1, Ordering::Relaxed)
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

fn bench_sorted_mdbx_batch_read(c: &mut Criterion) {
    const STORED_ROWS: usize = 32_768;
    let (store, _tempdir) = open_mdbx("sorted-batch-read");
    let entries = (0..STORED_ROWS)
        .map(|index| {
            let key = (index as u32 * 2).to_be_bytes().to_vec();
            let value = [0xA5, (index & 0xff) as u8].to_vec();
            (key, value)
        })
        .collect::<Vec<_>>();
    store
        .commit_raw_overlay(
            entries
                .iter()
                .map(|(key, value)| (key.as_slice(), Some(value.as_slice()))),
        )
        .expect("seed sorted MDBX batch-read rows");
    let snapshot = store.snapshot();
    let keys = (0..STORED_ROWS * 2)
        .map(|index| (index as u32).to_be_bytes().to_vec())
        .collect::<Vec<_>>();

    let mut group = c.benchmark_group("storage/mdbx_sorted_batch_read");
    group.throughput(Throughput::Elements(keys.len() as u64));
    group.bench_function("64k_keys_half_missing", |b| {
        b.iter(|| {
            let values = snapshot
                .try_get_many_bytes_sorted(&keys)
                .expect("sorted MDBX batch read");
            black_box(values);
        });
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_store_cache_commit,
    bench_import_shaped_store_cache_commit,
    bench_sorted_mdbx_batch_read,
);
criterion_main!(benches);
