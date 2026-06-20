//! Background runtime for the daemon-owned indexer service.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, RuntimeEvent};
use neo_indexer::{IndexerService, IndexerStatus, NotificationIndexRecord};
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_rpc::application_logs::ApplicationLogsService;
use tokio::sync::broadcast::error::RecvError;
use tracing::{error, info, warn};

mod application_logs;
use application_logs::recover_application_log_notifications;
#[cfg(test)]
use application_logs::{
    ApplicationLogRecoveryError, application_log_notification_records,
    parse_application_log_executions,
};

/// Background task that follows canonical-chain events.
pub(crate) async fn run_live_indexer(
    blockchain: BlockchainHandle,
    indexer: Arc<IndexerService>,
    application_logs: Option<Arc<ApplicationLogsService>>,
    backfill_on_startup: bool,
) {
    let mut events = blockchain.subscribe();
    if backfill_on_startup {
        backfill_indexer(&blockchain, &indexer, application_logs.as_deref()).await;
    }

    loop {
        match events.recv().await {
            Ok(RuntimeEvent::Imported { hash, height, .. }) => {
                index_block_by_hash(
                    &blockchain,
                    &indexer,
                    application_logs.as_deref(),
                    hash,
                    height,
                )
                .await;
            }
            Ok(RuntimeEvent::Reverted { hash, height }) => {
                if let Err(err) = indexer.remove_block_by_hash(&hash) {
                    warn!(
                        target: "neo::indexer",
                        height,
                        hash = %hash,
                        error = %err,
                        "failed to persist indexer block removal"
                    );
                }
                if let Err(err) = indexer.revert_to_height(height.saturating_sub(1)) {
                    warn!(
                        target: "neo::indexer",
                        height,
                        error = %err,
                        "failed to persist indexer rollback"
                    );
                }
            }
            Ok(RuntimeEvent::TipChanged { height, .. }) => {
                if let Err(err) = indexer.revert_to_height(height) {
                    warn!(
                        target: "neo::indexer",
                        height,
                        error = %err,
                        "failed to persist indexer tip-change rollback"
                    );
                }
                backfill_indexer(&blockchain, &indexer, application_logs.as_deref()).await;
            }
            Ok(RuntimeEvent::Shutdown) => break,
            Err(RecvError::Lagged(skipped)) => {
                warn!(
                    target: "neo::indexer",
                    skipped,
                    "indexer lagged behind block events; rebuilding from canonical chain"
                );
                backfill_indexer(&blockchain, &indexer, application_logs.as_deref()).await;
            }
            Err(RecvError::Closed) => break,
        }
    }
}

async fn backfill_indexer(
    blockchain: &BlockchainHandle,
    indexer: &IndexerService,
    application_logs: Option<&ApplicationLogsService>,
) {
    let height = match blockchain.get_height().await {
        Ok(height) => height,
        Err(err) => {
            warn!(target: "neo::indexer", error = %err, "failed to read blockchain height for indexer backfill");
            return;
        }
    };

    prune_indexer_to_canonical_height(indexer, height);
    let Some(start_height) = backfill_start_height(blockchain, indexer, height).await else {
        info!(
            target: "neo::indexer",
            height,
            "indexer backfill skipped; indexed tip is already at maximum height"
        );
        return;
    };

    info!(
        target: "neo::indexer",
        height,
        start_height,
        "starting indexer backfill"
    );
    for block_height in start_height..=height {
        match blockchain.get_block_by_height(block_height).await {
            Ok(Some(block)) => {
                let log_notifications =
                    recover_application_log_notifications(application_logs, &block, "backfill");
                if !should_index_block(indexer, &block, block_height, &log_notifications) {
                    continue;
                }
                let result =
                    index_block_with_available_notifications(indexer, &block, log_notifications);
                if let Err(err) = result {
                    error!(
                        target: "neo::indexer",
                        block_height,
                        error = %err,
                        "failed to index block during backfill"
                    );
                }
            }
            Ok(None) => {
                warn!(
                    target: "neo::indexer",
                    block_height,
                    "canonical block missing during indexer backfill"
                );
            }
            Err(err) => {
                warn!(
                    target: "neo::indexer",
                    block_height,
                    error = %err,
                    "failed to fetch block during indexer backfill"
                );
            }
        }

        if block_height % 256 == 0 {
            tokio::task::yield_now().await;
        }
    }
    info!(
        target: "neo::indexer",
        indexed_height = ?indexer.status().indexed_height,
        "indexer backfill finished"
    );
}

async fn backfill_start_height(
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

fn backfill_start_height_from_status(
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

fn prune_indexer_to_canonical_height(indexer: &IndexerService, height: u32) {
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

async fn index_block_by_hash(
    blockchain: &BlockchainHandle,
    indexer: &IndexerService,
    application_logs: Option<&ApplicationLogsService>,
    hash: UInt256,
    height: u32,
) {
    match blockchain.get_block(&hash).await {
        Ok(Some(block)) => {
            let log_notifications =
                recover_application_log_notifications(application_logs, &block, "imported block");
            if !should_index_block(indexer, &block, height, &log_notifications) {
                return;
            }
            let result =
                index_block_with_available_notifications(indexer, &block, log_notifications);
            if let Err(err) = result {
                error!(
                    target: "neo::indexer",
                    height,
                    hash = %hash,
                    error = %err,
                    "failed to index imported block"
                );
            }
        }
        Ok(None) => {
            warn!(
                target: "neo::indexer",
                height,
                hash = %hash,
                "imported block was not found for indexing"
            );
        }
        Err(err) => {
            warn!(
                target: "neo::indexer",
                height,
                hash = %hash,
                error = %err,
                "failed to fetch imported block for indexing"
            );
        }
    }
}

fn index_block_with_available_notifications(
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

fn block_is_already_indexed(indexer: &IndexerService, block: &Block, block_height: u32) -> bool {
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

fn should_index_block(
    indexer: &IndexerService,
    block: &Block,
    block_height: u32,
    log_notifications: &[NotificationIndexRecord],
) -> bool {
    !block_is_already_indexed(indexer, block, block_height)
        || should_enrich_notifications(indexer, block, log_notifications)
}

fn should_enrich_notifications(
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

#[cfg(test)]
mod tests;
