use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neo_core::crypto::Crypto;

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

// TODO: Add MPT trie insert/lookup benchmarks once a simpler in-memory
// MptStoreSnapshot implementation is available for benchmarking.

criterion_group!(benches, bench_sha256, bench_hash256, bench_hash160,);
criterion_main!(benches);
