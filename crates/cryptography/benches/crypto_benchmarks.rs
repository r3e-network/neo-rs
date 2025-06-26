//! Performance benchmarks for Neo cryptographic operations
//!
//! These benchmarks measure the performance of cryptographic operations that are
//! critical for blockchain performance, including ECDSA signing/verification,
//! hashing operations, and key generation.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use neo_cryptography::{
    ecdsa::{KeyPair, PrivateKey, PublicKey},
    hash::{hash160, hash256, sha256},
};
use rand::Rng;

/// Benchmark ECDSA key operations
fn bench_ecdsa_key_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("ecdsa_key_operations");

    group.bench_function("keypair_generate", |b| {
        b.iter(|| black_box(KeyPair::generate().unwrap()))
    });

    let keypair = KeyPair::generate().unwrap();

    group.bench_function("keypair_get_public_key", |b| {
        b.iter(|| black_box(keypair.get_public_key()))
    });

    group.bench_function("keypair_get_private_key", |b| {
        b.iter(|| black_box(keypair.get_private_key()))
    });

    group.bench_function("keypair_get_verification_script", |b| {
        b.iter(|| black_box(keypair.get_verification_script()))
    });

    group.bench_function("keypair_get_script_hash", |b| {
        b.iter(|| black_box(keypair.get_script_hash()))
    });

    group.finish();
}

/// Benchmark ECDSA signing and verification
fn bench_ecdsa_sign_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("ecdsa_sign_verify");

    let keypair = KeyPair::generate().unwrap();
    let message = b"Hello, Neo blockchain! This is a test message for signing.";
    let signature = keypair.sign(message).unwrap();
    let public_key = keypair.get_public_key();

    group.bench_function("ecdsa_sign", |b| {
        b.iter(|| black_box(keypair.sign(message).unwrap()))
    });

    group.bench_function("ecdsa_verify", |b| {
        b.iter(|| black_box(public_key.verify(message, &signature).unwrap()))
    });

    // Benchmark with different message sizes
    for size in [32, 64, 128, 256, 512, 1024].iter() {
        let large_message = vec![0x42u8; *size];

        group.bench_with_input(
            BenchmarkId::new("ecdsa_sign_message_size", size),
            size,
            |b, _| b.iter(|| black_box(keypair.sign(&large_message).unwrap())),
        );

        let large_signature = keypair.sign(&large_message).unwrap();
        group.bench_with_input(
            BenchmarkId::new("ecdsa_verify_message_size", size),
            size,
            |b, _| {
                b.iter(|| black_box(public_key.verify(&large_message, &large_signature).unwrap()))
            },
        );
    }

    group.finish();
}

/// Benchmark hash operations
fn bench_hash_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_operations");

    let test_data = b"Hello, Neo blockchain! This is test data for hashing operations.";

    group.bench_function("sha256", |b| b.iter(|| black_box(sha256(test_data))));

    group.bench_function("hash160", |b| b.iter(|| black_box(hash160(test_data))));

    group.bench_function("hash256", |b| b.iter(|| black_box(hash256(test_data))));

    // Benchmark with different data sizes
    for size in [32, 64, 128, 256, 512, 1024, 2048, 4096].iter() {
        let large_data = vec![0x42u8; *size];

        group.bench_with_input(BenchmarkId::new("sha256_data_size", size), size, |b, _| {
            b.iter(|| black_box(sha256(&large_data)))
        });

        group.bench_with_input(BenchmarkId::new("hash160_data_size", size), size, |b, _| {
            b.iter(|| black_box(hash160(&large_data)))
        });

        group.bench_with_input(BenchmarkId::new("hash256_data_size", size), size, |b, _| {
            b.iter(|| black_box(hash256(&large_data)))
        });
    }

    group.finish();
}

/// Benchmark batch cryptographic operations (simulating blockchain workload)
fn bench_batch_crypto_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_crypto_operations");

    // Simulate transaction verification batch (common blockchain operation)
    group.bench_function("verify_transaction_batch", |b| {
        // Pre-generate keypairs and signatures
        let mut keypairs = Vec::new();
        let mut signatures = Vec::new();
        let message = b"Transaction data to be verified";

        for _ in 0..100 {
            let keypair = KeyPair::generate().unwrap();
            let signature = keypair.sign(message).unwrap();
            keypairs.push(keypair);
            signatures.push(signature);
        }

        b.iter(|| {
            let mut verified_count = 0;
            for (keypair, signature) in keypairs.iter().zip(signatures.iter()) {
                let public_key = keypair.get_public_key();
                if public_key.verify(message, signature).unwrap() {
                    verified_count += 1;
                }
            }
            black_box(verified_count)
        })
    });

    // Simulate address generation batch
    group.bench_function("generate_address_batch", |b| {
        b.iter(|| {
            let mut addresses = Vec::new();
            for _ in 0..100 {
                let keypair = KeyPair::generate().unwrap();
                let script_hash = keypair.get_script_hash();
                addresses.push(script_hash);
            }
            black_box(addresses.len())
        })
    });

    // Simulate hash computation batch (like Merkle tree construction)
    group.bench_function("hash_computation_batch", |b| {
        let mut data_items = Vec::new();
        for i in 0..100 {
            let mut data = vec![0u8; 32];
            data[0] = i as u8;
            data_items.push(data);
        }

        b.iter(|| {
            let mut hashes = Vec::new();
            for data in &data_items {
                hashes.push(sha256(data));
            }

            // Simulate Merkle tree level computation
            while hashes.len() > 1 {
                let mut next_level = Vec::new();
                for chunk in hashes.chunks(2) {
                    if chunk.len() == 2 {
                        let combined = [chunk[0], chunk[1]].concat();
                        next_level.push(sha256(&combined));
                    } else {
                        next_level.push(chunk[0]);
                    }
                }
                hashes = next_level;
            }

            black_box(hashes[0])
        })
    });

    group.finish();
}

/// Benchmark key serialization and parsing
fn bench_key_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_serialization");

    let keypair = KeyPair::generate().unwrap();
    let public_key = keypair.get_public_key();
    let private_key = keypair.get_private_key();

    // Public key operations
    let public_key_bytes = public_key.to_bytes();
    let public_key_hex = hex::encode(&public_key_bytes);

    group.bench_function("public_key_to_bytes", |b| {
        b.iter(|| black_box(public_key.to_bytes()))
    });

    group.bench_function("public_key_from_bytes", |b| {
        b.iter(|| black_box(PublicKey::from_bytes(&public_key_bytes).unwrap()))
    });

    group.bench_function("public_key_to_hex", |b| {
        b.iter(|| black_box(hex::encode(public_key.to_bytes())))
    });

    group.bench_function("public_key_from_hex", |b| {
        b.iter(|| {
            let bytes = hex::decode(&public_key_hex).unwrap();
            black_box(PublicKey::from_bytes(&bytes).unwrap())
        })
    });

    // Private key operations
    let private_key_bytes = private_key.to_bytes();

    group.bench_function("private_key_to_bytes", |b| {
        b.iter(|| black_box(private_key.to_bytes()))
    });

    group.bench_function("private_key_from_bytes", |b| {
        b.iter(|| black_box(PrivateKey::from_bytes(&private_key_bytes).unwrap()))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ecdsa_key_operations,
    bench_ecdsa_sign_verify,
    bench_hash_operations,
    bench_batch_crypto_operations,
    bench_key_serialization
);

criterion_main!(benches);
