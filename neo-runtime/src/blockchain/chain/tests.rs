use neo_base::hash::Hash256;

use super::super::BlockSummary;
use super::{state::RECENT_BLOCK_LIMIT, Blockchain};

fn block(index: u64, txs: usize, fees: u64, prev: Hash256) -> BlockSummary {
    let mut hash_bytes = [0u8; 32];
    hash_bytes[0..8].copy_from_slice(&index.to_le_bytes());
    BlockSummary::new(
        index,
        Hash256::new(hash_bytes),
        prev,
        index * 1_000,
        txs,
        (txs as u64) * 100,
        fees,
    )
}

#[test]
fn queue_and_apply_blocks() {
    let mut chain = Blockchain::new();
    let genesis = block(1, 1, 100, Hash256::ZERO);
    chain.apply_block(genesis);
    assert_eq!(chain.height(), 1);
    assert_eq!(chain.last_block().unwrap().transaction_count, 1);

    chain.apply_block(block(3, 0, 0, Hash256::ZERO));
    assert_eq!(chain.height(), 1);

    chain.apply_block(block(2, 0, 0, Hash256::ZERO));
    assert_eq!(chain.height(), 2);
}

#[test]
fn rollback_truncates_head() {
    let mut chain = Blockchain::new();
    let block1 = block(1, 1, 10, Hash256::ZERO);
    let block2 = block(2, 1, 10, block1.hash);
    let block3 = block(3, 1, 10, block2.hash);
    chain.apply_block(block1);
    chain.apply_block(block2);
    chain.apply_block(block3);
    assert_eq!(chain.height(), 3);

    chain.rollback_to(1);
    assert_eq!(chain.height(), 1);
    assert_eq!(chain.last_block().unwrap().index, 1);
}

#[test]
fn rollback_to_future_height_is_noop() {
    let mut chain = Blockchain::new();
    chain.apply_block(block(1, 1, 10, Hash256::ZERO));
    chain.rollback_to(5);
    assert_eq!(chain.height(), 1);
}

#[test]
fn totals_track_lifetime_chain_activity() {
    let mut chain = Blockchain::new();
    let blocks = (RECENT_BLOCK_LIMIT as u64) + 10;
    for index in 1..=blocks {
        chain.apply_block(block(index, 2, 5, Hash256::ZERO));
    }
    assert_eq!(chain.height(), blocks);
    assert_eq!(chain.total_transactions(), blocks * 2);
    assert_eq!(chain.total_fees(), blocks * 5);
    assert_eq!(chain.total_size_bytes(), blocks * 200);
    assert_eq!(chain.recent_blocks().count(), RECENT_BLOCK_LIMIT);
}

#[test]
fn snapshot_restores_recent_window() {
    let mut chain = Blockchain::new();
    let mut prev = Hash256::ZERO;
    for index in 1..=5 {
        let summary = block(index, 1, 10, prev);
        prev = summary.hash;
        chain.apply_block(summary);
    }
    let snapshot = chain.snapshot();
    let stats_before = chain.window_stats().unwrap();

    let mut restored = Blockchain::new();
    restored.restore_snapshot(snapshot);
    let stats_after = restored.window_stats().unwrap();

    assert_eq!(stats_after.block_count, stats_before.block_count);
    assert_eq!(stats_after.duration_ms, stats_before.duration_ms);
    assert_eq!(stats_after.total_bytes, stats_before.total_bytes);
}
