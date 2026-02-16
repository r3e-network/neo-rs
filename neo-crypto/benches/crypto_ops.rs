//! Cryptographic Operations Benchmarks
//!
//! Benchmarks for signature verification, hash operations, and other
//! cryptographic primitives used in the Neo blockchain.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use neo_crypto::{Crypto, Ed25519Crypto, Secp256k1Crypto, Secp256r1Crypto};
use rand::{RngCore, rngs::OsRng};

// Generate random data of specified size
fn random_data(size: usize) -> Vec<u8> {
    let mut data = vec![0u8; size];
    OsRng.fill_bytes(&mut data);
    data
}

fn bench_sha256(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_sha256");

    for size in [32, 64, 256, 1024, 4096, 65536].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::sha256(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_sha512(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_sha512");

    for size in [32, 64, 256, 1024, 4096, 65536].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::sha512(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_ripemd160(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_ripemd160");

    for size in [32, 64, 256, 1024, 4096].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::ripemd160(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_hash160(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_hash160");

    for size in [32, 64, 256, 1024, 4096].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::hash160(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_hash256(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_hash256");

    for size in [32, 64, 256, 1024, 4096, 65536].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::hash256(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_keccak256(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_keccak256");

    for size in [32, 64, 256, 1024, 4096].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::keccak256(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_blake2b(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_blake2b");

    for size in [32, 64, 256, 1024, 4096, 65536].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Crypto::blake2b(black_box(&data));
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_secp256r1_sign(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_secp256r1_sign");

    let private_key = Secp256r1Crypto::generate_private_key();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result =
                    Secp256r1Crypto::sign(black_box(&message), black_box(&private_key)).unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_secp256r1_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_secp256r1_verify");

    let private_key = Secp256r1Crypto::generate_private_key();
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        let signature = Secp256r1Crypto::sign(&message, &private_key).unwrap();
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Secp256r1Crypto::verify(
                    black_box(&message),
                    black_box(&signature),
                    black_box(&public_key),
                )
                .unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_secp256k1_sign(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_secp256k1_sign");

    let private_key = Secp256k1Crypto::generate_private_key().unwrap();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result =
                    Secp256k1Crypto::sign(black_box(&message), black_box(&private_key)).unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_secp256k1_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_secp256k1_verify");

    let private_key = Secp256k1Crypto::generate_private_key().unwrap();
    let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        let signature = Secp256k1Crypto::sign(&message, &private_key).unwrap();
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Secp256k1Crypto::verify(
                    black_box(&message),
                    black_box(&signature),
                    black_box(&public_key),
                )
                .unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_ed25519_sign(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_ed25519_sign");

    let private_key = Ed25519Crypto::generate_private_key();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result =
                    Ed25519Crypto::sign(black_box(&message), black_box(&private_key)).unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_ed25519_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_ed25519_verify");

    let private_key = Ed25519Crypto::generate_private_key();
    let public_key = Ed25519Crypto::derive_public_key(&private_key).unwrap();

    for size in [32, 256, 1024].iter() {
        let message = random_data(*size);
        let signature = Ed25519Crypto::sign(&message, &private_key).unwrap();
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, _| {
            b.iter(|| {
                let result = Ed25519Crypto::verify(
                    black_box(&message),
                    black_box(&signature),
                    black_box(&public_key),
                )
                .unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("crypto_key_generation");

    group.bench_function("secp256r1_generate", |b| {
        b.iter(|| {
            let key = Secp256r1Crypto::generate_private_key();
            black_box(key)
        });
    });

    group.bench_function("secp256k1_generate", |b| {
        b.iter(|| {
            let key = Secp256k1Crypto::generate_private_key().unwrap();
            black_box(key)
        });
    });

    group.bench_function("ed25519_generate", |b| {
        b.iter(|| {
            let key = Ed25519Crypto::generate_private_key();
            black_box(key)
        });
    });

    group.finish();
}

fn bench_base58(c: &mut Criterion) {
    use neo_crypto::crypto_utils::Base58;

    let mut group = c.benchmark_group("crypto_base58");

    for size in [20, 32, 64, 256].iter() {
        let data = random_data(*size);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("encode", size), size, |b, _| {
            b.iter(|| {
                let result = Base58::encode(black_box(&data));
                black_box(result)
            });
        });
    }

    for size in [20, 32, 64, 256].iter() {
        let data = random_data(*size);
        let encoded = Base58::encode(&data);
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::new("decode", size), size, |b, _| {
            b.iter(|| {
                let result = Base58::decode(black_box(&encoded)).unwrap();
                black_box(result)
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sha256,
    bench_sha512,
    bench_ripemd160,
    bench_hash160,
    bench_hash256,
    bench_keccak256,
    bench_blake2b,
    bench_secp256r1_sign,
    bench_secp256r1_verify,
    bench_secp256k1_sign,
    bench_secp256k1_verify,
    bench_ed25519_sign,
    bench_ed25519_verify,
    bench_key_generation,
    bench_base58
);
criterion_main!(benches);
