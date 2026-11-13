//! End-to-end empty-block persistence benchmark.
//!
//! Measures the production `NeoSystem::persist_block` path (native OnPersist,
//! ledger state, write batch/commit) on a fresh in-memory system. Transactions
//! are intentionally excluded; this isolates the block pipeline overhead that
//! every block pays before transaction execution.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_native_contracts::native::NativeHelpers;
use neo_primitives::UInt160;
use neo_primitives::UInt256;
use neo_primitives::WitnessScope;
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::Block as ExecutionBlock;
use neo_protocol::payloads::Signer;
use neo_protocol::payloads::{block::Block, transaction::Transaction};
use neo_protocol::witness::Witness;
use neo_system::NeoSystem;

fn empty_block(index: u32, prev_hash: UInt256, timestamp: u64) -> Block {
    let mut block = Block::new();
    block.header.set_index(index);
    block.header.set_prev_hash(prev_hash);
    block.header.set_merkle_root(UInt256::zero());
    block.header.set_timestamp(timestamp);
    block.header.set_next_consensus(UInt160::zero());
    block
}

fn ret_transaction(height: u32, committee: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(height);
    tx.set_system_fee(0);
    tx.set_network_fee(0);
    tx.set_valid_until_block(height.saturating_add(1_000));
    tx.set_signers(vec![Signer::new(committee, WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::new()]);
    tx.set_script(vec![0x40]); // RET
    tx
}

fn bench_persist_empty_block(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();

    let system = NeoSystem::new(ProtocolSettings::testnet(), None, None).expect("neo system");
    let mut height = system.context().ledger().current_height();
    let genesis = system.genesis_block().as_ref().clone();
    let mut prev_hash = genesis.hash();
    let mut timestamp = 1_700_000_000_000u64;

    c.bench_function("persist_empty_block", |b| {
        b.iter(|| {
            height = height.saturating_add(1);
            timestamp = timestamp.saturating_add(15_000);
            let mut block = empty_block(height, prev_hash, timestamp);
            prev_hash = block.hash();
            // `NeoSystem::persist_block` consumes the execution representation.
            let block = ExecutionBlock::from(&block);
            system.persist_block(block).expect("persist block");
            black_box(());
        });
    });
}

fn bench_persist_one_ret_transaction(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();

    let settings = ProtocolSettings::testnet();
    let committee = NativeHelpers::committee_address(&settings, None);
    let system = NeoSystem::new(settings, None, None).expect("neo system");
    let mut height = system.context().ledger().current_height();
    let genesis = system.genesis_block().as_ref().clone();
    let mut prev_hash = genesis.hash();
    let mut timestamp = 1_700_000_000_000u64;

    c.bench_function("persist_block_one_ret_tx", |b| {
        b.iter(|| {
            height = height.saturating_add(1);
            timestamp = timestamp.saturating_add(15_000);
            let mut block = empty_block(height, prev_hash, timestamp);
            block.transactions = vec![ret_transaction(height, committee)];
            block.rebuild_merkle_root();
            prev_hash = block.hash();
            // `NeoSystem::persist_block` consumes the execution representation.
            let block = ExecutionBlock::from(&block);
            system.persist_block(block).expect("persist block");
            black_box(());
        });
    });
}

fn bench_persist_ten_ret_transactions(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();

    let settings = ProtocolSettings::testnet();
    let committee = NativeHelpers::committee_address(&settings, None);
    let system = NeoSystem::new(settings, None, None).expect("neo system");
    let mut height = system.context().ledger().current_height();
    let genesis = system.genesis_block().as_ref().clone();
    let mut prev_hash = genesis.hash();
    let mut timestamp = 1_700_000_000_000u64;

    c.bench_function("persist_block_ten_ret_txs", |b| {
        b.iter(|| {
            height = height.saturating_add(1);
            timestamp = timestamp.saturating_add(15_000);
            let mut block = empty_block(height, prev_hash, timestamp);
            block.transactions = (0..10)
                .map(|offset| ret_transaction(height.saturating_add(offset), committee))
                .collect();
            block.rebuild_merkle_root();
            prev_hash = block.hash();
            // `NeoSystem::persist_block` consumes the execution representation.
            let block = ExecutionBlock::from(&block);
            system.persist_block(block).expect("persist block");
            black_box(());
        });
    });
}

fn bench_persist_consecutive_empty_blocks(c: &mut Criterion) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let _guard = runtime.enter();

    let system = NeoSystem::new(ProtocolSettings::testnet(), None, None).expect("neo system");
    let mut height = system.context().ledger().current_height();
    let genesis = system.genesis_block().as_ref().clone();
    let mut prev_hash = genesis.hash();
    let mut timestamp = 1_700_000_000_000u64;

    c.bench_function("persist_consecutive_empty_blocks_32", |b| {
        b.iter(|| {
            // Measure an explicit run of consecutive empty blocks. Every block
            // still executes all native hooks and commits in height order.
            for _ in 0..32 {
                height = height.saturating_add(1);
                timestamp = timestamp.saturating_add(15_000);
                let mut block = empty_block(height, prev_hash, timestamp);
                prev_hash = block.hash();
                system
                    .persist_block(ExecutionBlock::from(&block))
                    .expect("persist empty block");
            }
            black_box(());
        });
    });
}

criterion_group!(
    benches,
    bench_persist_empty_block,
    bench_persist_consecutive_empty_blocks,
    bench_persist_one_ret_transaction,
    bench_persist_ten_ret_transactions
);
criterion_main!(benches);
