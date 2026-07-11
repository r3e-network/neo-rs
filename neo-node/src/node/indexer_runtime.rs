//! # neo-node::node::indexer_runtime
//!
//! Activation supervision and canonical event handling for the daemon-owned
//! read-side Index stage.
//!
//! ## Boundary
//!
//! This application module decides when the optional indexer follows committed
//! chain state. It does not validate blocks or own projection storage formats.
//!
//! ## Contents
//!
//! - `application_logs`: optional durable notification recovery adapter.
//! - `stage`: bounded, crash-resumable committed-chain projection follower.

use std::sync::Arc;

use neo_blockchain::{BlockchainHandle, RuntimeEvent};
use neo_indexer::{IndexerService, NotificationIndexRecord};
use neo_rpc::application_logs::ApplicationLogsService;
use neo_storage::persistence::Store;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, warn};

mod application_logs;
mod stage;
use application_logs::recover_application_log_notifications;
#[cfg(test)]
use application_logs::{
    ApplicationLogRecoveryError, application_log_notification_records,
    parse_application_log_executions,
};
#[cfg(test)]
use stage::IndexStageError;
use stage::{CanonicalIndexSource, IndexStage};

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

pub(crate) async fn run_live_indexer_when_ready<S>(
    blockchain: BlockchainHandle,
    indexer: Arc<IndexerService>,
    application_logs: Option<Arc<ApplicationLogsService<S>>>,
    startup_height: u64,
) where
    S: Store + 'static,
{
    let start_mode =
        indexer_runtime_start_mode(startup_height, neo_runtime::sync_metrics::peer_live_tip());
    if !wait_until_indexer_runtime_ready(&blockchain, startup_height, start_mode).await {
        return;
    }
    run_live_indexer(blockchain, indexer, application_logs).await;
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
) where
    S: Store + 'static,
{
    let mut events = blockchain.subscribe();
    let notification_logs = application_logs.clone();
    let stage = IndexStage::new(blockchain.clone(), indexer, move |block| {
        recover_application_log_notifications(
            notification_logs.as_deref(),
            block,
            "durable Index stage",
        )
    });
    run_index_stage(&stage, "startup").await;

    loop {
        match events.recv().await {
            Ok(RuntimeEvent::Imported { .. }) => run_index_stage(&stage, "import").await,
            Ok(RuntimeEvent::Reverted { .. }) => run_index_stage(&stage, "revert").await,
            Ok(RuntimeEvent::TipChanged { .. }) => run_index_stage(&stage, "tip change").await,
            Ok(RuntimeEvent::RelayResult { .. }) => {
                // The indexer follows canonical-chain state only. RelayResult
                // is consumed by RPC/subscription surfaces, not block indexing.
            }
            Ok(RuntimeEvent::Shutdown) => break,
            Err(RecvError::Lagged(skipped)) => {
                warn!(
                    target: "neo::indexer",
                    skipped,
                    "Index stage lagged behind chain events; reconciling from its durable checkpoint"
                );
                run_index_stage(&stage, "lag recovery").await;
            }
            Err(RecvError::Closed) => break,
        }
    }
}

async fn run_index_stage<P, N>(stage: &IndexStage<P, N>, reason: &'static str)
where
    P: CanonicalIndexSource,
    N: Fn(&neo_payloads::Block) -> Vec<NotificationIndexRecord> + Send + Sync,
{
    match stage.execute_to_tip().await {
        Ok(outcome) => {
            if outcome.processed_blocks > 0 {
                info!(
                    target: "neo::indexer",
                    stage = outcome.stage.as_str(),
                    reason,
                    start_height = ?outcome.start_height,
                    target_height = outcome.target_height,
                    processed_blocks = outcome.processed_blocks,
                    committed_batches = outcome.committed_batches,
                    indexed_height = ?outcome.checkpoint.indexed_height,
                    "durable Index stage advanced"
                );
            }
        }
        Err(error) => {
            error!(
                target: "neo::indexer",
                reason,
                error = %error,
                "durable Index stage stopped before target"
            );
        }
    }
}

#[cfg(test)]
#[path = "../tests/node/indexer_runtime.rs"]
mod tests;
