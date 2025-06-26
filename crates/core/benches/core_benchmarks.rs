//! Performance benchmarks for Neo core types and operations
//!
//! These benchmarks measure the performance of critical operations that are
//! used frequently in blockchain processing, allowing us to compare against
//! the C# implementation and identify optimization opportunities.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_core::{BigDecimal, Transaction, UInt160, UInt256, Witness};
use num_bigint::BigInt;
use std::str::FromStr;

/// Benchmark UInt160 operations
fn bench_uint160_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("uint160_operations");

    // Test data
    let test_bytes = [0x12u8; 20];
    let test_hex = "0x1234567890abcdef1234567890abcdef12345678";

    group.bench_function("uint160_new", |b| b.iter(|| black_box(UInt160::new())));

    group.bench_function("uint160_from_bytes", |b| {
        b.iter(|| black_box(UInt160::from_bytes(&test_bytes).unwrap()))
    });

    group.bench_function("uint160_from_string", |b| {
        b.iter(|| black_box(UInt160::from_str(test_hex).unwrap()))
    });

    let uint160 = UInt160::from_bytes(&test_bytes).unwrap();

    group.bench_function("uint160_to_array", |b| {
        b.iter(|| black_box(uint160.to_array()))
    });

    group.bench_function("uint160_to_string", |b| {
        b.iter(|| black_box(uint160.to_string()))
    });

    group.bench_function("uint160_equals", |b| {
        let other = UInt160::from_bytes(&test_bytes).unwrap();
        b.iter(|| black_box(uint160 == other))
    });

    group.finish();
}

/// Benchmark UInt256 operations
fn bench_uint256_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("uint256_operations");

    // Test data
    let test_bytes = [0x12u8; 32];
    let test_hex = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";

    group.bench_function("uint256_new", |b| b.iter(|| black_box(UInt256::new())));

    group.bench_function("uint256_from_bytes", |b| {
        b.iter(|| black_box(UInt256::from_bytes(&test_bytes).unwrap()))
    });

    group.bench_function("uint256_from_string", |b| {
        b.iter(|| black_box(UInt256::parse(test_hex).unwrap()))
    });

    let uint256 = UInt256::from_bytes(&test_bytes).unwrap();

    group.bench_function("uint256_to_array", |b| {
        b.iter(|| black_box(uint256.to_array()))
    });

    group.bench_function("uint256_to_string", |b| {
        b.iter(|| black_box(uint256.to_string()))
    });

    group.bench_function("uint256_equals", |b| {
        let other = UInt256::from_bytes(&test_bytes).unwrap();
        b.iter(|| black_box(uint256 == other))
    });

    group.finish();
}

/// Benchmark Transaction operations
fn bench_transaction_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("transaction_operations");

    group.bench_function("transaction_new", |b| {
        b.iter(|| black_box(Transaction::new()))
    });

    let mut transaction = Transaction::new();
    let script = vec![0x0c, 0x21, 0x03]; // Sample script

    group.bench_function("transaction_set_script", |b| {
        b.iter(|| {
            let mut tx = transaction.clone();
            black_box(tx.set_script(script.clone()))
        })
    });

    transaction.set_script(script);

    group.bench_function("transaction_get_hash_data", |b| {
        b.iter(|| black_box(transaction.get_hash_data()))
    });

    group.bench_function("transaction_size", |b| {
        b.iter(|| black_box(transaction.size()))
    });

    group.finish();
}

/// Benchmark Witness operations
fn bench_witness_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("witness_operations");

    let invocation_script = vec![0x0c, 0x40]; // PUSHDATA1 64 bytes
    let verification_script = vec![0x0c, 0x21, 0x03]; // Sample verification script

    group.bench_function("witness_new", |b| b.iter(|| black_box(Witness::new())));

    group.bench_function("witness_new_with_scripts", |b| {
        b.iter(|| {
            black_box(Witness::new_with_scripts(
                invocation_script.clone(),
                verification_script.clone(),
            ))
        })
    });

    let witness = Witness::new_with_scripts(invocation_script, verification_script);

    group.bench_function("witness_size", |b| b.iter(|| black_box(witness.size())));

    group.bench_function("witness_clone", |b| b.iter(|| black_box(witness.clone())));

    group.finish();
}

/// Benchmark BigDecimal operations
fn bench_big_decimal_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("big_decimal_operations");

    group.bench_function("big_decimal_new", |b| {
        b.iter(|| black_box(BigDecimal::new(BigInt::from(12345), 8)))
    });

    group.bench_function("big_decimal_parse", |b| {
        b.iter(|| black_box(BigDecimal::parse("123.45678901", 8).unwrap()))
    });

    let decimal1 = BigDecimal::new(BigInt::from(12345), 8);

    group.bench_function("big_decimal_to_string", |b| {
        b.iter(|| black_box(decimal1.to_string()))
    });

    group.finish();
}

/// Benchmark batch operations that simulate real blockchain workloads
fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");

    // Simulate processing a batch of transactions
    group.bench_function("process_transaction_batch", |b| {
        b.iter(|| {
            let mut transactions = Vec::new();
            for i in 0..100 {
                let mut tx = Transaction::new();
                let script = vec![0x0c, 0x21, i as u8]; // Unique script per transaction
                tx.set_script(script);
                transactions.push(tx);
            }

            // Process each transaction (simulate hash calculation)
            for tx in &transactions {
                black_box(tx.get_hash_data());
            }

            black_box(transactions.len())
        })
    });

    // Simulate address validation batch
    group.bench_function("validate_address_batch", |b| {
        b.iter(|| {
            let mut addresses = Vec::new();
            for i in 0..100 {
                let mut bytes = [0u8; 20];
                bytes[0] = i as u8;
                let uint160 = UInt160::from_bytes(&bytes).unwrap();
                addresses.push(uint160.to_address());
            }

            // Validate each address
            for address in &addresses {
                black_box(UInt160::from_address(address).is_ok());
            }

            black_box(addresses.len())
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_uint160_operations,
    bench_uint256_operations,
    bench_transaction_operations,
    bench_witness_operations,
    bench_big_decimal_operations,
    bench_batch_operations
);

criterion_main!(benches);
