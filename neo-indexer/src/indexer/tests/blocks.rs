use super::*;

#[test]
fn indexes_block_and_transaction_positions() {
    let account = account(1);
    let tx0 = transaction(10, &[account]);
    let tx1 = transaction(11, &[account]);
    let tx1_hash = tx1.try_hash().expect("tx hash");
    let block = block(7, 7, vec![tx0, tx1]);

    let mut indexer = Indexer::new();
    let block_record = indexer.index_block(&block).expect("index block");

    assert_eq!(block_record.height, 7);
    assert_eq!(block_record.transaction_count, 2);
    assert_eq!(indexer.block_by_height(7), Some(block_record.clone()));
    let block_hash = block_record.hash;
    assert_eq!(indexer.status().indexed_hash, Some(block_hash));
    assert_eq!(indexer.block_by_hash(&block_hash), Some(block_record));
    let listed_blocks = indexer.blocks(0, 10);
    assert_eq!(listed_blocks.len(), 1);
    assert_eq!(listed_blocks[0].height, 7);

    let tx_record = indexer.transaction(&tx1_hash).expect("tx index");
    assert_eq!(tx_record.block_height, 7);
    assert_eq!(tx_record.transaction_index, 1);

    let block_transactions = indexer.transactions_for_block(&block_hash, 1, 1);
    assert_eq!(block_transactions, vec![tx_record]);
}

#[test]
fn indexes_signer_accounts() {
    let first = account(1);
    let second = account(2);
    let block = block(
        8,
        8,
        vec![transaction(20, &[first]), transaction(21, &[first, second])],
    );

    let mut indexer = Indexer::new();
    indexer.index_block(&block).expect("index block");

    let first_records = indexer.transactions_for_account(&first, 0, 10);
    assert_eq!(first_records.len(), 2);
    assert_eq!(first_records[0].transaction_index, 0);
    assert_eq!(first_records[1].transaction_index, 1);

    let second_records = indexer.transactions_for_account(&second, 0, 10);
    assert_eq!(second_records.len(), 1);
    assert_eq!(second_records[0].transaction_index, 1);
}

#[test]
fn pagination_applies_after_canonical_ordering() {
    let signer = account(1);
    let recipient = account(2);
    let contract = account(9);
    let mut indexer = Indexer::new();

    for (height, nonce) in [(3, 300), (1, 100), (2, 200)] {
        let tx = transaction(nonce, &[signer]);
        let block = block(height, u64::from(nonce), vec![tx.clone()]);
        indexer
            .index_block_with_application_executions(
                &block,
                &[execution_with_state(
                    Some(tx),
                    contract,
                    "Transfer",
                    transfer_state(Some(signer), Some(recipient), i64::from(height)),
                )],
            )
            .expect("index block with notification");
    }

    let account_transactions = indexer.transactions_for_account(&signer, 1, 1);
    assert_eq!(account_transactions.len(), 1);
    assert_eq!(account_transactions[0].block_height, 2);

    let account_notifications = indexer.notifications_for_account(&signer, 1, 1);
    assert_eq!(account_notifications.len(), 1);
    assert_eq!(account_notifications[0].block_height, 2);
    assert_eq!(account_notifications[0].state[2]["value"], "2");

    let contract_notifications =
        indexer.notifications_for_contract(&contract, Some("Transfer"), 2, 1);
    assert_eq!(contract_notifications.len(), 1);
    assert_eq!(contract_notifications[0].block_height, 3);
}

#[test]
fn replacing_height_removes_stale_indexes() {
    let first = account(1);
    let second = account(2);
    let old_tx = transaction(30, &[first]);
    let old_hash = old_tx.try_hash().expect("old tx hash");
    let old_block = block(9, 9, vec![old_tx]);
    let new_tx = transaction(31, &[second]);
    let new_hash = new_tx.try_hash().expect("new tx hash");
    let new_block = block(9, 10, vec![new_tx]);

    let mut indexer = Indexer::new();
    indexer.index_block(&old_block).expect("old block");
    indexer.index_block(&new_block).expect("new block");

    assert!(indexer.transaction(&old_hash).is_none());
    assert!(indexer.transactions_for_account(&first, 0, 10).is_empty());
    assert!(indexer.transaction(&new_hash).is_some());
    assert_eq!(indexer.transactions_for_account(&second, 0, 10).len(), 1);
    assert_eq!(
        indexer.block_by_height(9).expect("height").hash,
        new_block.try_hash().expect("new hash")
    );
}

#[test]
fn revert_to_height_removes_later_indexes() {
    let first = account(1);
    let mut indexer = Indexer::new();
    let block1 = block(1, 1, vec![transaction(40, &[first])]);
    let block2 = block(2, 2, vec![transaction(41, &[first])]);
    let block3 = block(3, 3, vec![transaction(42, &[first])]);
    indexer.index_block(&block1).expect("block1");
    indexer.index_block(&block2).expect("block2");
    indexer.index_block(&block3).expect("block3");

    let removed = indexer.revert_to_height(1);

    assert_eq!(removed.len(), 2);
    assert!(indexer.block_by_height(2).is_none());
    assert!(indexer.block_by_height(3).is_none());
    assert_eq!(indexer.status().indexed_height, Some(1));
    assert_eq!(
        indexer.status().indexed_hash,
        Some(block1.try_hash().expect("block1 hash"))
    );
    assert_eq!(indexer.transactions_for_account(&first, 0, 10).len(), 1);
}

#[test]
fn revert_to_max_height_is_noop() {
    let first = account(1);
    let mut indexer = Indexer::new();
    let block = block(u32::MAX, 1, vec![transaction(43, &[first])]);
    indexer.index_block(&block).expect("block");

    let removed = indexer.revert_to_height(u32::MAX);

    assert!(removed.is_empty());
    assert_eq!(indexer.status().indexed_height, Some(u32::MAX));
    assert_eq!(
        indexer.status().indexed_hash,
        Some(block.try_hash().expect("block hash"))
    );
    assert_eq!(indexer.transactions_for_account(&first, 0, 10).len(), 1);
}
