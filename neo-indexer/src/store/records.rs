//! JSON record encoding and paged reads for the Neo indexer service store.

use std::collections::BTreeMap;

use neo_storage::persistence::{SeekDirection, StoreSnapshot};
use serde::{Serialize, de::DeserializeOwned};

use super::keys::{
    STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_KEY, account_transaction_key, block_by_hash_key,
    block_by_height_key, notification_by_account_key, notification_by_block_key,
    notification_by_chain_key, notification_by_contract_key, notification_by_transaction_key,
    transaction_by_chain_key, transaction_by_hash_key,
};
use crate::error::{IndexerError, IndexerResult};
use crate::model::{AccountTransactionRecord, IndexerSnapshot};

pub(crate) fn get_record<T>(snapshot: &dyn StoreSnapshot, key: Vec<u8>) -> IndexerResult<Option<T>>
where
    T: DeserializeOwned,
{
    snapshot
        .try_get(&key)
        .map(|value| decode_record(key, value))
        .transpose()
}

pub(crate) fn read_record_page<T>(
    snapshot: &dyn StoreSnapshot,
    prefix: &[u8],
    skip: usize,
    limit: usize,
) -> IndexerResult<Vec<T>>
where
    T: DeserializeOwned,
{
    read_record_page_filtered(snapshot, prefix, |_| true, skip, limit)
}

pub(crate) fn read_record_page_filtered<T>(
    snapshot: &dyn StoreSnapshot,
    prefix: &[u8],
    mut filter: impl FnMut(&T) -> bool,
    skip: usize,
    limit: usize,
) -> IndexerResult<Vec<T>>
where
    T: DeserializeOwned,
{
    if limit == 0 {
        return Ok(Vec::new());
    }

    let prefix = prefix.to_vec();
    let mut skipped = 0usize;
    let mut records = Vec::new();
    for (key, value) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
        let record = decode_record(key, value)?;
        if !filter(&record) {
            continue;
        }
        if skipped < skip {
            skipped += 1;
            continue;
        }
        records.push(record);
        if records.len() >= limit {
            break;
        }
    }
    Ok(records)
}

pub(crate) fn read_record_prefix_filtered<T>(
    snapshot: &dyn StoreSnapshot,
    prefix: &[u8],
    mut filter: impl FnMut(&T) -> bool,
) -> IndexerResult<Vec<T>>
where
    T: DeserializeOwned,
{
    let prefix = prefix.to_vec();
    let mut records = Vec::new();
    for (key, value) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
        let record = decode_record(key, value)?;
        if filter(&record) {
            records.push(record);
        }
    }
    Ok(records)
}

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

pub(super) fn read_record_prefix<T>(
    snapshot: &dyn StoreSnapshot,
    prefix: &[u8],
) -> IndexerResult<Vec<T>>
where
    T: DeserializeOwned,
{
    let prefix = prefix.to_vec();
    snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, value)| decode_record(key, value))
        .collect()
}

pub(super) fn put_records(
    snapshot: &mut dyn StoreSnapshot,
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
    T: Serialize,
{
    let bytes = serde_json::to_vec(value).map_err(|source| IndexerError::StoreRecordEncode {
        key: key.clone(),
        source,
    })?;
    records.insert(key, bytes);
    Ok(())
}

pub(super) fn decode_record<T>(key: Vec<u8>, value: Vec<u8>) -> IndexerResult<T>
where
    T: DeserializeOwned,
{
    serde_json::from_slice(&value).map_err(|source| IndexerError::StoreRecordDecode { key, source })
}
