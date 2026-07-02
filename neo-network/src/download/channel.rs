//! Channel-backed block downloader adapter.
//!
//! The adapter keeps tests and composition roots on the same `BlockDownloader`
//! stream contract as future P2P and fast-sync implementations.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use tokio::sync::mpsc;

use super::{BlockDownloadBatch, BlockDownloadConfig, BlockDownloader};
use crate::NetworkResult;

/// Channel-backed downloader adapter.
///
/// This is intentionally small: it provides the stream contract for tests and
/// composition roots while peer-request scheduling evolves independently.
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
