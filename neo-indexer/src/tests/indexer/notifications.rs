use super::*;

#[test]
fn indexes_notifications_by_block_transaction_and_contract() {
    let signer = account(8);
    let recipient = account(2);
    let contract = account(9);
    let tx = transaction(60, &[signer]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(10, 10, vec![tx.clone()]);

    let mut indexer = Indexer::new();
    let block_record = indexer
        .index_block_with_application_executions(
            &block,
            &[execution_with_state(
                Some(tx),
                contract,
                "Transfer",
                transfer_state(Some(signer), Some(recipient), 42),
            )],
        )
        .expect("index block notifications");

    let by_contract = indexer.notifications_for_contract(&contract, Some("Transfer"), 0, 10);
    assert_eq!(by_contract.len(), 1);
    assert_eq!(by_contract[0].tx_hash, Some(tx_hash));
    assert_eq!(by_contract[0].trigger, "Application");
    assert_eq!(by_contract[0].state_item_count, 3);
    assert_eq!(by_contract[0].state[0]["type"], "ByteString");
    assert_eq!(by_contract[0].state[1]["type"], "ByteString");
    assert_eq!(by_contract[0].state[2]["type"], "Integer");
    assert_eq!(by_contract[0].state[2]["value"], "42");
    assert_eq!(by_contract[0].accounts, vec![signer, recipient]);

    let by_transaction = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(by_transaction, by_contract);

    let by_block = indexer.notifications_for_block(&block_record.hash, 0, 10);
    assert_eq!(by_block, by_contract);
    assert_eq!(
        indexer.notifications_for_account(&signer, 0, 10),
        by_contract
    );
    assert_eq!(
        indexer.notifications_for_account(&recipient, 0, 10),
        by_contract
    );
    assert_eq!(indexer.status().indexed_notifications, 1);
    assert_eq!(indexer.status().indexed_notification_accounts, 2);
}

#[test]
fn indexes_recovered_notification_records_and_derives_transfer_accounts() {
    let signer = account(1);
    let recipient = account(2);
    let contract = account(9);
    let tx = transaction(65, &[signer]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(15, 15, vec![tx]);
    let block_hash = block.try_hash().expect("block hash");
    let state = transfer_state(Some(signer), Some(recipient), 77);

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_notification_records(
            &block,
            vec![NotificationIndexRecord {
                block_hash,
                block_height: block.index(),
                tx_hash: Some(tx_hash),
                execution_index: 1,
                notification_index: 0,
                contract_hash: contract,
                event_name: "Transfer".to_string(),
                trigger: "Application".to_string(),
                state_item_count: 3,
                state: state_json(&state),
                accounts: Vec::new(),
            }],
        )
        .expect("index recovered notification");

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].accounts, vec![signer, recipient]);
    assert_eq!(indexer.notifications_for_account(&signer, 0, 10), records);
    assert_eq!(
        indexer.notifications_for_account(&recipient, 0, 10),
        records
    );
}

#[test]
fn indexes_recovered_notification_records_normalizes_supplied_accounts() {
    let signer = account(1);
    let recipient = account(2);
    let contract = account(9);
    let tx = transaction(66, &[signer]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(16, 16, vec![tx]);
    let block_hash = block.try_hash().expect("block hash");

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_notification_records(
            &block,
            vec![NotificationIndexRecord {
                block_hash,
                block_height: block.index(),
                tx_hash: Some(tx_hash),
                execution_index: 1,
                notification_index: 0,
                contract_hash: contract,
                event_name: "Transfer".to_string(),
                trigger: "Application".to_string(),
                state_item_count: 0,
                state: Vec::new(),
                accounts: vec![recipient, UInt160::zero(), signer, recipient, signer],
            }],
        )
        .expect("index recovered notification");

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].accounts, vec![recipient, signer]);
    assert_eq!(indexer.notifications_for_account(&signer, 0, 10), records);
    assert_eq!(
        indexer.notifications_for_account(&recipient, 0, 10),
        records
    );
    assert!(
        indexer
            .notifications_for_account(&UInt160::zero(), 0, 10)
            .is_empty()
    );
}

#[test]
fn indexes_recovered_notification_records_prefers_state_accounts() {
    let signer = account(1);
    let recipient = account(2);
    let foreign = account(3);
    let contract = account(9);
    let tx = transaction(67, &[signer]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let block = block(17, 17, vec![tx]);
    let block_hash = block.try_hash().expect("block hash");
    let state = transfer_state(Some(signer), Some(recipient), 13);

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_notification_records(
            &block,
            vec![NotificationIndexRecord {
                block_hash,
                block_height: block.index(),
                tx_hash: Some(tx_hash),
                execution_index: 1,
                notification_index: 0,
                contract_hash: contract,
                event_name: "Transfer".to_string(),
                trigger: "Application".to_string(),
                state_item_count: 99,
                state: state_json(&state),
                accounts: vec![foreign],
            }],
        )
        .expect("index recovered notification");

    let records = indexer.notifications_for_transaction(&tx_hash, 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state_item_count, 3);
    assert_eq!(records[0].accounts, vec![signer, recipient]);
    assert_eq!(indexer.notifications_for_account(&signer, 0, 10), records);
    assert_eq!(
        indexer.notifications_for_account(&recipient, 0, 10),
        records
    );
    assert!(
        indexer
            .notifications_for_account(&foreign, 0, 10)
            .is_empty()
    );
}

#[test]
fn indexes_contract_transactions_from_notifications() {
    let first = account(1);
    let second = account(2);
    let contract = account(9);
    let other_contract = account(8);
    let tx1 = transaction(72, &[first]);
    let tx1_hash = tx1.try_hash().expect("tx1 hash");
    let tx2 = transaction(73, &[second]);
    let tx2_hash = tx2.try_hash().expect("tx2 hash");
    let block = block(14, 14, vec![tx1.clone(), tx2.clone()]);

    let mut first_execution = ApplicationExecuted::new(
        Some(tx1),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    first_execution
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Transfer".to_string(),
            transfer_state(Some(first), Some(second), 1),
        ));
    first_execution
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            "Approval".to_string(),
            Vec::new(),
        ));
    first_execution
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            other_contract,
            "Transfer".to_string(),
            transfer_state(Some(first), Some(second), 2),
        ));

    let second_execution = execution_with_state(
        Some(tx2),
        contract,
        "Transfer",
        transfer_state(Some(second), Some(first), 3),
    );

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_application_executions(&block, &[first_execution, second_execution])
        .expect("index contract activity");

    let contract_transactions = indexer.transactions_for_contract(&contract, None, 0, 10);
    assert_eq!(contract_transactions.len(), 2);
    assert_eq!(contract_transactions[0].hash, tx1_hash);
    assert_eq!(contract_transactions[1].hash, tx2_hash);

    let approval_transactions =
        indexer.transactions_for_contract(&contract, Some("Approval"), 0, 10);
    assert_eq!(approval_transactions.len(), 1);
    assert_eq!(approval_transactions[0].hash, tx1_hash);

    let paged_transactions = indexer.transactions_for_contract(&contract, None, 1, 1);
    assert_eq!(paged_transactions.len(), 1);
    assert_eq!(paged_transactions[0].hash, tx2_hash);

    let other_transactions = indexer.transactions_for_contract(&other_contract, None, 0, 10);
    assert_eq!(other_transactions.len(), 1);
    assert_eq!(other_transactions[0].hash, tx1_hash);
}

#[test]
fn rejects_execution_transaction_outside_indexed_block() {
    let signer = account(1);
    let contract = account(9);
    let block_tx = transaction(63, &[signer]);
    let foreign_tx = transaction(64, &[signer]);
    let foreign_hash = foreign_tx.try_hash().expect("foreign tx hash");
    let block = block(13, 13, vec![block_tx]);

    let mut indexer = Indexer::new();
    let err = indexer
        .index_block_with_application_executions(
            &block,
            &[execution(Some(foreign_tx), contract, "Transfer")],
        )
        .expect_err("foreign execution transaction is rejected");

    assert!(matches!(
        err,
        IndexerError::ExecutionTransactionNotInBlock {
            hash,
            execution_index: 0
        } if hash == foreign_hash
    ));
    assert_eq!(indexer.status().indexed_blocks, 0);
    assert!(
        indexer
            .notifications_for_contract(&contract, None, 0, 10)
            .is_empty()
    );
}

#[test]
fn replacing_height_removes_stale_notifications() {
    let old_contract = account(3);
    let new_contract = account(4);
    let old_account = account(1);
    let new_account = account(2);
    let old_tx = transaction(61, &[old_account]);
    let old_block = block(11, 11, vec![old_tx.clone()]);
    let new_tx = transaction(62, &[new_account]);
    let new_block = block(11, 12, vec![new_tx.clone()]);

    let mut indexer = Indexer::new();
    indexer
        .index_block_with_application_executions(
            &old_block,
            &[execution_with_state(
                Some(old_tx),
                old_contract,
                "Transfer",
                transfer_state(Some(old_account), None, 5),
            )],
        )
        .expect("old block");
    indexer
        .index_block_with_application_executions(
            &new_block,
            &[execution_with_state(
                Some(new_tx),
                new_contract,
                "Transfer",
                transfer_state(None, Some(new_account), 7),
            )],
        )
        .expect("new block");

    assert!(
        indexer
            .notifications_for_contract(&old_contract, None, 0, 10)
            .is_empty()
    );
    assert!(
        indexer
            .notifications_for_account(&old_account, 0, 10)
            .is_empty()
    );
    let records = indexer.notifications_for_contract(&new_contract, Some("Transfer"), 0, 10);
    assert_eq!(records.len(), 1);
    assert_eq!(
        indexer.notifications_for_account(&new_account, 0, 10),
        records
    );
    assert_eq!(indexer.status().indexed_notifications, 1);
    assert_eq!(indexer.status().indexed_notification_accounts, 1);
}
