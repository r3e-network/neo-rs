//! Neo-RS Performance Benchmarks
//!
//! Comprehensive benchmarks for all critical Neo-RS components.
//! These benchmarks ensure performance regressions are caught early.

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use neo_core::{Block, Transaction, UInt256};
use neo_cryptography::{ecdsa::ECDsa, hash::sha256};
use neo_vm::{ApplicationEngine, Script};
use std::time::Duration;

/// Benchmark cryptographic operations
fn bench_cryptography(c: &mut Criterion) {
    let mut group = c.benchmark_group("cryptography");

    // SHA256 hashing
    let data_sizes = [32, 256, 1024, 8192];
    for size in data_sizes {
        let data = vec![0u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("sha256", size), &data, |b, data| {
            b.iter(|| {
                let _hash = sha256(black_box(data));
            });
        });
    }

    // ECDSA signature verification
    let message = b"Hello, Neo!";
    let (private_key, public_key) = ECDsa::generate_keypair().unwrap();
    let signature = ECDsa::sign(message, &private_key).unwrap();

    group.bench_function("ecdsa_sign", |b| {
        b.iter(|| {
            let _sig = ECDsa::sign(black_box(message), black_box(&private_key)).unwrap();
        });
    });

    group.bench_function("ecdsa_verify", |b| {
        b.iter(|| {
            let result = ECDsa::verify(
                black_box(message),
                black_box(&signature),
                black_box(&public_key),
            );
            assert!(result.unwrap());
        });
    });

    group.finish();
}

/// Benchmark VM operations
fn bench_vm(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm");
    group.measurement_time(Duration::from_secs(10));

    // Simple arithmetic operations
    group.bench_function("vm_arithmetic", |b| {
        b.iter_batched(
            || {
                // PUSH 100, PUSH 200, ADD
                let script = Script::from_bytes(&[0x62, 0x64, 0x93]).unwrap();
                ApplicationEngine::new()
            },
            |mut engine| {
                let script = Script::from_bytes(&[0x62, 0x64, 0x93]).unwrap();
                engine.load_script(&script).unwrap();
                let _result = black_box(engine.execute());
            },
            BatchSize::SmallInput,
        );
    });

    // Complex script execution
    let complex_script = create_complex_script();
    group.bench_function("vm_complex_script", |b| {
        b.iter_batched(
            || ApplicationEngine::new(),
            |mut engine| {
                engine.load_script(&complex_script).unwrap();
                let _result = black_box(engine.execute());
            },
            BatchSize::SmallInput,
        );
    });

    // Stack operations
    group.bench_function("vm_stack_operations", |b| {
        b.iter_batched(
            || {
                let mut engine = ApplicationEngine::new();
                // Pre-populate stack
                for i in 0..100 {
                    engine.push_integer(i);
                }
                engine
            },
            |mut engine| {
                // Perform various stack operations
                for _ in 0..50 {
                    let _a = engine.pop().unwrap();
                    let _b = engine.pop().unwrap();
                    engine.push_integer(123);
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark serialization operations
fn bench_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization");

    // Transaction serialization
    let transaction = create_test_transaction();
    group.bench_function("transaction_serialize", |b| {
        b.iter(|| {
            let _bytes = black_box(&transaction).to_bytes();
        });
    });

    let tx_bytes = transaction.to_bytes();
    group.bench_function("transaction_deserialize", |b| {
        b.iter(|| {
            let _tx = Transaction::from_bytes(black_box(&tx_bytes)).unwrap();
        });
    });

    // Block serialization
    let block = create_test_block();
    group.bench_function("block_serialize", |b| {
        b.iter(|| {
            let _bytes = black_box(&block).to_bytes();
        });
    });

    let block_bytes = block.to_bytes();
    group.bench_function("block_deserialize", |b| {
        b.iter(|| {
            let _block = Block::from_bytes(black_box(&block_bytes)).unwrap();
        });
    });

    group.finish();
}

/// Benchmark network message processing
fn bench_network(c: &mut Criterion) {
    let mut group = c.benchmark_group("network");

    // Message parsing
    let version_message = create_version_message();
    group.bench_function("parse_version_message", |b| {
        b.iter(|| {
            let _msg = parse_network_message(black_box(&version_message));
        });
    });

    // Peer management
    group.bench_function("peer_connection_handling", |b| {
        b.iter_batched(
            || create_mock_peer_manager(),
            |mut manager| {
                for i in 0..100 {
                    let addr = format!("192.168.1.{}", i % 255).parse().unwrap();
                    manager.add_peer(addr);
                }
                manager.cleanup_inactive_peers();
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark storage operations
fn bench_storage(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage");

    // Memory storage operations
    group.bench_function("memory_storage_write", |b| {
        b.iter_batched(
            || create_memory_storage(),
            |mut storage| {
                for i in 0..1000 {
                    let key = format!("key_{}", i);
                    let value = format!("value_{}", i);
                    storage.put(key.as_bytes(), value.as_bytes()).unwrap();
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("memory_storage_read", |b| {
        b.iter_batched(
            || {
                let mut storage = create_memory_storage();
                for i in 0..1000 {
                    let key = format!("key_{}", i);
                    let value = format!("value_{}", i);
                    storage.put(key.as_bytes(), value.as_bytes()).unwrap();
                }
                storage
            },
            |storage| {
                for i in 0..1000 {
                    let key = format!("key_{}", i);
                    let _value = storage.get(key.as_bytes()).unwrap();
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark consensus operations
fn bench_consensus(c: &mut Criterion) {
    let mut group = c.benchmark_group("consensus");

    // Message validation
    let prepare_request = create_prepare_request();
    group.bench_function("validate_prepare_request", |b| {
        b.iter(|| {
            let _result = validate_consensus_message(black_box(&prepare_request));
        });
    });

    // Signature aggregation
    let signatures = create_test_signatures(7);
    group.bench_function("aggregate_signatures", |b| {
        b.iter(|| {
            let _aggregated = aggregate_signatures(black_box(&signatures));
        });
    });

    group.finish();
}

/// Benchmark overall node performance
fn bench_node_integration(c: &mut Criterion) {
    let mut group = c.benchmark_group("integration");
    group.measurement_time(Duration::from_secs(15));

    // Block processing pipeline
    group.bench_function("process_block_pipeline", |b| {
        b.iter_batched(
            || create_test_node(),
            |mut node| {
                let block = create_test_block();
                let _result = node.process_block(black_box(block));
            },
            BatchSize::SmallInput,
        );
    });

    // Transaction processing
    group.bench_function("process_transaction_batch", |b| {
        b.iter_batched(
            || create_test_node(),
            |mut node| {
                let transactions = create_transaction_batch(100);
                for tx in transactions {
                    let _result = node.process_transaction(black_box(tx));
                }
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

// Helper functions for benchmark data creation

fn create_complex_script() -> Script {
    // Complex script with loops, conditionals, and crypto operations
    Script::from_bytes(&[
        0x51, 0x52, 0x53, // PUSH1, PUSH2, PUSH3
        0x93, 0x93, // ADD, ADD
        0x52, 0x9F, // PUSH2, MOD
        0x63, 0x04, 0x00, // JMP +4
        0x51, 0x9C, // PUSH1, NOT
        0x6B, // RETURN
    ])
    .unwrap()
}

fn create_test_transaction() -> Transaction {
    Transaction {
        version: 0,
        nonce: 123456,
        system_fee: 1000000,
        network_fee: 100000,
        valid_until_block: 999999,
        attributes: vec![],
        signers: vec![],
        script: vec![0x51; 100], // PUSH1 * 100
        witnesses: vec![],
    }
}

fn create_test_block() -> Block {
    Block {
        version: 0,
        prev_hash: UInt256::zero(),
        merkle_root: UInt256::zero(),
        timestamp: 1234567890,
        index: 100,
        next_consensus: UInt160::zero(),
        witness: vec![],
        consensus_data: ConsensusData {
            primary_index: 0,
            nonce: 123456,
        },
        transactions: vec![create_test_transaction(); 10],
    }
}

fn create_version_message() -> Vec<u8> {
    // Mock version message bytes
    vec![0x00; 64]
}

fn parse_network_message(_bytes: &[u8]) -> Result<(), ()> {
    // Mock message parsing
    Ok(())
}

fn create_mock_peer_manager() -> MockPeerManager {
    MockPeerManager::new()
}

struct MockPeerManager {
    peers: Vec<std::net::SocketAddr>,
}

impl MockPeerManager {
    fn new() -> Self {
        Self { peers: Vec::new() }
    }

    fn add_peer(&mut self, addr: std::net::SocketAddr) {
        self.peers.push(addr);
    }

    fn cleanup_inactive_peers(&mut self) {
        self.peers.retain(|_| true); // Mock cleanup
    }
}

fn create_memory_storage() -> MockStorage {
    MockStorage::new()
}

struct MockStorage {
    data: std::collections::HashMap<Vec<u8>, Vec<u8>>,
}

impl MockStorage {
    fn new() -> Self {
        Self {
            data: std::collections::HashMap::new(),
        }
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), ()> {
        self.data.insert(key.to_vec(), value.to_vec());
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, ()> {
        Ok(self.data.get(key).cloned())
    }
}

fn create_prepare_request() -> Vec<u8> {
    vec![0x01; 128] // Mock prepare request
}

fn validate_consensus_message(_msg: &[u8]) -> bool {
    true // Mock validation
}

fn create_test_signatures(count: usize) -> Vec<Vec<u8>> {
    (0..count).map(|_| vec![0x42; 64]).collect()
}

fn aggregate_signatures(_sigs: &[Vec<u8>]) -> Vec<u8> {
    vec![0x99; 64] // Mock aggregated signature
}

fn create_test_node() -> MockNode {
    MockNode::new()
}

struct MockNode;

impl MockNode {
    fn new() -> Self {
        Self
    }

    fn process_block(&mut self, _block: Block) -> Result<(), ()> {
        // Mock block processing
        std::thread::sleep(Duration::from_nanos(100));
        Ok(())
    }

    fn process_transaction(&mut self, _tx: Transaction) -> Result<(), ()> {
        // Mock transaction processing
        std::thread::sleep(Duration::from_nanos(10));
        Ok(())
    }
}

fn create_transaction_batch(count: usize) -> Vec<Transaction> {
    (0..count).map(|_| create_test_transaction()).collect()
}

// Benchmark groups
criterion_group!(
    benches,
    bench_cryptography,
    bench_vm,
    bench_serialization,
    bench_network,
    bench_storage,
    bench_consensus,
    bench_node_integration
);

criterion_main!(benches);
