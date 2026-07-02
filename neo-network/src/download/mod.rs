//! # neo-network::download
//!
//! Stream-shaped block download contracts.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. It owns the downloader-facing stream
//! contract, peer bias, concurrency, and retry policy records. It does not
//! validate blocks, execute transactions, or persist state.
//!
//! ## Contents
//!
//! - `BlockDownloadConfig`: bounded request concurrency, batch size, retry, and
//!   peer-bias settings.
//! - `BlockDownloadBatch`: one contiguous downloaded block batch.
//! - `BlockDownloader`: stream trait consumed by sync/import drivers.
//! - `ChannelBlockDownloader`: channel-backed adapter for tests and
//!   composition roots.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use neo_payloads::Block;
use tokio::sync::mpsc;

use crate::{NetworkResult, PeerId};

/// Downloader policy for request scheduling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockDownloadConfig {
    /// Maximum number of in-flight peer requests.
    pub max_concurrency: usize,
    /// Maximum blocks yielded in one stream item.
    pub max_batch_size: usize,
    /// Number of times a failed request may be retried on another peer.
    pub retry_limit: usize,
    /// Preferred peer for biased requests, when a caller is catching up from a
    /// trusted source.
    pub peer_bias: Option<PeerId>,
}

impl Default for BlockDownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 8,
            max_batch_size: 500,
            retry_limit: 2,
            peer_bias: None,
        }
    }
}

impl BlockDownloadConfig {
    /// Construct a config with clamped non-zero concurrency and batch size.
    #[must_use]
    pub fn new(max_concurrency: usize, max_batch_size: usize) -> Self {
        Self {
            max_concurrency: max_concurrency.max(1),
            max_batch_size: max_batch_size.max(1),
            ..Self::default()
        }
    }

    /// Override the retry limit.
    #[must_use]
    pub const fn with_retry_limit(mut self, retry_limit: usize) -> Self {
        self.retry_limit = retry_limit;
        self
    }

    /// Bias requests toward one peer.
    #[must_use]
    pub const fn with_peer_bias(mut self, peer_bias: PeerId) -> Self {
        self.peer_bias = Some(peer_bias);
        self
    }
}

/// One contiguous batch yielded by a block downloader.
#[derive(Clone, Debug)]
pub struct BlockDownloadBatch {
    /// Peer that supplied this batch, when known.
    pub peer_id: Option<PeerId>,
    /// Height of the first block in `blocks`.
    pub start_height: u32,
    /// Downloaded blocks in canonical order.
    pub blocks: Vec<Block>,
}

impl BlockDownloadBatch {
    /// Construct a downloaded batch.
    #[must_use]
    pub fn new(peer_id: Option<PeerId>, start_height: u32, blocks: Vec<Block>) -> Self {
        Self {
            peer_id,
            start_height,
            blocks,
        }
    }

    /// Height immediately after the last block in this batch.
    #[must_use]
    pub fn next_height(&self) -> u32 {
        self.start_height
            .saturating_add(u32::try_from(self.blocks.len()).unwrap_or(u32::MAX))
    }

    /// Returns `true` when this batch carries no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl From<BlockDownloadBatch> for neo_runtime::SyncBlockBatch {
    fn from(batch: BlockDownloadBatch) -> Self {
        Self::new(batch.start_height, batch.blocks)
    }
}

/// Stream of downloaded block batches.
///
/// Concrete implementations may pull from P2P `GetBlockByIndex`, a local
/// package, or a future state-sync source. Sync drivers should consume this
/// trait and pass contiguous batches into `neo_runtime::ImportQueue` or
/// `neo_runtime::BlockImport`.
pub trait BlockDownloader: Stream<Item = NetworkResult<BlockDownloadBatch>> + Send + Unpin {
    /// Downloader scheduling config.
    fn config(&self) -> &BlockDownloadConfig;
}

/// Channel-backed downloader adapter.
///
/// This is intentionally small: it provides the stream contract for tests and
/// composition roots while the peer-request scheduler evolves independently.
#[derive(Debug)]
pub struct ChannelBlockDownloader {
    config: BlockDownloadConfig,
    rx: mpsc::Receiver<NetworkResult<BlockDownloadBatch>>,
}

impl ChannelBlockDownloader {
    /// Build a channel-backed downloader and its sending half.
    #[must_use]
    pub fn channel(
        config: BlockDownloadConfig,
        capacity: usize,
    ) -> (mpsc::Sender<NetworkResult<BlockDownloadBatch>>, Self) {
        let (tx, rx) = mpsc::channel(capacity.max(1));
        (tx, Self { config, rx })
    }
}

impl Stream for ChannelBlockDownloader {
    type Item = NetworkResult<BlockDownloadBatch>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.rx.poll_recv(cx)
    }
}

impl BlockDownloader for ChannelBlockDownloader {
    fn config(&self) -> &BlockDownloadConfig {
        &self.config
    }
}

#[cfg(test)]
#[path = "../tests/download/block_downloader.rs"]
mod tests;
