#![allow(missing_docs)] // benchmark/integration-test harness: not public API
use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use neo_crypto::Crypto;
use neo_primitives::UInt256;

/// Benchmark SHA-256 hashing with various input sizes.
fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("sha256");

    for &size in &[32, 256, 1024, 4096, 65536] {
        let data = vec![0xABu8; size];
        group.bench_function(format!("{size}_bytes"), |b| {
            b.iter(|| {
                black_box(Crypto::sha256(black_box(&data)));
            });
        });
    }
    group.finish();
}

/// Benchmark Hash256 (double SHA-256) with various input sizes.
fn bench_hash256(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash256");

    for &size in &[32, 256, 1024, 4096] {
        let data = vec![0xCDu8; size];
        group.bench_function(format!("{size}_bytes"), |b| {
            b.iter(|| {
                black_box(Crypto::hash256(black_box(&data)));
            });
        });
    }
    group.finish();
}

/// Benchmark Hash160 (SHA-256 + RIPEMD-160) with various input sizes.
fn bench_hash160(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash160");

    for &size in &[32, 256, 1024, 4096] {
        let data = vec![0xEFu8; size];
        group.bench_function(format!("{size}_bytes"), |b| {
            b.iter(|| {
                black_box(Crypto::hash160(black_box(&data)));
            });
        });
    }
    group.finish();
}

fn make_mpt_hashes(count: usize) -> Vec<(UInt256, u64)> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    (0..count)
        .map(|index| {
            let mut bytes = [0u8; 32];
            for chunk in bytes.chunks_exact_mut(8) {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                chunk.copy_from_slice(&state.to_le_bytes());
            }
            (UInt256::from_array(bytes), index as u64)
        })
        .collect()
}

/// Measure the raw-byte hash ordering used by deferred MPT finalization.
fn bench_mpt_hash_sort(c: &mut Criterion) {
    const HASH_COUNT: usize = 200_000;
    let input = make_mpt_hashes(HASH_COUNT);
    let mut group = c.benchmark_group("mpt_hash_sort");
    group.throughput(criterion::Throughput::Elements(HASH_COUNT as u64));
    group.bench_function("200k_raw_byte_order", |b| {
        b.iter_batched(
            || input.clone(),
            |mut hashes| {
                hashes.sort_unstable_by_key(|(hash, _)| hash.to_array());
                black_box(hashes);
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

// MPT trie insert/lookup benchmarks belong in a dedicated state-service suite
// once the benchmark harness can create compact in-memory MPT snapshots.

criterion_group!(
    benches,
    bench_sha256,
    bench_hash256,
    bench_hash160,
    bench_mpt_hash_sort,
);
criterion_main!(benches);
