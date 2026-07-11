use super::*;
use crate::{IndexBlockBatchEntry, NotificationIndexRecord};

#[test]
fn service_indexes_a_contiguous_block_batch_atomically() {
    let first = block_with_transactions(0, vec![transaction(1, account(1))]);
    let second = block_with_transactions(1, vec![transaction(2, account(2))]);
    let service = IndexerService::new();

    let records = service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&first),
            IndexBlockBatchEntry::block_only(&second),
        ])
        .expect("index batch");

    assert_eq!(records.len(), 2);
    assert_eq!(service.status().indexed_height, Some(1));
    assert_eq!(service.status().indexed_blocks, 2);
    assert_eq!(service.status().indexed_transactions, 2);
}

#[test]
fn invalid_later_block_does_not_partially_apply_batch() {
    let valid = block_with_transactions(0, vec![transaction(1, account(1))]);
    let duplicate = transaction(2, account(2));
    let invalid = block_with_transactions(1, vec![duplicate.clone(), duplicate]);
    let service = IndexerService::new();

    let error = service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&valid),
            IndexBlockBatchEntry::block_only(&invalid),
        ])
        .expect_err("duplicate transaction must reject the whole batch");

    assert!(matches!(error, IndexerError::DuplicateTransaction { .. }));
    assert_eq!(service.status().indexed_height, None);
    assert_eq!(service.status().indexed_blocks, 0);
    assert!(service.block_by_height(0).is_none());
}

#[test]
fn invalid_later_notification_does_not_partially_apply_batch() {
    let first = block_with_transactions(0, vec![transaction(1, account(1))]);
    let second = block_with_transactions(1, vec![transaction(2, account(2))]);
    let invalid_notification = NotificationIndexRecord {
        block_hash: first.try_hash().expect("first block hash"),
        block_height: second.index(),
        tx_hash: None,
        execution_index: 0,
        notification_index: 0,
        contract_hash: account(3),
        event_name: "Transfer".to_string(),
        trigger: "Application".to_string(),
        state_item_count: 0,
        state: Vec::new(),
        accounts: Vec::new(),
    };
    let service = IndexerService::new();

    let error = service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&first),
            IndexBlockBatchEntry::with_notifications(&second, vec![invalid_notification]),
        ])
        .expect_err("notification for another block must reject the whole batch");

    assert!(matches!(
        error,
        IndexerError::MissingNotificationBlock { .. }
    ));
    assert_eq!(service.status().indexed_height, None);
    assert_eq!(service.status().indexed_blocks, 0);
    assert!(service.block_by_height(0).is_none());
    assert!(service.block_by_height(1).is_none());
}

#[test]
fn duplicate_height_in_batch_is_rejected_before_mutation() {
    let first = block_with_transactions(0, vec![transaction(1, account(1))]);
    let second = block_with_transactions(0, vec![transaction(2, account(2))]);
    let service = IndexerService::new();

    let error = service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&first),
            IndexBlockBatchEntry::block_only(&second),
        ])
        .expect_err("one batch cannot contain two canonical blocks at one height");

    assert!(matches!(
        error,
        IndexerError::DuplicateBlockHeight { height: 0 }
    ));
    assert_eq!(service.status().indexed_blocks, 0);
}

#[test]
fn duplicate_transaction_across_batch_is_rejected_before_mutation() {
    let duplicate = transaction(3, account(3));
    let first = block_with_transactions(0, vec![duplicate.clone()]);
    let second = block_with_transactions(1, vec![duplicate]);
    let service = IndexerService::new();

    let error = service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&first),
            IndexBlockBatchEntry::block_only(&second),
        ])
        .expect_err("canonical batch cannot repeat a transaction hash");

    assert!(matches!(error, IndexerError::DuplicateTransaction { .. }));
    assert_eq!(service.status().indexed_blocks, 0);
    assert_eq!(service.status().indexed_transactions, 0);
}

#[test]
fn transaction_already_indexed_in_an_unreplaced_block_is_rejected() {
    let duplicate = transaction(4, account(4));
    let first = block_with_transactions(0, vec![duplicate.clone()]);
    let second = block_with_transactions(1, vec![duplicate]);
    let service = IndexerService::new();
    service.index_block(&first).expect("index first block");

    let error = service
        .index_block(&second)
        .expect_err("transaction cannot move to another retained block");

    assert!(matches!(error, IndexerError::DuplicateTransaction { .. }));
    assert_eq!(service.status().indexed_height, Some(0));
    assert_eq!(service.status().indexed_blocks, 1);
    assert_eq!(service.status().indexed_transactions, 1);
    assert!(service.block_by_height(1).is_none());
}

#[test]
fn clear_removes_the_entire_canonical_projection() {
    let first = block_with_transactions(0, vec![transaction(1, account(1))]);
    let second = block_with_transactions(1, vec![transaction(2, account(2))]);
    let service = IndexerService::new();
    service
        .index_block_batch([
            IndexBlockBatchEntry::block_only(&first),
            IndexBlockBatchEntry::block_only(&second),
        ])
        .expect("index batch");

    service.clear().expect("clear projection");

    assert_eq!(service.status().indexed_height, None);
    assert_eq!(service.status().indexed_blocks, 0);
    assert_eq!(service.status().indexed_transactions, 0);
    assert!(service.block_by_height(0).is_none());
    assert!(service.block_by_height(1).is_none());
}

#[test]
fn live_append_requires_the_exact_next_height_of_a_contiguous_prefix() {
    let service = IndexerService::new();
    let genesis = block_with_transactions(0, Vec::new());
    let unrelated = block_with_transactions(1, Vec::new());
    assert!(service.can_append_contiguous_block(&genesis));
    assert!(!service.can_append_contiguous_block(&unrelated));

    service.index_block(&genesis).expect("index genesis");
    let mut child = block_with_transactions(1, Vec::new());
    child
        .header
        .set_prev_hash(genesis.try_hash().expect("genesis hash"));
    assert!(service.can_append_contiguous_block(&child));
    assert!(
        !service.can_append_contiguous_block(&genesis),
        "live pre-commit indexing must not replace a tip before the committed-chain stage reconciles a reorg"
    );
    assert!(
        !service.can_append_contiguous_block(&unrelated),
        "a new chain must not append past a stale indexed tip with a different parent hash"
    );

    service
        .index_block(&block_with_transactions(2, Vec::new()))
        .expect("create historical gap");
    let mut height_three = block_with_transactions(3, Vec::new());
    height_three
        .header
        .set_prev_hash(service.status().indexed_hash.expect("indexed tip hash"));
    assert!(
        !service.can_append_contiguous_block(&height_three),
        "a max-height marker with missing rows is not a resumable checkpoint"
    );
}
