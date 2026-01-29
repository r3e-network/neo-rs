//! Serialization/Deserialization Benchmarks
//!
//! Benchmarks for transaction serialization, block serialization, and other
//! data structure encoding/decoding operations in Neo core.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use neo_core::{
    network::p2p::payloads::{
        transaction::Transaction, Header, Signer, Witness,
    },
    extensions::SerializableExtensions,
    io::{BinaryWriter, MemoryReader, Serializable},
};
use neo_primitives::{UInt160, UInt256};
use rand::{rngs::OsRng, RngCore};

// Generate random bytes
fn random_bytes(size: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; size];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn bench_transaction_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_transaction");
    
    // Create transactions of different sizes
    for signer_count in [1, 2, 3].iter() {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(12345);
        tx.set_system_fee(1000000);
        tx.set_network_fee(500000);
        tx.set_valid_until_block(1000);
        
        // Add signers and matching witnesses
        for _ in 0..*signer_count {
            let signer = Signer::new(
                UInt160::from_bytes(&random_bytes(20)).unwrap(),
                neo_core::network::p2p::payloads::WitnessScope::None,
            );
            tx.add_signer(signer);
            
            let witness = Witness::new_with_scripts(random_bytes(32), random_bytes(32));
            tx.add_witness(witness);
        }
        
        // Add script
        tx.set_script(random_bytes(256));
        
        group.bench_with_input(
            BenchmarkId::new("serialize", signer_count),
            signer_count,
            |b, _| {
                b.iter(|| {
                    let result = black_box(&tx).to_array().unwrap();
                    black_box(result)
                });
            },
        );
        
        let serialized = tx.to_array().unwrap();
        group.throughput(Throughput::Bytes(serialized.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("deserialize", signer_count),
            signer_count,
            |b, _| {
                b.iter(|| {
                    let result = Transaction::from_bytes(&serialized).unwrap();
                    black_box(result)
                });
            },
        );
    }
    
    group.finish();
}

fn bench_header_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_header");
    
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::from_bytes(&random_bytes(32)).unwrap());
    header.set_merkle_root(UInt256::from_bytes(&random_bytes(32)).unwrap());
    header.set_timestamp(1234567890);
    header.set_index(100);
    header.set_primary_index(0);
    
    group.bench_function("serialize", |b| {
        b.iter(|| {
            let result = black_box(&header).to_array().unwrap();
            black_box(result)
        });
    });
    
    let serialized = header.to_array().unwrap();
    group.throughput(Throughput::Bytes(serialized.len() as u64));
    group.bench_function("deserialize", |b| {
        b.iter(|| {
            let mut reader = MemoryReader::new(&serialized);
            let result = Header::deserialize(&mut reader).unwrap();
            black_box(result)
        });
    });
    
    group.finish();
}

fn bench_binary_writer(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_binary");
    
    // Benchmark writing different data types
    group.bench_function("write_u8", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            writer.write_u8(black_box(42)).unwrap();
            black_box(writer.into_bytes())
        });
    });
    
    group.bench_function("write_u32", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            writer.write_u32(black_box(0xDEADBEEFu32)).unwrap();
            black_box(writer.into_bytes())
        });
    });
    
    group.bench_function("write_u64", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            writer.write_u64(black_box(0xDEADBEEFCAFEBABEu64)).unwrap();
            black_box(writer.into_bytes())
        });
    });
    
    group.bench_function("write_bytes_256", |b| {
        let data = random_bytes(256);
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            writer.write_bytes(black_box(&data)).unwrap();
            black_box(writer.into_bytes())
        });
    });
    
    group.finish();
}

fn bench_hash_computation(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization_hash");
    
    // Create a transaction and benchmark its hash computation
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(12345);
    tx.set_system_fee(1000000);
    tx.set_network_fee(500000);
    tx.set_valid_until_block(1000);
    
    // Add a signer and matching witness
    let signer = Signer::new(
        UInt160::from_bytes(&random_bytes(20)).unwrap(),
        neo_core::network::p2p::payloads::WitnessScope::None,
    );
    tx.add_signer(signer);
    let witness = Witness::new_with_scripts(random_bytes(32), random_bytes(32));
    tx.add_witness(witness);
    
    tx.set_script(random_bytes(256));
    
    group.bench_function("transaction_hash", |b| {
        b.iter(|| {
            let result = black_box(&mut tx).hash();
            black_box(result)
        });
    });
    
    // Create a block header and benchmark its hash
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::from_bytes(&random_bytes(32)).unwrap());
    header.set_merkle_root(UInt256::from_bytes(&random_bytes(32)).unwrap());
    header.set_timestamp(1234567890);
    header.set_index(100);
    header.set_primary_index(0);
    
    group.bench_function("header_hash", |b| {
        b.iter(|| {
            let result = black_box(&mut header).hash();
            black_box(result)
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_transaction_serialization,
    bench_header_serialization,
    bench_binary_writer,
    bench_hash_computation
);
criterion_main!(benches);
