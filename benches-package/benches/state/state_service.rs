#![allow(missing_docs)] // benchmark/integration-test harness: not public API
use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use neo_state_service::{
    StateRootApplyMetrics, StateStore, commit_handlers::StateServiceCommitHandlers,
};
use neo_storage::{DataCache, StorageItem, StorageKey, mdbx::MdbxStore, persistence::Store};
use std::sync::Arc;

#[path = "../support/mod.rs"]
mod support;

use support::{BenchTempDir, make_mdbx_store};

fn make_snapshot(change_count: usize) -> DataCache {
    let snapshot = DataCache::new(false);

    for index in 0..change_count {
        let key = StorageKey::new(5, (index as u32).to_le_bytes().to_vec());
        let value = StorageItem::from_bytes(vec![index as u8, 0xAA, 0x55]);
        snapshot.add(key, value);
    }

    snapshot
}

fn make_transaction_batch(
    block_count: usize,
    changes_per_block: usize,
    generation: u8,
) -> Vec<DataCache> {
    (0..block_count)
        .map(|block| {
            let snapshot = DataCache::new(false);
            for change in 0..changes_per_block {
                let key_index = block
                    .checked_mul(changes_per_block)
                    .and_then(|base| base.checked_add(change))
                    .expect("benchmark key index stays in range");
                let key = StorageKey::new(5, (key_index as u32).to_le_bytes().to_vec());
                let value = StorageItem::from_bytes(vec![
                    generation,
                    block as u8,
                    change as u8,
                    0xAA,
                    0x55,
                ]);
                snapshot.add(key, value);
            }
            snapshot
        })
        .collect()
}

struct TransactionBatchWorkload<S: Store, G> {
    store: Arc<StateStore<S>>,
    handlers: StateServiceCommitHandlers<S>,
    snapshots: Vec<DataCache>,
    _storage_guard: G,
}

impl<S: Store, G> TransactionBatchWorkload<S, G> {
    fn new(
        store: StateStore<S>,
        storage_guard: G,
        block_count: usize,
        changes_per_block: usize,
    ) -> Self {
        let store = Arc::new(store);
        let handlers =
            StateServiceCommitHandlers::new_async_with_capacity(Arc::clone(&store), block_count);
        Self {
            store,
            handlers,
            snapshots: make_transaction_batch(block_count, changes_per_block, 0x11),
            _storage_guard: storage_guard,
        }
    }

    fn apply_batch(&self) -> Option<u32> {
        for (offset, snapshot) in self.snapshots.iter().enumerate() {
            assert!(
                self.handlers
                    .on_committing_deferred(offset as u32, snapshot,)
            );
        }
        assert!(self.handlers.flush());
        self.store.current_local_index()
    }
}

fn make_mdbx_state_store() -> (StateStore<MdbxStore>, BenchTempDir) {
    let (backing, tempdir) = make_mdbx_store("neo-state-service-mdbx-bench");
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

fn bench_state_service_empty_continuation_batches(c: &mut Criterion) {
    let mut group = c.benchmark_group("state_service/empty_continuation_batches");
    for &batch_len in &[10usize, 100, 1000, 4096] {
        group.bench_function(format!("memory_{batch_len}_blocks"), |b| {
            let store = Arc::new(StateStore::with_mpt(false));
            let handlers = StateServiceCommitHandlers::new_async_with_capacity(
                Arc::clone(&store),
                batch_len.max(1),
            );
            let seed = make_snapshot(1);
            let empty = DataCache::new(false);
            let mut next_index = 1u32;
            store
                .apply_snapshot_changes(0, &seed)
                .expect("seed root applies");
            b.iter(|| {
                let start = next_index;
                next_index = next_index.wrapping_add(batch_len as u32);
                black_box(apply_empty_batch(&handlers, &empty, start, batch_len));
            });
        });
    }
    group.finish();

    let mut mdbx_group = c.benchmark_group("state_service/empty_continuation_batches_mdbx");
    for &batch_len in &[10usize, 100, 1000, 4096] {
        mdbx_group.bench_function(format!("{batch_len}_blocks"), |b| {
            let (store, _tempdir) = make_mdbx_state_store();
            let store = Arc::new(store);
            let handlers = StateServiceCommitHandlers::new_async_with_capacity(
                Arc::clone(&store),
                batch_len.max(1),
            );
            let seed = make_snapshot(1);
            let empty = DataCache::new(false);
            let mut next_index = 1u32;
            store
                .apply_snapshot_changes(0, &seed)
                .expect("seed root applies");
            b.iter(|| {
                let start = next_index;
                next_index = next_index.wrapping_add(batch_len as u32);
                black_box(apply_empty_batch(&handlers, &empty, start, batch_len));
            });
        });
    }
    mdbx_group.finish();
}

fn bench_state_service_transaction_bearing_batches(c: &mut Criterion) {
    const BLOCK_COUNT: usize = 32;
    const CHANGES_PER_BLOCK: usize = 16;

    let mut group = c.benchmark_group("state_service/transaction_bearing_batches");
    group.throughput(criterion::Throughput::Elements(BLOCK_COUNT as u64));
    group.bench_with_input(
        BenchmarkId::new("memory_blocks", BLOCK_COUNT),
        &CHANGES_PER_BLOCK,
        |b, &changes_per_block| {
            b.iter_batched(
                || {
                    TransactionBatchWorkload::new(
                        StateStore::with_mpt(false),
                        (),
                        BLOCK_COUNT,
                        changes_per_block,
                    )
                },
                |workload| {
                    let root = black_box(workload.apply_batch());
                    (workload, root)
                },
                BatchSize::PerIteration,
            );
        },
    );
    group.bench_with_input(
        BenchmarkId::new("mdbx_blocks", BLOCK_COUNT),
        &CHANGES_PER_BLOCK,
        |b, &changes_per_block| {
            b.iter_batched(
                || {
                    let (store, tempdir) = make_mdbx_state_store();
                    TransactionBatchWorkload::new(store, tempdir, BLOCK_COUNT, changes_per_block)
                },
                |workload| {
                    let root = black_box(workload.apply_batch());
                    (workload, root)
                },
                BatchSize::PerIteration,
            );
        },
    );
    group.finish();

    eprintln!("StateService transaction-bearing batch stage EWMA:");
    for stage in StateRootApplyMetrics::state_root_apply_stage_stats() {
        eprintln!(
            "  {:>20}: {:>8} us over {} observations",
            stage.stage, stage.avg_us, stage.calls
        );
    }
    for count in StateRootApplyMetrics::state_root_apply_count_stats() {
        eprintln!(
            "  {:>20}: avg {:>8}, total {} over {} samples",
            count.kind, count.avg, count.total, count.samples
        );
    }
}

fn apply_empty_batch<S: Store>(
    handlers: &StateServiceCommitHandlers<S>,
    empty: &DataCache,
    start_index: u32,
    batch_len: usize,
) -> bool {
    for offset in 0..batch_len {
        if !handlers.on_committing_deferred(start_index + offset as u32, empty) {
            return false;
        }
    }
    handlers.flush_result().is_ok()
}

criterion_group!(
    benches,
    bench_state_service_apply_snapshot_changes,
    bench_state_service_empty_continuation_batches,
    bench_state_service_transaction_bearing_batches,
);
criterion_main!(benches);
