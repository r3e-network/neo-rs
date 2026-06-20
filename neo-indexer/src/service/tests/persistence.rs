use super::*;

#[test]
fn persistent_service_round_trips_snapshot_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("indexer.json");
    let mut header = Header::new();
    header.set_index(3);
    let block = Block::from_parts(header, Vec::new());

    let service = IndexerService::open(&path).expect("open empty");
    assert_eq!(service.snapshot_path(), Some(path.as_path()));
    let record = service.index_block(&block).expect("index block");
    assert!(path.exists(), "snapshot file is written");

    let restored = IndexerService::open(&path).expect("restore");

    assert_eq!(restored.status().indexed_height, Some(3));
    assert_eq!(restored.block_by_hash(&record.hash), Some(record));
}

#[test]
fn persistent_service_does_not_leave_temporary_snapshot_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("indexer.json");
    let temp_path = temporary_snapshot_path(&path);
    let service = IndexerService::open(&path).expect("open empty");

    for height in 1..=2 {
        let mut header = Header::new();
        header.set_index(height);
        service
            .index_block(&Block::from_parts(header, Vec::new()))
            .expect("index block");

        assert!(path.exists(), "snapshot file is committed");
        assert!(
            !temp_path.exists(),
            "temporary snapshot file is renamed away"
        );
    }

    let restored = IndexerService::open(&path).expect("restore");

    assert_eq!(restored.status().indexed_height, Some(2));
}

#[test]
fn write_snapshot_removes_temporary_file_when_commit_fails() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("indexer.json");
    let temp_path = temporary_snapshot_path(&path);
    std::fs::create_dir(&path).expect("target directory");

    let err = write_snapshot(&path, &IndexerSnapshot::new(Vec::new(), Vec::new()))
        .expect_err("directory target should reject snapshot commit");

    assert!(matches!(err, IndexerError::SnapshotWrite { .. }));
    assert!(
        path.is_dir(),
        "failed commit leaves existing directory intact"
    );
    assert!(
        !temp_path.exists(),
        "failed commit should remove temporary snapshot file"
    );
}

#[test]
fn persistent_service_rolls_back_memory_after_snapshot_commit_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("indexer.json");
    let service = IndexerService::open(&path).expect("open empty");
    std::fs::create_dir(&path).expect("target directory");

    let mut header = Header::new();
    header.set_index(5);
    let block = Block::from_parts(header, Vec::new());

    let err = service
        .index_block(&block)
        .expect_err("directory target should reject snapshot commit");

    assert!(matches!(err, IndexerError::SnapshotWrite { .. }));
    assert_eq!(service.status().indexed_blocks, 0);
    assert_eq!(service.status().indexed_height, None);
    assert!(service.block_by_height(5).is_none());
}

#[test]
fn persistent_service_persists_reverts() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("nested").join("indexer.json");
    let service = IndexerService::open(&path).expect("open empty");

    for height in 1..=3 {
        let mut header = Header::new();
        header.set_index(height);
        service
            .index_block(&Block::from_parts(header, Vec::new()))
            .expect("index block");
    }
    service.revert_to_height(1).expect("revert");

    let restored = IndexerService::open(&path).expect("restore");

    assert_eq!(restored.status().indexed_height, Some(1));
    assert!(restored.block_by_height(2).is_none());
    assert!(restored.block_by_height(3).is_none());
}

#[test]
fn persistent_service_round_trips_notifications() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("indexer.json");
    let signer = account(1);
    let recipient = account(2);
    let contract = account(3);
    let tx = transaction(80, signer);
    let tx_hash = tx.try_hash().expect("tx hash");
    let mut header = Header::new();
    header.set_index(4);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let service = IndexerService::open(&path).expect("open empty");
    service
        .index_block_with_application_executions(
            &block,
            &[execution(
                tx,
                contract,
                "Transfer",
                transfer_state(signer, recipient, 7),
            )],
        )
        .expect("index block notifications");

    let restored = IndexerService::open(&path).expect("restore");

    let records = restored.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].contract_hash, contract);
    assert_eq!(records[0].event_name, "Transfer");
    assert_eq!(records[0].state_item_count, 3);
    assert_eq!(records[0].state[0]["type"], "ByteString");
    assert_eq!(records[0].state[1]["type"], "ByteString");
    assert_eq!(records[0].state[2]["type"], "Integer");
    assert_eq!(records[0].state[2]["value"], "7");
    assert_eq!(records[0].accounts, vec![signer, recipient]);
    assert_eq!(restored.notifications_for_account(&signer, 0, 10), records);
    assert_eq!(
        restored.notifications_for_account(&recipient, 0, 10),
        records
    );
}
