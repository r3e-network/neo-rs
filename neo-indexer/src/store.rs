//! Service-store schema and query helpers for the Neo indexer.

use std::sync::Arc;

use neo_storage::persistence::{SeekDirection, Store, StoreSnapshot};

use crate::error::{IndexerError, IndexerResult};
use crate::indexer::Indexer;
use crate::model::IndexerSnapshot;

mod keys;
mod records;
mod status;

pub(crate) use keys::{
    BLOCK_BY_HEIGHT_PREFIX, LEGACY_STORE_SNAPSHOT_KEY, NOTIFICATION_BY_CHAIN_PREFIX, STORE_PREFIX,
    STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_KEY, TRANSACTION_BY_CHAIN_PREFIX,
    account_transaction_prefix, block_by_hash_key, block_by_height_key,
    notification_by_account_prefix, notification_by_block_prefix, notification_by_contract_prefix,
    notification_by_transaction_prefix, transaction_by_block_prefix, transaction_by_hash_key,
};
pub(crate) use records::{
    get_record, read_record_page, read_record_page_filtered, read_record_prefix_filtered,
};
pub(crate) use status::status;

#[cfg(test)]
pub(crate) use keys::{
    ACCOUNT_TRANSACTION_PREFIX, BLOCK_BY_HASH_PREFIX, NOTIFICATION_BY_ACCOUNT_PREFIX,
    NOTIFICATION_BY_BLOCK_PREFIX, NOTIFICATION_BY_CONTRACT_PREFIX,
    NOTIFICATION_BY_TRANSACTION_PREFIX, TRANSACTION_BY_HASH_PREFIX, account_transaction_key,
};

pub(crate) fn read_indexer(store: &Arc<dyn Store>) -> IndexerResult<Indexer> {
    let snapshot = store.snapshot();
    if has_v3_records(snapshot.as_ref()) {
        return read_records(snapshot.as_ref());
    }

    let Some(raw) = snapshot.try_get(&LEGACY_STORE_SNAPSHOT_KEY.to_vec()) else {
        return Ok(Indexer::new());
    };
    let indexer_snapshot = serde_json::from_slice::<IndexerSnapshot>(&raw)
        .map_err(|source| IndexerError::StoreSnapshotDecode { source })?;
    let indexer = Indexer::from_snapshot(indexer_snapshot.clone())?;
    write_indexer(store, &indexer_snapshot)?;
    Ok(indexer)
}

pub(crate) fn write_indexer(
    store: &Arc<dyn Store>,
    indexer_snapshot: &IndexerSnapshot,
) -> IndexerResult<()> {
    let records = records::encode_records(indexer_snapshot)?;
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).ok_or(IndexerError::StoreSnapshotShared)?;
    clear_indexer_store(snapshot)?;
    records::put_records(snapshot, records)?;
    snapshot
        .try_commit()
        .map_err(|source| IndexerError::StoreRecordWrite { source })
}

pub(crate) fn write_indexer_delta(
    store: &Arc<dyn Store>,
    previous: &IndexerSnapshot,
    current: &IndexerSnapshot,
) -> IndexerResult<()> {
    let previous_records = records::encode_records(previous)?;
    let current_records = records::encode_records(current)?;
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).ok_or(IndexerError::StoreSnapshotShared)?;
    snapshot
        .delete(LEGACY_STORE_SNAPSHOT_KEY.to_vec())
        .map_err(|source| IndexerError::StoreRecordWrite { source })?;

    for key in previous_records.keys() {
        if !current_records.contains_key(key) {
            snapshot
                .delete(key.clone())
                .map_err(|source| IndexerError::StoreRecordWrite { source })?;
        }
    }

    for (key, value) in current_records {
        if key.as_slice() == STORE_SCHEMA_VERSION_KEY || previous_records.get(&key) != Some(&value)
        {
            snapshot
                .put(key, value)
                .map_err(|source| IndexerError::StoreRecordWrite { source })?;
        }
    }

    snapshot
        .try_commit()
        .map_err(|source| IndexerError::StoreRecordWrite { source })
}

fn has_v3_records(snapshot: &dyn StoreSnapshot) -> bool {
    if snapshot
        .try_get(&STORE_SCHEMA_VERSION_KEY.to_vec())
        .is_some()
    {
        return true;
    }
    let prefix = STORE_PREFIX.to_vec();
    snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .next()
        .is_some()
}

fn read_records(snapshot: &dyn StoreSnapshot) -> IndexerResult<Indexer> {
    let version = snapshot
        .try_get(&STORE_SCHEMA_VERSION_KEY.to_vec())
        .unwrap_or_default();
    if version.as_slice() != STORE_SCHEMA_VERSION {
        return Err(IndexerError::UnsupportedStoreSchemaVersion {
            version: String::from_utf8_lossy(&version).into_owned(),
        });
    }

    let blocks = records::read_record_prefix(snapshot, BLOCK_BY_HEIGHT_PREFIX)?;
    let transactions = records::read_record_prefix(snapshot, TRANSACTION_BY_CHAIN_PREFIX)?;
    let notifications = records::read_record_prefix(snapshot, NOTIFICATION_BY_CHAIN_PREFIX)?;
    Indexer::from_snapshot(IndexerSnapshot::with_notifications(
        blocks,
        transactions,
        notifications,
    ))
}

fn clear_indexer_store(snapshot: &mut dyn StoreSnapshot) -> IndexerResult<()> {
    let prefix = STORE_PREFIX.to_vec();
    let keys = snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .map(|(key, _)| key)
        .chain(std::iter::once(LEGACY_STORE_SNAPSHOT_KEY.to_vec()))
        .collect::<Vec<_>>();
    for key in keys {
        snapshot
            .delete(key)
            .map_err(|source| IndexerError::StoreRecordWrite { source })?;
    }
    Ok(())
}
