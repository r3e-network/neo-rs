//! Paged and filtered reads for encoded indexer store records.

use neo_storage::persistence::{SeekDirection, StoreSnapshot};
use serde::de::DeserializeOwned;

use super::record_codec::decode_record;
use crate::error::IndexerResult;

pub(crate) fn get_record<T>(snapshot: &impl StoreSnapshot, key: Vec<u8>) -> IndexerResult<Option<T>>
where
    T: DeserializeOwned,
{
    snapshot
        .try_get(&key)
        .map(|value| decode_record(key, value))
        .transpose()
}

pub(crate) fn read_record_page<T>(
    snapshot: &impl StoreSnapshot,
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
    snapshot: &impl StoreSnapshot,
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
    snapshot: &impl StoreSnapshot,
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

pub(super) fn read_record_prefix<T>(
    snapshot: &impl StoreSnapshot,
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
