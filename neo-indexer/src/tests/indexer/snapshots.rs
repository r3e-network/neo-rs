use super::*;

#[test]
fn snapshot_round_trips_lookup_tables() {
    let first = account(1);
    let second = account(2);
    let mut indexer = Indexer::new();
    let block1 = block(1, 1, vec![transaction(50, &[first])]);
    let block2 = block(2, 2, vec![transaction(51, &[first, second])]);
    let block2_hash = block2.try_hash().expect("block hash");
    indexer.index_block(&block1).expect("block1");
    indexer.index_block(&block2).expect("block2");

    let restored = Indexer::from_snapshot(indexer.snapshot()).expect("restore snapshot");

    assert_eq!(restored.status(), indexer.status());
    assert_eq!(
        restored.block_by_height(2).expect("height").hash,
        block2_hash
    );
    assert_eq!(restored.transactions_for_account(&first, 0, 10).len(), 2);
    assert_eq!(restored.transactions_for_account(&second, 0, 10).len(), 1);
}

#[test]
fn snapshot_round_trips_notifications() {
    let contract = account(8);
    let sender = account(1);
    let recipient = account(2);
    let tx = transaction(70, &[sender]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(12, 12, vec![tx.clone()]);

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_application_executions(
            &block,
            &[execution_with_state(
                Some(tx),
                contract,
                "Transfer",
                transfer_state(Some(sender), Some(recipient), 9),
            )],
        )
        .expect("index block");

    let restored = Indexer::from_snapshot(indexer.snapshot()).expect("restore snapshot");

    let records = restored.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(restored.notifications_for_account(&sender, 0, 10), records);
    assert_eq!(
        restored.notifications_for_account(&recipient, 0, 10),
        records
    );
}

#[test]
fn snapshot_rejects_unsupported_versions() {
    for version in [
        0,
        INDEXER_SNAPSHOT_VERSION - 1,
        INDEXER_SNAPSHOT_VERSION + 1,
    ] {
        let mut snapshot = IndexerSnapshot::new(Vec::new(), Vec::new());
        snapshot.version = version;

        let err = Indexer::from_snapshot(snapshot).expect_err("unsupported version");

        assert!(matches!(
            err,
            IndexerError::UnsupportedSnapshotVersion { version: observed } if observed == version
        ));
    }
}

#[test]
fn snapshot_rejects_missing_transaction_block() {
    let hash = hash256(1);
    let snapshot = IndexerSnapshot::new(
        Vec::new(),
        vec![TransactionIndexRecord {
            hash,
            block_hash: hash256(2),
            block_height: 7,
            transaction_index: 0,
            signers: Vec::new(),
        }],
    );

    let err = Indexer::from_snapshot(snapshot).expect_err("invalid snapshot");

    assert!(matches!(
        err,
        IndexerError::MissingTransactionBlock {
            hash: observed,
            ..
        } if observed == hash
    ));
}

#[test]
fn snapshot_rejects_transaction_index_outside_block_count() {
    let block_hash = hash256(2);
    let tx_hash = hash256(3);
    let snapshot = IndexerSnapshot::new(
        vec![BlockIndexRecord {
            hash: block_hash,
            height: 7,
            timestamp: 1_700_000_000_000,
            transaction_count: 1,
        }],
        vec![TransactionIndexRecord {
            hash: tx_hash,
            block_hash,
            block_height: 7,
            transaction_index: 1,
            signers: Vec::new(),
        }],
    );

    let err = Indexer::from_snapshot(snapshot).expect_err("invalid snapshot");

    assert!(matches!(
        err,
        IndexerError::TransactionIndexOutOfBounds {
            hash,
            block_hash: observed_block,
            transaction_index: 1,
            transaction_count: 1,
        } if hash == tx_hash && observed_block == block_hash
    ));
}

#[test]
fn snapshot_rejects_duplicate_transaction_position() {
    let block_hash = hash256(4);
    let snapshot = IndexerSnapshot::new(
        vec![BlockIndexRecord {
            hash: block_hash,
            height: 8,
            timestamp: 1_700_000_000_000,
            transaction_count: 2,
        }],
        vec![
            TransactionIndexRecord {
                hash: hash256(5),
                block_hash,
                block_height: 8,
                transaction_index: 0,
                signers: Vec::new(),
            },
            TransactionIndexRecord {
                hash: hash256(6),
                block_hash,
                block_height: 8,
                transaction_index: 0,
                signers: Vec::new(),
            },
        ],
    );

    let err = Indexer::from_snapshot(snapshot).expect_err("invalid snapshot");

    assert!(matches!(
        err,
        IndexerError::DuplicateTransactionPosition {
            block_hash: observed_block,
            block_height: 8,
            transaction_index: 0,
        } if observed_block == block_hash
    ));
}

#[test]
fn snapshot_rejects_block_transaction_count_mismatch() {
    let block_hash = hash256(7);
    let snapshot = IndexerSnapshot::new(
        vec![BlockIndexRecord {
            hash: block_hash,
            height: 9,
            timestamp: 1_700_000_000_000,
            transaction_count: 2,
        }],
        vec![TransactionIndexRecord {
            hash: hash256(8),
            block_hash,
            block_height: 9,
            transaction_index: 0,
            signers: Vec::new(),
        }],
    );

    let err = Indexer::from_snapshot(snapshot).expect_err("invalid snapshot");

    assert!(matches!(
        err,
        IndexerError::TransactionCountMismatch {
            block_hash: observed_block,
            block_height: 9,
            expected: 2,
            actual: 1,
        } if observed_block == block_hash
    ));
}

#[test]
fn snapshot_restores_transactions_in_block_position_order() {
    let block_hash = hash256(9);
    let tx0 = hash256(10);
    let tx1 = hash256(11);
    let snapshot = IndexerSnapshot::new(
        vec![BlockIndexRecord {
            hash: block_hash,
            height: 10,
            timestamp: 1_700_000_000_000,
            transaction_count: 2,
        }],
        vec![
            TransactionIndexRecord {
                hash: tx1,
                block_hash,
                block_height: 10,
                transaction_index: 1,
                signers: Vec::new(),
            },
            TransactionIndexRecord {
                hash: tx0,
                block_hash,
                block_height: 10,
                transaction_index: 0,
                signers: Vec::new(),
            },
        ],
    );

    let restored = Indexer::from_snapshot(snapshot).expect("valid snapshot");

    let tx_hashes = restored
        .snapshot()
        .transactions
        .into_iter()
        .map(|record| record.hash)
        .collect::<Vec<_>>();
    assert_eq!(tx_hashes, vec![tx0, tx1]);
}
