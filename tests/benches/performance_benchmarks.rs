//! Performance Benchmarks for neo-rs
//!
//! Benchmarks for critical performance paths:
//! - State trie operations (MPT)
//! - Cryptographic operations
//! - Block/transaction serialization
//! - Chain state operations

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use neo_crypto::Crypto;
use neo_primitives::{UInt160, UInt256};
use neo_state::{StateChanges, StateTrieManager, StorageItem, StorageKey};

// ============================================================================
// Cryptographic Benchmarks
// ============================================================================

fn bench_sha256(c: &mut Criterion) {
    let data_sizes = [32, 256, 1024, 4096];

    let mut group = c.benchmark_group("crypto/sha256");
    for size in data_sizes {
        let data = vec![0xFFu8; size];
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| Crypto::sha256(black_box(data)))
        });
    }
    group.finish();
}

fn bench_hash256(c: &mut Criterion) {
    let data_sizes = [32, 256, 1024];

    let mut group = c.benchmark_group("crypto/hash256");
    for size in data_sizes {
        let data = vec![0xFFu8; size];
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| Crypto::hash256(black_box(data)))
        });
    }
    group.finish();
}

fn bench_hash160(c: &mut Criterion) {
    let data_sizes = [33, 65]; // Compressed and uncompressed public key sizes

    let mut group = c.benchmark_group("crypto/hash160");
    for size in data_sizes {
        let data = vec![0xFFu8; size];
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| Crypto::hash160(black_box(data)))
        });
    }
    group.finish();
}

// ============================================================================
// State Trie Benchmarks
// ============================================================================

fn bench_state_trie_single_insert(c: &mut Criterion) {
    c.bench_function("state_trie/single_insert", |b| {
        b.iter_with_setup(
            || {
                let trie = StateTrieManager::new(false);
                let mut changes = StateChanges::new();
                let key = StorageKey::new(UInt160::default(), vec![0x01, 0x02, 0x03]);
                let item = StorageItem::new(vec![0x04, 0x05, 0x06]);
                changes.storage.insert(key, Some(item));
                (trie, changes)
            },
            |(mut trie, changes)| black_box(trie.apply_changes(1, &changes).unwrap()),
        )
    });
}

fn bench_state_trie_batch_insert(c: &mut Criterion) {
    let batch_sizes = [10, 100, 1000];

    let mut group = c.benchmark_group("state_trie/batch_insert");
    for size in batch_sizes {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_with_setup(
                || {
                    let trie = StateTrieManager::new(false);
                    let mut changes = StateChanges::new();
                    for i in 0..size {
                        let key =
                            StorageKey::new(UInt160::default(), (i as u32).to_le_bytes().to_vec());
                        let item = StorageItem::new(vec![0xFFu8; 32]);
                        changes.storage.insert(key, Some(item));
                    }
                    (trie, changes)
                },
                |(mut trie, changes)| black_box(trie.apply_changes(1, &changes).unwrap()),
            )
        });
    }
    group.finish();
}

fn bench_state_trie_incremental_blocks(c: &mut Criterion) {
    c.bench_function("state_trie/10_blocks_10_changes_each", |b| {
        b.iter_with_setup(
            || StateTrieManager::new(false),
            |mut trie| {
                for block in 1u32..=10 {
                    let mut changes = StateChanges::new();
                    for i in 0..10 {
                        let key = StorageKey::new(UInt160::default(), vec![block as u8, i as u8]);
                        let item = StorageItem::new(vec![0xFFu8; 32]);
                        changes.storage.insert(key, Some(item));
                    }
                    black_box(trie.apply_changes(block, &changes).unwrap());
                }
            },
        )
    });
}

// ============================================================================
// Primitive Type Benchmarks
// ============================================================================

fn bench_uint256_from_bytes(c: &mut Criterion) {
    let bytes = [0xFFu8; 32];

    c.bench_function("primitives/uint256_from_bytes", |b| {
        b.iter(|| UInt256::from_bytes(black_box(&bytes)))
    });
}

fn bench_uint256_to_bytes(c: &mut Criterion) {
    let hash = UInt256::from([0xFFu8; 32]);

    c.bench_function("primitives/uint256_to_bytes", |b| {
        b.iter(|| black_box(&hash).to_bytes())
    });
}

fn bench_uint160_from_bytes(c: &mut Criterion) {
    let bytes = [0xFFu8; 20];

    c.bench_function("primitives/uint160_from_bytes", |b| {
        b.iter(|| UInt160::from_bytes(black_box(&bytes)))
    });
}

// ============================================================================
// Storage Key Benchmarks
// ============================================================================

fn bench_storage_key_creation(c: &mut Criterion) {
    let contract = UInt160::from([0x01u8; 20]);
    let key_data = vec![0x01, 0x02, 0x03, 0x04];

    c.bench_function("storage/key_creation", |b| {
        b.iter(|| StorageKey::new(black_box(contract), black_box(key_data.clone())))
    });
}

fn bench_storage_key_hash(c: &mut Criterion) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let key = StorageKey::new(UInt160::from([0x01u8; 20]), vec![0x01, 0x02, 0x03]);

    c.bench_function("storage/key_hash", |b| {
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            black_box(&key).hash(&mut hasher);
            hasher.finish()
        })
    });
}

// ============================================================================
// Criterion Groups
// ============================================================================

criterion_group!(crypto_benches, bench_sha256, bench_hash256, bench_hash160,);

criterion_group!(
    state_trie_benches,
    bench_state_trie_single_insert,
    bench_state_trie_batch_insert,
    bench_state_trie_incremental_blocks,
);

criterion_group!(
    primitive_benches,
    bench_uint256_from_bytes,
    bench_uint256_to_bytes,
    bench_uint160_from_bytes,
);

criterion_group!(
    storage_benches,
    bench_storage_key_creation,
    bench_storage_key_hash,
);

criterion_main!(
    crypto_benches,
    state_trie_benches,
    primitive_benches,
    storage_benches,
);
