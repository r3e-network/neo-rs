//! Snapshot-to-record materialization and store writes.

use std::collections::BTreeMap;

use super::keys::{
    STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_KEY, account_transaction_key, block_by_hash_key,
    block_by_height_key, notification_by_account_key, notification_by_block_key,
    notification_by_chain_key, notification_by_contract_key, notification_by_transaction_key,
    transaction_by_chain_key, transaction_by_hash_key,
};
use super::record_codec::encode_record;
use crate::error::IndexerResult;
use crate::indexer::ProjectionChangeSet;
use crate::model::{AccountTransactionRecord, IndexerSnapshot};

pub(super) fn encode_records(
    indexer_snapshot: &IndexerSnapshot,
) -> IndexerResult<BTreeMap<Vec<u8>, Vec<u8>>> {
    let mut records = BTreeMap::new();
    records.insert(
        STORE_SCHEMA_VERSION_KEY.to_vec(),
        STORE_SCHEMA_VERSION.to_vec(),
    );

    for block in &indexer_snapshot.blocks {
        insert_block_records(&mut records, block)?;
    }
    for transaction in &indexer_snapshot.transactions {
        insert_transaction_records(&mut records, transaction)?;
    }
    for notification in &indexer_snapshot.notifications {
        insert_notification_records(&mut records, notification)?;
    }

    Ok(records)
}

pub(super) fn encode_change_set(
    change: &ProjectionChangeSet,
) -> IndexerResult<(BTreeMap<Vec<u8>, Vec<u8>>, BTreeMap<Vec<u8>, Vec<u8>>)> {
    let mut removed = BTreeMap::new();
    for bundle in &change.removed {
        insert_block_records(&mut removed, &bundle.block)?;
        for transaction in &bundle.transactions {
            insert_transaction_records(&mut removed, transaction)?;
        }
        for notification in &bundle.notifications {
            insert_notification_records(&mut removed, notification)?;
        }
    }

    let mut inserted = BTreeMap::new();
    inserted.insert(
        STORE_SCHEMA_VERSION_KEY.to_vec(),
        STORE_SCHEMA_VERSION.to_vec(),
    );
    for bundle in &change.inserted {
        insert_block_records(&mut inserted, &bundle.block)?;
        for transaction in &bundle.transactions {
            insert_transaction_records(&mut inserted, transaction)?;
        }
        for notification in &bundle.notifications {
            insert_notification_records(&mut inserted, notification)?;
        }
    }
    Ok((removed, inserted))
}

fn insert_block_records(
    records: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    block: &crate::model::BlockIndexRecord,
) -> IndexerResult<()> {
    insert_record(records, block_by_height_key(block.height), block)?;
    insert_record(records, block_by_hash_key(&block.hash), block)
}

fn insert_transaction_records(
    records: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    transaction: &crate::model::TransactionIndexRecord,
) -> IndexerResult<()> {
    insert_record(records, transaction_by_chain_key(transaction), transaction)?;
    insert_record(
        records,
        transaction_by_hash_key(&transaction.hash),
        transaction,
    )?;
    for account in &transaction.signers {
        insert_record(
            records,
            account_transaction_key(account, transaction),
            &AccountTransactionRecord {
                account: *account,
                tx_hash: transaction.hash,
                block_hash: transaction.block_hash,
                block_height: transaction.block_height,
                transaction_index: transaction.transaction_index,
            },
        )?;
    }
    Ok(())
}

fn insert_notification_records(
    records: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    notification: &crate::model::NotificationIndexRecord,
) -> IndexerResult<()> {
    insert_record(
        records,
        notification_by_chain_key(notification),
        notification,
    )?;
    insert_record(
        records,
        notification_by_block_key(notification),
        notification,
    )?;
    if let Some(tx_hash) = notification.tx_hash {
        insert_record(
            records,
            notification_by_transaction_key(&tx_hash, notification),
            notification,
        )?;
    }
    insert_record(
        records,
        notification_by_contract_key(notification),
        notification,
    )?;
    for account in &notification.accounts {
        insert_record(
            records,
            notification_by_account_key(account, notification),
            notification,
        )?;
    }
    Ok(())
}

fn insert_record<T>(
    records: &mut BTreeMap<Vec<u8>, Vec<u8>>,
    key: Vec<u8>,
    value: &T,
) -> IndexerResult<()>
where
    T: serde::Serialize,
{
    let (key, bytes) = encode_record(key, value)?;
    records.insert(key, bytes);
    Ok(())
}
