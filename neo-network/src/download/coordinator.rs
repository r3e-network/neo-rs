//! Transport-agnostic multi-peer block downloader.
//!
//! The coordinator composes the cross-peer range scheduler and ordered response
//! buffer. It delegates the actual wire request to a caller-provided
//! [`BlockRangeFetcher`], keeping socket transport outside the scheduling and
//! ordering policy.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::task::JoinSet;

use super::{
    BlockDownloadBatch, BlockDownloadConfig, BlockDownloadPeer, BlockDownloader,
    BlockRangeAssignment, CrossPeerBlockRangeScheduler, OrderedBlockBatchBuffer,
};
use crate::{NetworkError, NetworkResult};

type FetchResult = (BlockRangeAssignment, NetworkResult<BlockDownloadBatch>);

/// Fetches one assigned block range from the selected peer.
pub trait BlockRangeFetcher: Clone + Send + Sync + Unpin + 'static {
    /// Fetch the blocks for `assignment`.
    fn fetch_range(
        &self,
        assignment: BlockRangeAssignment,
    ) -> impl std::future::Future<Output = NetworkResult<BlockDownloadBatch>> + Send + 'static;
}

/// Multi-peer downloader that yields ordered block batches.
pub struct BlockDownloadCoordinator<F: BlockRangeFetcher> {
    config: BlockDownloadConfig,
    scheduler: CrossPeerBlockRangeScheduler,
    buffer: OrderedBlockBatchBuffer,
    peers: Vec<BlockDownloadPeer>,
    fetcher: F,
    in_flight: JoinSet<FetchResult>,
}

impl<F: BlockRangeFetcher> std::fmt::Debug for BlockDownloadCoordinator<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockDownloadCoordinator")
            .field("config", &self.config)
            .field("scheduler", &self.scheduler)
            .field("buffer", &self.buffer)
            .field("peers", &self.peers)
            .field("in_flight", &self.in_flight.len())
            .finish_non_exhaustive()
    }
}

impl<F: BlockRangeFetcher> BlockDownloadCoordinator<F> {
    /// Create a coordinator for `(local_height, target_height]`.
    #[must_use]
    pub fn new(
        local_height: u32,
        target_height: u32,
        peers: Vec<BlockDownloadPeer>,
        config: BlockDownloadConfig,
        fetcher: F,
    ) -> Self {
        Self {
            config,
            scheduler: CrossPeerBlockRangeScheduler::new(local_height, target_height, config),
            buffer: OrderedBlockBatchBuffer::new(local_height.saturating_add(1)),
            peers,
            fetcher,
            in_flight: JoinSet::new(),
        }
    }

    /// Replace the peer snapshot used for future assignments.
    pub fn set_peers(&mut self, peers: Vec<BlockDownloadPeer>) {
        self.peers = peers;
    }

    /// Current peer snapshot.
    #[must_use]
    pub fn peers(&self) -> &[BlockDownloadPeer] {
        &self.peers
    }

    /// Height expected for the next emitted batch.
    #[must_use]
    pub fn next_height(&self) -> u32 {
        self.buffer.next_height()
    }

    /// Number of active wire fetches.
    #[must_use]
    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    fn fill_in_flight(&mut self) {
        while self.in_flight.len() < self.config.max_concurrency {
            let Some(assignment) = self.scheduler.next_assignment(&self.peers) else {
                break;
            };
            let fetcher = self.fetcher.clone();
            self.in_flight.spawn(async move {
                let result = fetcher.fetch_range(assignment).await;
                (assignment, result)
            });
        }
    }

    fn no_progress_error(&self) -> NetworkError {
        NetworkError::Protocol(format!(
            "no eligible peer can serve next block range starting at {}",
            self.buffer.next_height()
        ))
    }
}

impl<F: BlockRangeFetcher> Stream for BlockDownloadCoordinator<F> {
    type Item = NetworkResult<BlockDownloadBatch>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if let Some(batch) = this.buffer.pop_ready() {
            return Poll::Ready(Some(Ok(batch)));
        }

        loop {
            this.fill_in_flight();

            if let Some(batch) = this.buffer.pop_ready() {
                return Poll::Ready(Some(Ok(batch)));
            }

            if this.in_flight.is_empty() {
                if this.scheduler.is_complete() {
                    return Poll::Ready(None);
                }
                return Poll::Ready(Some(Err(this.no_progress_error())));
            }

            match this.in_flight.poll_join_next(cx) {
                Poll::Ready(Some(Ok((assignment, Ok(mut batch))))) => {
                    if let Err(err) = this.scheduler.record_success(assignment) {
                        return Poll::Ready(Some(Err(err)));
                    }
                    if batch.peer_id.is_none() {
                        batch.peer_id = Some(assignment.peer_id);
                    }
                    if let Err(err) = this.buffer.insert(batch) {
                        return Poll::Ready(Some(Err(err)));
                    }
                    continue;
                }
                Poll::Ready(Some(Ok((assignment, Err(err))))) => {
                    if let Err(retry_err) = this.scheduler.record_failure(assignment) {
                        return Poll::Ready(Some(Err(retry_err)));
                    }
                    tracing::debug!(
                        target: "neo_network::download",
                        peer = %assignment.peer_id,
                        start = assignment.request.start,
                        end = assignment.request.end(),
                        attempt = assignment.attempt,
                        error = %err,
                        "block range fetch failed; scheduling retry"
                    );
                    continue;
                }
                Poll::Ready(Some(Err(err))) => {
                    return Poll::Ready(Some(Err(NetworkError::Protocol(format!(
                        "block range fetch task failed: {err}"
                    )))));
                }
                Poll::Ready(None) => {
                    if this.scheduler.is_complete() {
                        return Poll::Ready(None);
                    }
                    return Poll::Ready(Some(Err(this.no_progress_error())));
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<F: BlockRangeFetcher> BlockDownloader for BlockDownloadCoordinator<F> {
    fn config(&self) -> &BlockDownloadConfig {
        &self.config
    }
}
