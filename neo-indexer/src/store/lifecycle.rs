//! Store-backed indexer open and atomic delta-write helpers.

use std::sync::Arc;

use neo_storage::persistence::{
    ReadOnlyStoreGeneric, SeekDirection, Store, StoreSnapshot, WriteStore,
};

use super::keys::{
    BLOCK_BY_HEIGHT_PREFIX, LEGACY_STORE_SNAPSHOT_KEY, NOTIFICATION_BY_CHAIN_PREFIX, STORE_PREFIX,
    STORE_SCHEMA_VERSION, STORE_SCHEMA_VERSION_KEY, TRANSACTION_BY_CHAIN_PREFIX,
};
use super::record_read::read_record_prefix;
use super::record_write::{encode_change_set, encode_records};
use crate::error::{IndexerError, IndexerResult};
use crate::indexer::{Indexer, ProjectionChangeSet};
use crate::model::IndexerSnapshot;

pub(crate) fn read_indexer<S>(store: &Arc<S>) -> IndexerResult<Indexer>
where
    S: Store + 'static,
{
    let snapshot = store.snapshot();
    if has_v3_records(snapshot.as_ref()) {
        return read_records(snapshot.as_ref());
    }
    if snapshot
        .try_get(&LEGACY_STORE_SNAPSHOT_KEY.to_vec())
        .is_some()
    {
        return Err(IndexerError::LegacyStoreSnapshotUnsupported);
    }
    Ok(Indexer::new())
}

pub(crate) fn write_indexer_delta<S>(
    store: &Arc<S>,
    previous: &IndexerSnapshot,
    current: &IndexerSnapshot,
) -> IndexerResult<()>
where
    S: Store + 'static,
{
    let previous_records = encode_records(previous)?;
    let current_records = encode_records(current)?;
    write_record_delta(store, previous_records, current_records)
}

pub(crate) fn write_indexer_change_set<S>(
    store: &Arc<S>,
    change: &ProjectionChangeSet,
) -> IndexerResult<()>
where
    S: Store + 'static,
{
    let (removed_records, inserted_records) = encode_change_set(change)?;
    write_record_delta(store, removed_records, inserted_records)
}

fn write_record_delta<S>(
    store: &Arc<S>,
    removed_records: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
    inserted_records: std::collections::BTreeMap<Vec<u8>, Vec<u8>>,
) -> IndexerResult<()>
where
    S: Store + 'static,
{
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).ok_or(IndexerError::StoreSnapshotShared)?;

    for key in removed_records.keys() {
        if !inserted_records.contains_key(key) {
            snapshot
                .delete(key.clone())
                .map_err(|source| IndexerError::StoreRecordWrite { source })?;
        }
    }

    for (key, value) in inserted_records {
        if key.as_slice() == STORE_SCHEMA_VERSION_KEY || removed_records.get(&key) != Some(&value) {
            snapshot
                .put(key, value)
                .map_err(|source| IndexerError::StoreRecordWrite { source })?;
        }
    }

    snapshot
        .try_commit()
        .map_err(|source| IndexerError::StoreRecordWrite { source })
}

fn has_v3_records(snapshot: &impl StoreSnapshot) -> bool {
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

fn read_records(snapshot: &impl StoreSnapshot) -> IndexerResult<Indexer> {
    let version = snapshot
        .try_get(&STORE_SCHEMA_VERSION_KEY.to_vec())
        .unwrap_or_default();
    if version.as_slice() != STORE_SCHEMA_VERSION {
        return Err(IndexerError::UnsupportedStoreSchemaVersion {
            version: String::from_utf8_lossy(&version).into_owned(),
        });
    }

    let blocks = read_record_prefix(snapshot, BLOCK_BY_HEIGHT_PREFIX)?;
    let transactions = read_record_prefix(snapshot, TRANSACTION_BY_CHAIN_PREFIX)?;
    let notifications = read_record_prefix(snapshot, NOTIFICATION_BY_CHAIN_PREFIX)?;
    Indexer::from_snapshot(IndexerSnapshot::with_notifications(
        blocks,
        transactions,
        notifications,
    ))
}
