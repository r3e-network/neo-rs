use neo_blockchain::BlockchainHandle;
use neo_indexer::{IndexerService, IndexerStatus, NotificationIndexRecord};
use neo_payloads::Block;
use neo_primitives::UInt256;
use tracing::{info, warn};

pub(super) async fn backfill_start_height(
    blockchain: &BlockchainHandle,
    indexer: &IndexerService,
    chain_height: u32,
) -> Option<u32> {
    let status = match indexer.try_status() {
        Ok(status) => status,
        Err(err) => {
            warn!(
                target: "neo::indexer",
                chain_height,
                error = %err,
                "failed to read indexer status for resumable backfill; scanning from genesis"
            );
            return Some(0);
        }
    };
    let canonical_indexed_hash =
        canonical_hash_for_indexed_tip(blockchain, chain_height, status).await;
    let start_height =
        backfill_start_height_from_status(chain_height, status, canonical_indexed_hash);
    if start_height == Some(0) && status.indexed_height.is_some() {
        info!(
            target: "neo::indexer",
            chain_height,
            indexed_height = ?status.indexed_height,
            indexed_blocks = status.indexed_blocks,
            "indexer cannot safely resume backfill; scanning from genesis"
        );
    }
    start_height
}

async fn canonical_hash_for_indexed_tip(
    blockchain: &BlockchainHandle,
    chain_height: u32,
    status: IndexerStatus,
) -> Option<UInt256> {
    let indexed_height = status.indexed_height?;
    if indexed_height > chain_height {
        return None;
    }
    if status.indexed_hash.is_none() || !indexed_block_count_is_contiguous(status) {
        return None;
    }

    match blockchain.get_block_by_height(indexed_height).await {
        Ok(Some(block)) => match block.try_hash() {
            Ok(hash) => Some(hash),
            Err(err) => {
                warn!(
                    target: "neo::indexer",
                    indexed_height,
                    error = %err,
                    "failed to hash canonical indexed tip for resumable backfill"
                );
                None
            }
        },
        Ok(None) => {
            warn!(
                target: "neo::indexer",
                indexed_height,
                "indexed tip was not found in canonical chain; scanning from genesis"
            );
            None
        }
        Err(err) => {
            warn!(
                target: "neo::indexer",
                indexed_height,
                error = %err,
                "failed to fetch indexed tip for resumable backfill; scanning from genesis"
            );
            None
        }
    }
}

pub(super) fn backfill_start_height_from_status(
    chain_height: u32,
    status: IndexerStatus,
    canonical_indexed_hash: Option<UInt256>,
) -> Option<u32> {
    let Some(indexed_height) = status.indexed_height else {
        return Some(0);
    };
    if indexed_height > chain_height {
        return Some(0);
    }
    if !indexed_block_count_is_contiguous(status) {
        return Some(0);
    }
    if status.indexed_hash.is_none() || status.indexed_hash != canonical_indexed_hash {
        return Some(0);
    }
    indexed_height.checked_add(1)
}

fn indexed_block_count_is_contiguous(status: IndexerStatus) -> bool {
    status.indexed_height.and_then(|height| {
        usize::try_from(height)
            .ok()
            .and_then(|height| height.checked_add(1))
    }) == Some(status.indexed_blocks)
}

pub(super) fn prune_indexer_to_canonical_height(indexer: &IndexerService, height: u32) {
    match indexer.revert_to_height(height) {
        Ok(removed) if !removed.is_empty() => {
            info!(
                target: "neo::indexer",
                height,
                removed_blocks = removed.len(),
                "pruned indexer records above canonical chain height"
            );
        }
        Ok(_) => {}
        Err(err) => {
            warn!(
                target: "neo::indexer",
                height,
                error = %err,
                "failed to prune indexer records above canonical chain height"
            );
        }
    }
}

pub(super) fn index_block_with_available_notifications(
    indexer: &IndexerService,
    block: &Block,
    log_notifications: Vec<NotificationIndexRecord>,
) -> neo_indexer::IndexerResult<neo_indexer::BlockIndexRecord> {
    if log_notifications.is_empty() {
        indexer.index_block(block)
    } else {
        indexer.index_block_with_notification_records(block, log_notifications)
    }
}

pub(super) fn block_is_already_indexed(
    indexer: &IndexerService,
    block: &Block,
    block_height: u32,
) -> bool {
    let Some(record) = indexer.block_by_height(block_height) else {
        return false;
    };
    match block.try_hash() {
        Ok(hash) => record.hash == hash,
        Err(err) => {
            warn!(
                target: "neo::indexer",
                block_height,
                error = %err,
                "failed to hash block during indexer backfill skip check"
            );
            false
        }
    }
}

pub(super) fn should_index_block(
    indexer: &IndexerService,
    block: &Block,
    block_height: u32,
    log_notifications: &[NotificationIndexRecord],
) -> bool {
    !block_is_already_indexed(indexer, block, block_height)
        || should_enrich_notifications(indexer, block, log_notifications)
}

pub(super) fn should_enrich_notifications(
    indexer: &IndexerService,
    block: &Block,
    log_notifications: &[NotificationIndexRecord],
) -> bool {
    if log_notifications.is_empty() {
        return false;
    }
    let block_hash = match block.try_hash() {
        Ok(hash) => hash,
        Err(err) => {
            warn!(
                target: "neo::indexer",
                block_height = block.index(),
                error = %err,
                "failed to hash block during indexer notification enrichment check"
            );
            return false;
        }
    };
    let indexed_notifications = indexer.notifications_for_block(&block_hash, 0, usize::MAX);
    indexed_notifications.len() < log_notifications.len()
}
