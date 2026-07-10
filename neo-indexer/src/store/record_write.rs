//! Snapshot-to-record materialization and store writes.

use std::collections::BTreeMap;

use neo_storage::persistence::StoreSnapshot;

use super::keys::{
    STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_KEY, account_transaction_key, block_by_hash_key,
    block_by_height_key, notification_by_account_key, notification_by_block_key,
    notification_by_chain_key, notification_by_contract_key, notification_by_transaction_key,
    transaction_by_chain_key, transaction_by_hash_key,
};
use super::record_codec::encode_record;
use crate::error::{IndexerError, IndexerResult};
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
        insert_record(&mut records, block_by_height_key(block.height), block)?;
        insert_record(&mut records, block_by_hash_key(&block.hash), block)?;
    }
    for transaction in &indexer_snapshot.transactions {
        insert_record(
            &mut records,
            transaction_by_chain_key(transaction),
            transaction,
        )?;
        insert_record(
            &mut records,
            transaction_by_hash_key(&transaction.hash),
            transaction,
        )?;
        for account in &transaction.signers {
            insert_record(
                &mut records,
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
    }
    for notification in &indexer_snapshot.notifications {
        insert_record(
            &mut records,
            notification_by_chain_key(notification),
            notification,
        )?;
        insert_record(
            &mut records,
            notification_by_block_key(notification),
            notification,
        )?;
        if let Some(tx_hash) = notification.tx_hash {
            insert_record(
                &mut records,
                notification_by_transaction_key(&tx_hash, notification),
                notification,
            )?;
        }
        insert_record(
            &mut records,
            notification_by_contract_key(notification),
            notification,
        )?;
        for account in &notification.accounts {
            insert_record(
                &mut records,
                notification_by_account_key(account, notification),
                notification,
            )?;
        }
    }

    Ok(records)
}

pub(super) fn put_records(
    snapshot: &mut impl StoreSnapshot,
    records: BTreeMap<Vec<u8>, Vec<u8>>,
) -> IndexerResult<()> {
    for (key, value) in records {
        snapshot
            .put(key, value)
            .map_err(|source| IndexerError::StoreRecordWrite { source })?;
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
