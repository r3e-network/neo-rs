//! P2P downloader to sync-import task wiring.
//!
//! This module is the node-layer owner of production block download startup:
//! it snapshots live peers from `neo-network`, composes a
//! `BlockDownloadCoordinator`, and drains ordered batches through
//! `neo-system`'s `SyncDownloadImportDriver`.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Poll cadence for discovering new peer heights and starting coordinator runs.
pub(super) const COORDINATOR_SYNC_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Runs coordinator-backed P2P block download until shutdown.
pub(super) async fn run_coordinator_download_import<C>(
    blockchain: neo_blockchain::BlockchainHandle,
    pipeline: Arc<neo_system::SyncImportPipeline<C>>,
    peer_registry: Arc<neo_network::PeerRegistry>,
    shutdown: CancellationToken,
    config: neo_network::BlockDownloadConfig,
) -> anyhow::Result<()>
where
    C: neo_runtime::SyncStageCheckpointStore + 'static,
{
    let mut tick = tokio::time::interval(COORDINATOR_SYNC_POLL_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick.tick().await;

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            _ = tick.tick() => {
                let local_height = match blockchain.get_height().await {
                    Ok(height) => height,
                    Err(err) if shutdown.is_cancelled() => {
                        debug!(target: "neo::sync", error = %err, "blockchain height unavailable during shutdown");
                        return Ok(());
                    }
                    Err(err) => return Err(anyhow::Error::new(err).context("read blockchain height for coordinator sync")),
                };

                let peers = peer_registry.download_peers();
                let target_height = peers
                    .iter()
                    .map(|peer| peer.height)
                    .max()
                    .unwrap_or(local_height);
                if target_height <= local_height {
                    continue;
                }

                let eligible = peers
                    .into_iter()
                    .filter(|peer| peer.height > local_height)
                    .collect::<Vec<_>>();
                if eligible.is_empty() {
                    continue;
                }

                info!(
                    target: "neo::sync",
                    local_height,
                    target_height,
                    peers = eligible.len(),
                    max_concurrency = config.max_concurrency,
                    max_batch_size = config.max_batch_size,
                    "starting coordinator-backed P2P sync window"
                );

                let downloader = neo_network::BlockDownloadCoordinator::new(
                    local_height,
                    target_height,
                    eligible,
                    config,
                    Arc::clone(&peer_registry),
                );
                let mut driver = neo_system::SyncDownloadImportDriver::new_at_chain_tip(
                    Arc::clone(&pipeline),
                    downloader,
                    local_height,
                );
                match driver.import_all().await {
                    Ok(summary) => {
                        if summary.imported_blocks > 0 {
                            info!(
                                target: "neo::sync",
                                downloaded_batches = summary.downloaded_batches,
                                downloaded_blocks = summary.downloaded_blocks,
                                imported_blocks = summary.imported_blocks,
                                last_imported_height = ?summary.last_imported_height,
                                checkpoints_written = summary.checkpoints_written,
                                "coordinator-backed P2P sync window imported blocks"
                            );
                        }
                    }
                    Err(err) if shutdown.is_cancelled() => {
                        debug!(target: "neo::sync", error = %err, "coordinator sync stopped during shutdown");
                        return Ok(());
                    }
                    Err(err) => {
                        warn!(
                            target: "neo::sync",
                            error = %err,
                            "coordinator-backed P2P sync window failed; will retry with a fresh peer snapshot"
                        );
                    }
                }
            }
        }
    }
}

/// Returns the production downloader policy for the composed storage domains.
///
/// Static archive hooks retain exact Ledger rows until the canonical batch
/// commits. Bound the downloaded batch so that staging remains predictable
/// while preserving one store commit and archive sync for each bounded batch.
#[must_use]
pub(super) fn p2p_block_download_config(
    static_archive_enabled: bool,
) -> neo_network::BlockDownloadConfig {
    let mut config = neo_network::BlockDownloadConfig::default();
    if static_archive_enabled {
        config.max_batch_size = config
            .max_batch_size
            .min(super::static_files::STATIC_ARCHIVE_MAX_DEFERRED_BLOCKS);
    }
    config
}

#[cfg(test)]
#[path = "../tests/node/sync_downloader.rs"]
mod tests;
