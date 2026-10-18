//! RocksDB production-path empty-block persistence benchmark.
//!
//! Requires `--features rocksdb`. Measures `NeoSystem::persist_block` with the
//! durable RocksDB provider (WAL + write batch), including native OnPersist,
//! ledger updates, and commit.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_core::UInt160;
use neo_core::UInt256;
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::payloads::Block;
use neo_core::persistence::providers::RocksDBStoreProvider;
use neo_core::persistence::storage::StorageConfig;
use neo_core::persistence::store_provider::StoreProvider;
use neo_core::protocol_settings::ProtocolSettings;
use std::sync::Arc;

fn empty_block(index: u32, prev_hash: UInt256, timestamp: u64) -> Block {
    let mut block = Block::new();
    block.header.set_index(index);
    block.header.set_prev_hash(prev_hash);
    block.header.set_merkle_root(UInt256::zero());
    block.header.set_timestamp(timestamp);
    block.header.set_next_consensus(UInt160::zero());
    block
}

fn bench_rocksdb_persist_empty_block(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();

    let dir = tempfile::tempdir().expect("tempdir");
    let provider: Arc<dyn StoreProvider> = Arc::new(RocksDBStoreProvider::new(StorageConfig {
        path: dir.path().to_path_buf(),
        ..StorageConfig::default()
    }));
    let system = NeoSystem::new(ProtocolSettings::testnet(), Some(provider), None)
        .expect("neo system with rocksdb");

    let mut height = system.context().ledger().current_height();
    let mut genesis = system.genesis_block().as_ref().clone();
    let mut prev_hash = genesis.hash();
    let mut timestamp = 1_700_000_000_000u64;

    c.bench_function("rocksdb_persist_empty_block", |b| {
        b.iter(|| {
            height = height.saturating_add(1);
            timestamp = timestamp.saturating_add(15_000);
            let mut block = empty_block(height, prev_hash, timestamp);
            prev_hash = block.hash();
            black_box(system.persist_block(block).expect("persist block"));
        });
    });
}

criterion_group!(benches, bench_rocksdb_persist_empty_block);
criterion_main!(benches);
