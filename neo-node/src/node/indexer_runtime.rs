//! Background runtime for the daemon-owned indexer service.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, RuntimeEvent};
use neo_indexer::IndexerService;
#[cfg(test)]
use neo_indexer::{IndexerStatus, NotificationIndexRecord};
#[cfg(test)]
use neo_payloads::Block;
use neo_primitives::UInt256;
use neo_rpc::application_logs::ApplicationLogsService;
use neo_storage::persistence::Store;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, warn};

mod application_logs;
mod backfill;
use application_logs::recover_application_log_notifications;
#[cfg(test)]
use application_logs::{
    ApplicationLogRecoveryError, application_log_notification_records,
    parse_application_log_executions,
};
use backfill::{
    backfill_start_height, index_block_with_available_notifications,
    prune_indexer_to_canonical_height, should_index_block,
};
#[cfg(test)]
use backfill::{
    backfill_start_height_from_status, block_is_already_indexed, should_enrich_notifications,
};

const INDEXER_RUNTIME_ACTIVATION_WINDOW: u64 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IndexerRuntimeStartMode {
    StartNow,
    Deferred,
}

pub(crate) fn indexer_runtime_start_mode(
    local_height: u64,
    peer_tip: u64,
) -> IndexerRuntimeStartMode {
    if local_height > 0
        && (peer_tip == 0
            || local_height.saturating_add(INDEXER_RUNTIME_ACTIVATION_WINDOW) >= peer_tip)
    {
        IndexerRuntimeStartMode::StartNow
    } else {
        IndexerRuntimeStartMode::Deferred
    }
}

fn indexer_runtime_activation_reached(
    start_mode: IndexerRuntimeStartMode,
    local_height: u64,
    peer_tip: u64,
) -> bool {
    if start_mode == IndexerRuntimeStartMode::StartNow {
        return true;
    }
    peer_tip > 0 && local_height.saturating_add(INDEXER_RUNTIME_ACTIVATION_WINDOW) >= peer_tip
}

fn indexer_should_backfill_on_activation(
    backfill_on_startup: bool,
    start_mode: IndexerRuntimeStartMode,
) -> bool {
    backfill_on_startup || start_mode == IndexerRuntimeStartMode::Deferred
}

pub(crate) async fn run_live_indexer_when_ready<S>(
    blockchain: BlockchainHandle,
    indexer: Arc<IndexerService>,
    application_logs: Option<Arc<ApplicationLogsService<S>>>,
    backfill_on_startup: bool,
    startup_height: u64,
) where
    S: Store + 'static,
{
    let start_mode =
        indexer_runtime_start_mode(startup_height, neo_runtime::sync_metrics::peer_live_tip());
    if !wait_until_indexer_runtime_ready(&blockchain, startup_height, start_mode).await {
        return;
    }
    let backfill_on_startup =
        indexer_should_backfill_on_activation(backfill_on_startup, start_mode);

    run_live_indexer(blockchain, indexer, application_logs, backfill_on_startup).await;
}

async fn wait_until_indexer_runtime_ready(
    blockchain: &BlockchainHandle,
    startup_height: u64,
    start_mode: IndexerRuntimeStartMode,
) -> bool {
    let mut local_height = startup_height;
    let initial_peer_tip = neo_runtime::sync_metrics::peer_live_tip();
    if start_mode == IndexerRuntimeStartMode::StartNow {
        info!(
            target: "neo::indexer",
            local_height,
            peer_tip = initial_peer_tip,
            "starting indexer runtime"
        );
        return true;
    }

    info!(
        target: "neo::indexer",
        local_height,
        peer_tip = initial_peer_tip,
        activation_window = INDEXER_RUNTIME_ACTIVATION_WINDOW,
        "indexer runtime deferred during catch-up; supervisor will start it near peer tip"
    );

    let mut events = blockchain.subscribe();
    loop {
        match events.recv().await {
            Ok(RuntimeEvent::Imported { height, .. })
            | Ok(RuntimeEvent::TipChanged { height, .. }) => {
                local_height = u64::from(height);
            }
            Ok(RuntimeEvent::Reverted { height, .. }) => {
                local_height = u64::from(height.saturating_sub(1));
            }
            Ok(RuntimeEvent::RelayResult { .. }) => {
                // Relay outcomes do not advance or rewind the canonical chain;
                // keep waiting for import/tip events that affect activation.
            }
            Ok(RuntimeEvent::Shutdown) => {
                info!(
                    target: "neo::indexer",
                    "indexer runtime supervisor stopped before activation"
                );
                return false;
            }
            Err(RecvError::Lagged(skipped)) => match blockchain.get_height().await {
                Ok(height) => {
                    local_height = u64::from(height);
                    debug!(
                        target: "neo::indexer",
                        skipped,
                        local_height,
                        "indexer activation supervisor caught up after lagged events"
                    );
                }
                Err(err) => {
                    warn!(
                        target: "neo::indexer",
                        skipped,
                        error = %err,
                        "indexer activation supervisor failed to read chain height"
                    );
                }
            },
            Err(RecvError::Closed) => return false,
        }

        let peer_tip = neo_runtime::sync_metrics::peer_live_tip();
        if indexer_runtime_activation_reached(start_mode, local_height, peer_tip) {
            info!(
                target: "neo::indexer",
                local_height,
                peer_tip,
                "indexer runtime activation window reached"
            );
            return true;
        }
    }
}

/// Background task that follows canonical-chain events.
pub(crate) async fn run_live_indexer<S>(
    blockchain: BlockchainHandle,
    indexer: Arc<IndexerService>,
    application_logs: Option<Arc<ApplicationLogsService<S>>>,
    backfill_on_startup: bool,
) where
    S: Store + 'static,
{
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
            Ok(RuntimeEvent::RelayResult { .. }) => {
                // The indexer follows canonical-chain state only. RelayResult
                // is consumed by RPC/subscription surfaces, not block indexing.
            }
            Ok(RuntimeEvent::Shutdown) => break,
            Err(RecvError::Lagged(skipped)) => {
                // During catch-up, the indexer broadcast channel overflows because
                // blocks are persisted faster than the indexer can index them.
                // A full backfill (scan-from-genesis) here is O(n) expensive and
                // dominates sync time (measured: 30 blocks/min WITH backfill vs
                // 200+ WITHOUT). Instead, skip the missed events during catch-up
                // — the indexer will catch up naturally as sync slows near the
                // live tip, where the event rate drops to ~1 block/15s.
                let live_tip = neo_runtime::sync_metrics::peer_live_tip();
                let our_height = match blockchain.get_height().await {
                    Ok(h) => h as u64,
                    Err(_) => 0,
                };
                let near_tip = live_tip > 0 && our_height + 1000 >= live_tip;
                if near_tip {
                    warn!(
                        target: "neo::indexer",
                        skipped,
                        "indexer lagged behind block events near tip; rebuilding from canonical chain"
                    );
                    backfill_indexer(&blockchain, &indexer, application_logs.as_deref()).await;
                } else {
                    debug!(
                        target: "neo::indexer",
                        skipped,
                        our_height,
                        live_tip,
                        "indexer lagged during catch-up; skipping backfill (will catch up near tip)"
                    );
                }
            }
            Err(RecvError::Closed) => break,
        }
    }
}

async fn backfill_indexer<S>(
    blockchain: &BlockchainHandle,
    indexer: &IndexerService,
    application_logs: Option<&ApplicationLogsService<S>>,
) where
    S: Store,
{
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

async fn index_block_by_hash<S>(
    blockchain: &BlockchainHandle,
    indexer: &IndexerService,
    application_logs: Option<&ApplicationLogsService<S>>,
    hash: UInt256,
    height: u32,
) where
    S: Store,
{
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

#[cfg(test)]
#[path = "../tests/node/indexer_runtime.rs"]
mod tests;
