//! Storage Operations Benchmarks
//!
//! Benchmarks for storage read/write operations, caching, and database
//! operations used in the Neo blockchain.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use neo_core::persistence::{
    i_read_only_store::IReadOnlyStoreGeneric, i_write_store::IWriteStore, providers::MemoryStore,
    seek_direction::SeekDirection,
};
use neo_core::smart_contract::{StorageItem, StorageKey};
use rand::{rngs::OsRng, RngCore};

// Generate random bytes
fn random_bytes(size: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; size];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

// Create a storage key
fn create_storage_key(id: i32) -> StorageKey {
    let mut key_bytes = vec![0u8; 32];
    OsRng.fill_bytes(&mut key_bytes);
    StorageKey::new(id, key_bytes)
}

fn bench_memory_store_read_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_memory_store");

    // Benchmark write operations
    for size in [32usize, 256, 1024, 4096].iter() {
        let _data = random_bytes(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("write", size), size, |b, &size| {
            b.iter(|| {
                let mut store = MemoryStore::new();
                let key = create_storage_key(0);
                let raw_key = key.to_array();
                let raw_value = random_bytes(size);
                store.put(raw_key, raw_value);
                black_box(store)
            });
        });
    }

    // Benchmark read operations
    for size in [32usize, 256, 1024, 4096].iter() {
        let mut store = MemoryStore::new();
        let key = create_storage_key(0);
        let raw_key = key.to_array();
        let raw_value = random_bytes(*size);
        store.put(raw_key.clone(), raw_value);

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("read", size), size, |b, _| {
            b.iter(|| {
                let result = IReadOnlyStoreGeneric::try_get(&store, black_box(&raw_key));
                black_box(result)
            });
        });
    }

    // Benchmark delete operations
    group.bench_function("delete", |b| {
        b.iter(|| {
            let mut store = MemoryStore::new();
            let key = create_storage_key(0);
            let data = random_bytes(256);
            let raw_key = key.to_array();
            let raw_value = data;
            store.put(raw_key.clone(), raw_value);
            store.delete(black_box(raw_key));
            black_box(())
        });
    });

    group.finish();
}

fn bench_storage_key_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_key");

    group.bench_function("create", |b| {
        b.iter(|| {
            let key = create_storage_key(0);
            black_box(key)
        });
    });

    group.bench_function("serialize", |b| {
        let key = create_storage_key(0);
        b.iter(|| {
            let result = key.to_array();
            black_box(result)
        });
    });

    group.bench_function("deserialize", |b| {
        let key = create_storage_key(0);
        let serialized = key.to_array();
        b.iter(|| {
            let result = StorageKey::from_bytes(&serialized);
            black_box(result)
        });
    });

    group.finish();
}

fn bench_storage_item_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_item");

    for size in [32usize, 256, 1024, 4096].iter() {
        let data = random_bytes(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("create", size), size, |b, _| {
            b.iter(|| {
                let item = StorageItem::from_bytes(black_box(data.clone()));
                black_box(item)
            });
        });
    }

    group.bench_function("get_value", |b| {
        let data = random_bytes(256);
        let item = StorageItem::from_bytes(data);
        b.iter(|| {
            let result = item.get_value();
            black_box(result)
        });
    });

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_batch");

    group.bench_function("batch_write_10", |b| {
        b.iter(|| {
            let mut store = MemoryStore::new();
            for i in 0..10 {
                let key = create_storage_key(i);
                let data = random_bytes(256);
                let raw_key = key.to_array();
                let raw_value = data;
                store.put(raw_key, raw_value);
            }
            black_box(())
        });
    });

    group.bench_function("batch_write_100", |b| {
        b.iter(|| {
            let mut store = MemoryStore::new();
            for i in 0..100 {
                let key = create_storage_key(i);
                let data = random_bytes(256);
                let raw_key = key.to_array();
                let raw_value = data;
                store.put(raw_key, raw_value);
            }
            black_box(())
        });
    });

    group.bench_function("batch_read_10", |b| {
        let mut store = MemoryStore::new();
        let keys: Vec<_> = (0..10)
            .map(|i| {
                let key = create_storage_key(i);
                let data = random_bytes(256);
                let raw_key = key.to_array();
                let raw_value = data;
                store.put(raw_key.clone(), raw_value);
                raw_key
            })
            .collect();

        b.iter(|| {
            for key in &keys {
                let _ = IReadOnlyStoreGeneric::try_get(&store, black_box(key));
            }
            black_box(())
        });
    });

    group.bench_function("batch_read_100", |b| {
        let mut store = MemoryStore::new();
        let keys: Vec<_> = (0..100)
            .map(|i| {
                let key = create_storage_key(i);
                let data = random_bytes(256);
                let raw_key = key.to_array();
                let raw_value = data;
                store.put(raw_key.clone(), raw_value);
                raw_key
            })
            .collect();

        b.iter(|| {
            for key in &keys {
                let _ = IReadOnlyStoreGeneric::try_get(&store, black_box(key));
            }
            black_box(())
        });
    });

    group.finish();
}

fn bench_seek_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage_seek");

    group.bench_function("seek_forward_100", |b| {
        let mut store = MemoryStore::new();

        // Populate with data
        for i in 0..100 {
            let key = create_storage_key(i);
            let data = random_bytes(256);
            let raw_key = key.to_array();
            let raw_value = data;
            store.put(raw_key, raw_value);
        }

        let start_key = create_storage_key(0);
        let start_bytes = start_key.to_array();

        b.iter(|| {
            let result: Vec<_> = IReadOnlyStoreGeneric::find(
                &store,
                black_box(Some(&start_bytes)),
                SeekDirection::Forward,
            )
            .take(10)
            .collect();
            black_box(result)
        });
    });

    group.bench_function("seek_backward_100", |b| {
        let mut store = MemoryStore::new();

        // Populate with data
        for i in 0..100 {
            let key = create_storage_key(i);
            let data = random_bytes(256);
            let raw_key = key.to_array();
            let raw_value = data;
            store.put(raw_key, raw_value);
        }

        let start_key = create_storage_key(99);
        let start_bytes = start_key.to_array();

        b.iter(|| {
            let result: Vec<_> = IReadOnlyStoreGeneric::find(
                &store,
                black_box(Some(&start_bytes)),
                SeekDirection::Backward,
            )
            .take(10)
            .collect();
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_memory_store_read_write,
    bench_storage_key_operations,
    bench_storage_item_operations,
    bench_batch_operations,
    bench_seek_operations
);
criterion_main!(benches);
