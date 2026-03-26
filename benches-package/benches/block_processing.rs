use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::network::p2p::payloads::Header;
use neo_core::Transaction;

/// Create a sample header with deterministic data for benchmarking.
fn make_sample_header() -> Header {
    let mut header = Header::new();
    header.set_version(0);
    header.set_timestamp(1_678_000_000);
    header.set_nonce(0xDEAD_BEEF);
    header.set_index(42);
    header.set_primary_index(0);
    header
}

/// Benchmark block header serialization.
fn bench_header_serialize(c: &mut Criterion) {
    let header = make_sample_header();

    c.bench_function("header_serialize", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            header.serialize(&mut writer).expect("serialize header");
            black_box(writer.into_bytes());
        });
    });
}

/// Benchmark block header deserialization.
fn bench_header_deserialize(c: &mut Criterion) {
    let header = make_sample_header();
    let mut writer = BinaryWriter::new();
    header.serialize(&mut writer).expect("serialize header");
    let bytes = writer.into_bytes();

    c.bench_function("header_deserialize", |b| {
        b.iter(|| {
            let mut reader = MemoryReader::new(black_box(&bytes));
            let h = Header::deserialize(&mut reader).expect("deserialize header");
            black_box(h);
        });
    });
}

/// Benchmark block header serialization + deserialization roundtrip.
fn bench_header_roundtrip(c: &mut Criterion) {
    let header = make_sample_header();

    c.bench_function("header_roundtrip", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            header.serialize(&mut writer).expect("serialize");
            let bytes = writer.into_bytes();
            let mut reader = MemoryReader::new(&bytes);
            let h = Header::deserialize(&mut reader).expect("deserialize");
            black_box(h);
        });
    });
}

/// Benchmark block header hash computation.
fn bench_header_hash(c: &mut Criterion) {
    let mut header = make_sample_header();

    c.bench_function("header_hash", |b| {
        b.iter(|| {
            // Clear cached hash to force recomputation each iteration.
            let mut h = header.clone();
            black_box(h.hash());
        });
    });
}

/// Benchmark transaction hash computation.
fn bench_transaction_hash(c: &mut Criterion) {
    let tx = Transaction::new();

    c.bench_function("transaction_hash", |b| {
        b.iter(|| {
            let t = tx.clone();
            black_box(t.hash());
        });
    });
}

/// Benchmark transaction serialization.
fn bench_transaction_serialize(c: &mut Criterion) {
    let tx = Transaction::new();

    c.bench_function("transaction_serialize", |b| {
        b.iter(|| {
            let mut writer = BinaryWriter::new();
            tx.serialize(&mut writer).expect("serialize tx");
            black_box(writer.into_bytes());
        });
    });
}

criterion_group!(
    benches,
    bench_header_serialize,
    bench_header_deserialize,
    bench_header_roundtrip,
    bench_header_hash,
    bench_transaction_hash,
    bench_transaction_serialize,
);
criterion_main!(benches);
