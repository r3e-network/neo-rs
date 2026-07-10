use super::*;
use crate::IndexerStatus;

#[test]
fn store_backed_service_round_trips_prefixed_records() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let store_path = PathBuf::from("Indexer_004F454E");
    let signer = account(4);
    let recipient = account(5);
    let contract = account(6);
    let tx = transaction(90, signer);
    let tx_hash = tx.try_hash().expect("tx hash");
    let mut header = Header::new();
    header.set_index(11);
    let mut block = Block::from_parts(header, vec![tx.clone()]);
    block.try_rebuild_merkle_root().expect("merkle root");

    let service =
        IndexerService::open_store_with_path(Arc::clone(&store), Some(store_path.clone()))
            .expect("open store indexer");
    assert!(service.is_persistent());
    assert_eq!(service.persistence_mode(), "service-store");
    assert_eq!(service.snapshot_path(), None);
    assert_eq!(service.store_path(), Some(store_path.as_path()));
    service
        .index_block_with_application_executions(
            &block,
            &[execution(
                tx,
                contract,
                "Transfer",
                transfer_state(signer, recipient, 9),
            )],
        )
        .expect("index block notifications");
    service
        .flush_durable()
        .expect("fence persistent indexer records");

    let restored =
        IndexerService::open_store_with_path(Arc::clone(&store), Some(store_path.clone()))
            .expect("restore store indexer");

    assert_eq!(restored.status().indexed_height, Some(11));
    assert_eq!(restored.transaction(&tx_hash).expect("tx").block_height, 11);
    assert_eq!(
        restored.notifications_for_account(&recipient, 0, 10)[0].contract_hash,
        contract
    );
    assert_eq!(count_store_rows(&store, BLOCK_BY_HEIGHT_PREFIX), 1);
    assert_eq!(count_store_rows(&store, BLOCK_BY_HASH_PREFIX), 1);
    assert_eq!(count_store_rows(&store, TRANSACTION_BY_CHAIN_PREFIX), 1);
    assert_eq!(count_store_rows(&store, TRANSACTION_BY_HASH_PREFIX), 1);
    assert_eq!(count_store_rows(&store, ACCOUNT_TRANSACTION_PREFIX), 1);
    assert_eq!(count_store_rows(&store, NOTIFICATION_BY_CHAIN_PREFIX), 1);
    assert_eq!(count_store_rows(&store, NOTIFICATION_BY_BLOCK_PREFIX), 1);
    assert_eq!(
        count_store_rows(&store, NOTIFICATION_BY_TRANSACTION_PREFIX),
        1
    );
    assert_eq!(count_store_rows(&store, NOTIFICATION_BY_CONTRACT_PREFIX), 1);
    assert_eq!(count_store_rows(&store, NOTIFICATION_BY_ACCOUNT_PREFIX), 2);
    assert!(
        store
            .snapshot()
            .try_get(&LEGACY_STORE_SNAPSHOT_KEY.to_vec())
            .is_none()
    );
}

#[test]
fn store_backed_queries_read_prefix_records_without_memory_index() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let service = IndexerService::open_store(Arc::clone(&store)).expect("open store indexer");
    let signer = account(31);
    let other_signer = account(32);
    let recipient = account(33);
    let contract = account(34);
    let tx0 = transaction(130, signer);
    let tx0_hash = tx0.try_hash().expect("tx0 hash");
    let tx1 = transaction(131, other_signer);
    let tx1_hash = tx1.try_hash().expect("tx1 hash");
    let block = block_with_transactions(18, vec![tx0, tx1.clone()]);
    let block_hash = block.try_hash().expect("block hash");

    service
        .index_block_with_application_executions(
            &block,
            &[execution(
                tx1,
                contract,
                "Transfer",
                transfer_state(other_signer, recipient, 11),
            )],
        )
        .expect("index block notifications");

    *service.inner.write() = Indexer::new();

    assert_eq!(
        service.status(),
        IndexerStatus {
            indexed_height: Some(18),
            indexed_hash: Some(block_hash),
            indexed_blocks: 1,
            indexed_transactions: 2,
            indexed_accounts: 2,
            indexed_notifications: 1,
            indexed_notification_accounts: 2,
        }
    );
    assert_eq!(service.block_by_height(18).expect("block").hash, block_hash);
    assert_eq!(
        service.block_by_hash(&block_hash).expect("block").height,
        18
    );
    assert_eq!(service.blocks(0, 10).len(), 1);
    assert!(service.blocks(1, 10).is_empty());
    assert_eq!(
        service
            .transaction(&tx0_hash)
            .expect("tx0")
            .transaction_index,
        0
    );
    assert_eq!(
        service
            .transaction(&tx1_hash)
            .expect("tx1")
            .transaction_index,
        1
    );

    let block_transactions = service.transactions_for_block(&block_hash, 0, 10);
    assert_eq!(
        block_transactions
            .iter()
            .map(|record| record.hash)
            .collect::<Vec<_>>(),
        vec![tx0_hash, tx1_hash]
    );
    assert_eq!(
        service.transactions_for_account(&other_signer, 0, 10)[0].tx_hash,
        tx1_hash
    );
    assert_eq!(
        service.transactions_for_contract(&contract, Some("Transfer"), 0, 10)[0].hash,
        tx1_hash
    );
    assert!(
        service
            .transactions_for_contract(&contract, Some("Approval"), 0, 10)
            .is_empty()
    );

    let block_notifications = service.notifications_for_block(&block_hash, 0, 10);
    assert_eq!(block_notifications.len(), 1);
    assert_eq!(block_notifications[0].tx_hash, Some(tx1_hash));
    assert_eq!(
        service.notifications_for_transaction(&tx1_hash, 0, 10),
        block_notifications
    );
    assert_eq!(
        service.notifications_for_contract(&contract, Some("Transfer"), 0, 10),
        block_notifications
    );
    assert_eq!(
        service.notifications_for_account(&recipient, 0, 10),
        block_notifications
    );
    assert!(
        service
            .notifications_for_account(&recipient, 1, 10)
            .is_empty()
    );
}

#[test]
fn store_backed_activity_queries_match_memory_order_pagination_and_deduplication() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let store_service = IndexerService::open_store(Arc::clone(&store)).expect("open store indexer");
    let memory_service = IndexerService::new();

    let signer = account(41);
    let recipient = account(42);
    let contract = account(43);
    let other_contract = account(44);

    let tx_height_3 = transaction(203, signer);
    let tx_height_1 = transaction(201, signer);
    let tx_height_2 = transaction(202, signer);
    let height_3_hash = tx_height_3.try_hash().expect("height 3 tx hash");
    let height_1_hash = tx_height_1.try_hash().expect("height 1 tx hash");
    let height_2_hash = tx_height_2.try_hash().expect("height 2 tx hash");

    let cases = [
        (
            block_with_transactions(3, vec![tx_height_3.clone()]),
            vec![execution(
                tx_height_3.clone(),
                contract,
                "Transfer",
                transfer_state(signer, recipient, 3),
            )],
        ),
        (
            block_with_transactions(1, vec![tx_height_1.clone()]),
            vec![execution_with_notifications(
                tx_height_1.clone(),
                vec![
                    (contract, "Transfer", transfer_state(signer, recipient, 1)),
                    (contract, "Approval", Vec::new()),
                    (
                        other_contract,
                        "Transfer",
                        transfer_state(signer, recipient, 10),
                    ),
                ],
            )],
        ),
        (
            block_with_transactions(2, vec![tx_height_2.clone()]),
            vec![execution(
                tx_height_2.clone(),
                contract,
                "Transfer",
                transfer_state(signer, recipient, 2),
            )],
        ),
    ];

    for (block, executions) in &cases {
        memory_service
            .index_block_with_application_executions(block, executions)
            .expect("index memory service");
        store_service
            .index_block_with_application_executions(block, executions)
            .expect("index store service");
    }

    *store_service.inner.write() = Indexer::new();

    assert_eq!(store_service.blocks(0, 10), memory_service.blocks(0, 10));
    assert_eq!(
        store_service
            .blocks(0, 10)
            .iter()
            .map(|record| record.height)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        store_service.transactions_for_account(&signer, 0, 10),
        memory_service.transactions_for_account(&signer, 0, 10)
    );
    assert_eq!(
        store_service
            .transactions_for_account(&signer, 0, 10)
            .iter()
            .map(|record| record.tx_hash)
            .collect::<Vec<_>>(),
        vec![height_1_hash, height_2_hash, height_3_hash]
    );
    assert_eq!(
        store_service.transactions_for_account(&signer, 1, 1),
        memory_service.transactions_for_account(&signer, 1, 1)
    );
    assert_eq!(
        store_service.transactions_for_contract(&contract, None, 0, 10),
        memory_service.transactions_for_contract(&contract, None, 0, 10)
    );
    assert_eq!(
        store_service
            .transactions_for_contract(&contract, None, 0, 10)
            .iter()
            .map(|record| record.hash)
            .collect::<Vec<_>>(),
        vec![height_1_hash, height_2_hash, height_3_hash]
    );
    assert_eq!(
        store_service
            .transactions_for_contract(&contract, Some("Approval"), 0, 10)
            .iter()
            .map(|record| record.hash)
            .collect::<Vec<_>>(),
        vec![height_1_hash]
    );
    assert_eq!(
        store_service.notifications_for_contract(&contract, Some("Transfer"), 1, 1),
        memory_service.notifications_for_contract(&contract, Some("Transfer"), 1, 1)
    );
    assert_eq!(
        store_service.notifications_for_account(&recipient, 0, 10),
        memory_service.notifications_for_account(&recipient, 0, 10)
    );
    assert_eq!(
        store_service
            .notifications_for_account(&recipient, 0, 10)
            .iter()
            .map(|record| record.block_height)
            .collect::<Vec<_>>(),
        vec![1, 1, 2, 3]
    );
}

#[test]
fn store_backed_service_migrates_legacy_snapshot_record() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let mut header = Header::new();
    header.set_index(7);
    let block = Block::from_parts(header, Vec::new());
    let block_hash = block.try_hash().expect("block hash");
    put_legacy_store_snapshot(
        &store,
        &IndexerSnapshot::new(
            vec![BlockIndexRecord {
                hash: block_hash,
                height: 7,
                timestamp: block.timestamp(),
                transaction_count: 0,
            }],
            Vec::new(),
        ),
    );

    let restored = IndexerService::open_store(Arc::clone(&store)).expect("restore legacy");

    assert_eq!(restored.status().indexed_height, Some(7));
    assert_eq!(restored.block_by_height(7).expect("block").hash, block_hash);
    assert_eq!(count_store_rows(&store, BLOCK_BY_HEIGHT_PREFIX), 1);
    assert!(
        store
            .snapshot()
            .try_get(&LEGACY_STORE_SNAPSHOT_KEY.to_vec())
            .is_none()
    );
}

#[test]
fn store_backed_service_persists_reverts() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let service = IndexerService::open_store(Arc::clone(&store)).expect("open store indexer");

    for height in 1..=3 {
        let mut header = Header::new();
        header.set_index(height);
        service
            .index_block(&Block::from_parts(header, Vec::new()))
            .expect("index block");
    }
    service.revert_to_height(1).expect("revert");

    let restored = IndexerService::open_store(Arc::clone(&store)).expect("restore store indexer");

    assert_eq!(restored.status().indexed_height, Some(1));
    assert!(restored.block_by_height(2).is_none());
    assert!(restored.block_by_height(3).is_none());
    assert_eq!(count_store_rows(&store, BLOCK_BY_HEIGHT_PREFIX), 1);
}

#[test]
fn store_backed_service_delta_deletes_replaced_height_records() {
    let store = MemoryStoreProvider::new()
        .get_store("")
        .expect("memory store");
    let service = IndexerService::open_store(Arc::clone(&store)).expect("open store indexer");
    let old_signer = account(21);
    let new_signer = account(22);
    let old_tx = transaction(121, old_signer);
    let old_tx_hash = old_tx.try_hash().expect("old tx hash");
    let new_tx = transaction(122, new_signer);
    let new_tx_hash = new_tx.try_hash().expect("new tx hash");

    service
        .index_block(&block_with_transactions(42, vec![old_tx]))
        .expect("index old block");
    let old_record = service.transaction(&old_tx_hash).expect("old tx record");
    let old_tx_key = transaction_by_hash_key(&old_tx_hash);
    let old_account_key = account_transaction_key(&old_signer, &old_record);

    service
        .index_block(&block_with_transactions(42, vec![new_tx]))
        .expect("replace height");

    let snapshot = store.snapshot();
    assert!(snapshot.try_get(&old_tx_key).is_none());
    assert!(snapshot.try_get(&old_account_key).is_none());
    assert!(
        snapshot
            .try_get(&transaction_by_hash_key(&new_tx_hash))
            .is_some()
    );
    assert_eq!(count_store_rows(&store, BLOCK_BY_HEIGHT_PREFIX), 1);
    assert_eq!(count_store_rows(&store, TRANSACTION_BY_HASH_PREFIX), 1);
    assert_eq!(count_store_rows(&store, ACCOUNT_TRANSACTION_PREFIX), 1);

    let restored = IndexerService::open_store(store).expect("restore store indexer");
    assert!(restored.transaction(&old_tx_hash).is_none());
    assert!(
        restored
            .transactions_for_account(&old_signer, 0, 10)
            .is_empty()
    );
    assert_eq!(
        restored
            .transaction(&new_tx_hash)
            .expect("new tx")
            .block_height,
        42
    );
    assert_eq!(
        restored.transactions_for_account(&new_signer, 0, 10).len(),
        1
    );
}
