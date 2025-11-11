use alloc::vec::Vec;

use super::*;
use crate::{blockchain::BlockSummary, txpool::PendingTransaction};
use neo_base::hash::Hash256;

fn block(index: u64) -> BlockSummary {
    BlockSummary::new(
        index,
        Hash256::new([index as u8; 32]),
        Hash256::new([(index - 1) as u8; 32]),
        index * 1_000,
        1,
        200,
        10,
    )
}

#[test]
fn runtime_queues_and_commits() {
    let mut runtime = Runtime::new(100, 2);
    assert!(runtime.queue_transaction(PendingTransaction::new("tx1", 10, 120)));
    let reserved = runtime.tx_pool_mut().reserve_for_block(5, 10_000);
    assert_eq!(reserved.len(), 1);
    runtime.commit_block(block(1));
    assert_eq!(runtime.blockchain().height(), 1);
    assert!(runtime.fee_calculator().estimate(10) >= 120);
}

#[test]
fn runtime_syncs_height() {
    let mut runtime = Runtime::new(50, 1);
    runtime.sync_height(5);
    assert_eq!(runtime.blockchain().height(), 5);
}

#[test]
fn snapshot_restores_pending_transactions() {
    let mut runtime = Runtime::new(10, 1);
    runtime.queue_transaction(PendingTransaction::new("tx1", 5, 100));
    runtime.queue_transaction(PendingTransaction::new("tx2", 7, 200));
    let snapshot = runtime.snapshot();

    let mut restored = Runtime::new(1, 1);
    restored.restore_snapshot(snapshot);
    let pending: Vec<String> = restored.pending_ids().cloned().collect();
    assert_eq!(pending, vec!["tx1".to_string(), "tx2".to_string()]);
    assert_eq!(restored.tx_pool().len(), 2);
}
