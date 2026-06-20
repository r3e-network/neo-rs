//! Store-backed status counters for the Neo indexer service.

use std::collections::HashSet;

use neo_primitives::UInt160;
use neo_storage::persistence::{SeekDirection, StoreSnapshot};

use super::keys::{
    ACCOUNT_TRANSACTION_PREFIX, BLOCK_BY_HEIGHT_PREFIX, NOTIFICATION_BY_ACCOUNT_PREFIX,
    NOTIFICATION_BY_CHAIN_PREFIX, TRANSACTION_BY_CHAIN_PREFIX,
};
use super::records;
use crate::error::IndexerResult;
use crate::model::{BlockIndexRecord, IndexerStatus};

pub(crate) fn status(snapshot: &dyn StoreSnapshot) -> IndexerResult<IndexerStatus> {
    let indexed_tip = latest_block(snapshot)?;
    Ok(IndexerStatus {
        indexed_height: indexed_tip.as_ref().map(|block| block.height),
        indexed_hash: indexed_tip.map(|block| block.hash),
        indexed_blocks: count_prefix_rows(snapshot, BLOCK_BY_HEIGHT_PREFIX),
        indexed_transactions: count_prefix_rows(snapshot, TRANSACTION_BY_CHAIN_PREFIX),
        indexed_accounts: count_unique_key_segments(
            snapshot,
            ACCOUNT_TRANSACTION_PREFIX,
            ACCOUNT_TRANSACTION_PREFIX.len(),
            UInt160::LENGTH,
        ),
        indexed_notifications: count_prefix_rows(snapshot, NOTIFICATION_BY_CHAIN_PREFIX),
        indexed_notification_accounts: count_unique_key_segments(
            snapshot,
            NOTIFICATION_BY_ACCOUNT_PREFIX,
            NOTIFICATION_BY_ACCOUNT_PREFIX.len(),
            UInt160::LENGTH,
        ),
    })
}

fn latest_block(snapshot: &dyn StoreSnapshot) -> IndexerResult<Option<BlockIndexRecord>> {
    let prefix = BLOCK_BY_HEIGHT_PREFIX.to_vec();
    snapshot
        .find(Some(&prefix), SeekDirection::Backward)
        .next()
        .map(|(key, value)| records::decode_record(key, value))
        .transpose()
}

fn count_prefix_rows(snapshot: &dyn StoreSnapshot, prefix: &[u8]) -> usize {
    let prefix = prefix.to_vec();
    snapshot.find(Some(&prefix), SeekDirection::Forward).count()
}

fn count_unique_key_segments(
    snapshot: &dyn StoreSnapshot,
    prefix: &[u8],
    offset: usize,
    length: usize,
) -> usize {
    let prefix = prefix.to_vec();
    snapshot
        .find(Some(&prefix), SeekDirection::Forward)
        .filter_map(|(key, _)| key.get(offset..offset + length).map(<[u8]>::to_vec))
        .collect::<HashSet<_>>()
        .len()
}
