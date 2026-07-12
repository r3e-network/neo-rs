//! Production `Headers -> Bodies -> Import` P2P sync wiring.
//!
//! The daemon owns polling and shutdown policy. `neo-network` owns correlated
//! range transport/retry, `neo-system` owns durable header/body stage
//! composition, and `neo-blockchain` remains the only canonical importer.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use neo_network::{BlockDownloadPeer, HeaderRequest, PeerId};
use neo_runtime::{ServiceError, ServiceResult};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Poll cadence for discovering peer heights and starting fixed sync windows.
pub(super) const COORDINATOR_SYNC_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Runs durable headers-first P2P sync until shutdown.
pub(super) async fn run_staged_sync<C, H>(
    blockchain: neo_blockchain::BlockchainHandle,
    pipeline: Arc<neo_system::StagedSyncPipeline<C, H>>,
    peer_registry: Arc<neo_network::PeerRegistry>,
    shutdown: CancellationToken,
    config: neo_network::BlockDownloadConfig,
) -> anyhow::Result<()>
where
    C: neo_runtime::SyncStageCheckpointStore + 'static,
    H: neo_runtime::VerifiedHeaderStore + 'static,
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
                    Err(err) => return Err(anyhow::Error::new(err).context("read blockchain height for staged sync")),
                };

                let peers = peer_registry.download_peers();
                let peer_tip = peers
                    .iter()
                    .map(|peer| peer.height)
                    .max()
                    .unwrap_or(local_height);
                if peer_tip <= local_height {
                    continue;
                }

                let header_pipeline = pipeline.headers();
                let progress = match header_pipeline.prepare_window(peer_tip).await {
                    Ok(Some(progress)) => progress,
                    Ok(None) => continue,
                    Err(err) if shutdown.is_cancelled() => {
                        debug!(target: "neo::sync", error = %err, "header-stage recovery stopped during shutdown");
                        return Ok(());
                    }
                    Err(err) => {
                        warn!(target: "neo::sync", error = %err, "header-stage recovery failed; will retry");
                        continue;
                    }
                };

                info!(
                    target: "neo::sync",
                    local_height,
                    peer_tip,
                    header_checkpoint = progress.checkpoint.height,
                    target_height = progress.window.target_height,
                    "starting fixed headers-first P2P sync window"
                );

                let progress = match download_verified_headers(
                    Arc::clone(&header_pipeline),
                    Arc::clone(&peer_registry),
                    progress,
                    config.retry_limit,
                    &shutdown,
                )
                .await
                {
                    Ok(progress) => progress,
                    Err(err) if shutdown.is_cancelled() => {
                        debug!(target: "neo::sync", error = %err, "header download stopped during shutdown");
                        return Ok(());
                    }
                    Err(err) => {
                        warn!(target: "neo::sync", error = %err, "header stage failed; retaining durable prefix for retry");
                        continue;
                    }
                };

                let target_height = progress.window.target_height;
                let body_start = match blockchain.get_height().await {
                    Ok(height) => height,
                    Err(_err) if shutdown.is_cancelled() => return Ok(()),
                    Err(err) => {
                        warn!(target: "neo::sync", error = %err, "canonical height unavailable before body stage");
                        continue;
                    }
                };
                if body_start >= target_height {
                    match header_pipeline.finish_imported_window(body_start).await {
                        Ok(_) => continue,
                        Err(err) => {
                            warn!(target: "neo::sync", error = %err, "failed to reconcile already-imported header window");
                            continue;
                        }
                    }
                }

                let eligible = peer_registry
                    .download_peers()
                    .into_iter()
                    .filter(|peer| peer.height >= target_height)
                    .collect::<Vec<_>>();
                if eligible.is_empty() {
                    warn!(
                        target: "neo::sync",
                        target_height,
                        "no ready peer currently covers the fixed body target"
                    );
                    continue;
                }

                let fetcher = pipeline.verified_fetcher(Arc::clone(&peer_registry));
                let downloader = neo_network::BlockDownloadCoordinator::new(
                    body_start,
                    target_height,
                    eligible,
                    config,
                    fetcher,
                );
                let mut driver = neo_system::SyncDownloadImportDriver::new_at_chain_tip(
                    Arc::clone(&pipeline),
                    downloader,
                    body_start,
                );
                match driver.import_all().await {
                    Ok(summary) => {
                        if summary.imported_blocks > 0 {
                            info!(
                                target: "neo::sync",
                                target_height,
                                target_hash = ?progress.window.target_hash,
                                downloaded_batches = summary.downloaded_batches,
                                downloaded_blocks = summary.downloaded_blocks,
                                imported_blocks = summary.imported_blocks,
                                last_imported_height = ?summary.last_imported_height,
                                import_checkpoints_written = summary.import_checkpoints_written,
                                body_checkpoint = ?summary.body_checkpoint.as_ref().map(|checkpoint| checkpoint.height),
                                "headers-first P2P sync window imported canonical blocks"
                            );
                        }
                    }
                    Err(err) if shutdown.is_cancelled() => {
                        debug!(target: "neo::sync", error = %err, "staged sync stopped during shutdown");
                        return Ok(());
                    }
                    Err(err) => {
                        warn!(
                            target: "neo::sync",
                            error = %err,
                            target_height,
                            "body/import stage failed; will recover from canonical progress"
                        );
                    }
                }
            }
        }
    }
}

async fn download_verified_headers<H>(
    pipeline: Arc<neo_system::SyncHeaderPipeline<H>>,
    peer_registry: Arc<neo_network::PeerRegistry>,
    mut progress: neo_system::HeaderStageProgress,
    retry_limit: usize,
    shutdown: &CancellationToken,
) -> ServiceResult<neo_system::HeaderStageProgress>
where
    H: neo_runtime::VerifiedHeaderStore + 'static,
{
    let mut rejected_peers = HashSet::new();
    while !progress.is_complete() {
        let next_height = progress.next_height();
        let count = progress
            .window
            .target_height
            .saturating_sub(next_height)
            .saturating_add(1)
            .min(HeaderRequest::MAX_COUNT);
        let request = HeaderRequest::new(next_height, count);
        let mut candidates = eligible_header_peers(
            peer_registry.download_peers(),
            request.end(),
            &rejected_peers,
        );
        candidates.truncate(retry_limit.saturating_add(1));
        if candidates.is_empty() {
            return Err(ServiceError::unavailable(format!(
                "no ready peer covers header range {}..={}",
                request.start,
                request.end()
            )));
        }

        let mut advanced = false;
        let mut last_error = None;
        for peer in candidates {
            let Some(handle) = peer_registry.handle(peer.peer_id) else {
                rejected_peers.insert(peer.peer_id);
                continue;
            };
            let downloaded = tokio::select! {
                _ = shutdown.cancelled() => {
                    return Err(ServiceError::unavailable("header download cancelled"));
                }
                result = handle.fetch_headers_by_index(request) => result,
            };
            let batch = match downloaded {
                Ok(batch) => batch,
                Err(error) => {
                    neo_runtime::sync_metrics::record_header_fetch_failure();
                    rejected_peers.insert(peer.peer_id);
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            let outcome = match pipeline.accept_downloaded_headers(batch).await {
                Ok(outcome) => outcome,
                Err(error) => {
                    neo_runtime::sync_metrics::record_header_fetch_failure();
                    rejected_peers.insert(peer.peer_id);
                    last_error = Some(error.to_string());
                    continue;
                }
            };
            if outcome.accepted == 0 {
                neo_runtime::sync_metrics::record_header_fetch_failure();
                rejected_peers.insert(peer.peer_id);
                last_error = Some(format!(
                    "peer {} supplied no valid header at {}",
                    peer.peer_id, request.start
                ));
                continue;
            }
            if outcome.rejected() > 0 {
                neo_runtime::sync_metrics::record_header_fetch_failure();
                rejected_peers.insert(peer.peer_id);
                warn!(
                    target: "neo::sync",
                    peer = %peer.peer_id,
                    accepted = outcome.accepted,
                    rejected = outcome.rejected(),
                    "peer header response contained an invalid suffix"
                );
            }
            progress = outcome.progress;
            advanced = true;
            break;
        }
        if !advanced {
            return Err(ServiceError::unavailable(last_error.unwrap_or_else(|| {
                format!(
                    "header range {}..={} made no progress",
                    request.start,
                    request.end()
                )
            })));
        }
    }
    Ok(progress)
}

fn eligible_header_peers(
    peers: Vec<BlockDownloadPeer>,
    end_height: u32,
    excluded: &HashSet<PeerId>,
) -> Vec<BlockDownloadPeer> {
    let mut eligible = peers
        .into_iter()
        .filter(|peer| peer.height >= end_height && !excluded.contains(&peer.peer_id))
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| {
        right
            .height
            .cmp(&left.height)
            .then_with(|| left.peer_id.cmp(&right.peer_id))
    });
    eligible
}

/// Returns the production staged-sync downloader policy.
///
/// Static archive hooks retain exact Ledger rows until the precommit durability
/// fence. Bound each body batch so staging remains predictable while preserving
/// one canonical store commit and archive sync for each bounded batch.
#[must_use]
pub(super) fn p2p_staged_sync_config(
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
