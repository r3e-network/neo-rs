//! Benchmark for syscall dispatch optimization
//!
//! This benchmark demonstrates the performance improvement from using
//! cuckoo hash lookup instead of HashMap for built-in syscall dispatch.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neo_vm::syscalls;
use sha2::{Digest, Sha256};

fn bench_syscall_lookup(c: &mut Criterion) {
    // Initialize the registry
    syscalls::init();

    // Test syscalls representing hot paths in production
    let test_syscalls = vec![
        "System.Runtime.Platform",
        "System.Runtime.GasLeft",
        "System.Storage.Get",
        "System.Contract.Call",
        "System.Crypto.CheckSig",
    ];

    let mut group = c.benchmark_group("syscall_dispatch");

    // Benchmark u32 lookup (optimized hot path)
    group.bench_function("u32_lookup_optimized", |b| {
        let hashes: Vec<u32> = test_syscalls
            .iter()
            .map(|name| {
                let digest = Sha256::digest(name.as_bytes());
                u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
            })
            .collect();

        b.iter(|| {
            for hash in &hashes {
                let result = syscalls::get_syscall_entry_by_u32(black_box(*hash));
                black_box(result);
            }
        });
    });

    // Benchmark full hash lookup (for comparison)
    group.bench_function("full_hash_lookup", |b| {
        let hashes: Vec<[u8; 32]> = test_syscalls
            .iter()
            .map(|name| {
                let digest = Sha256::digest(name.as_bytes());
                digest.into()
            })
            .collect();

        b.iter(|| {
            for hash in &hashes {
                let result = syscalls::get_syscall_entry(black_box(hash));
                black_box(result);
            }
        });
    });

    // Benchmark name-based lookup (slowest path)
    group.bench_function("name_lookup", |b| {
        b.iter(|| {
            for name in &test_syscalls {
                let result = syscalls::find_by_method_name(black_box(name));
                black_box(result);
            }
        });
    });

    group.finish();
}

criterion_group!(benches, bench_syscall_lookup);
criterion_main!(benches);
