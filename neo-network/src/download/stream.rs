//! Block downloader stream trait.

use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;

use super::{BlockDownloadBatch, BlockDownloadConfig};
use crate::NetworkResult;

/// Stream of downloaded block batches.
pub trait BlockDownloader: Stream<Item = NetworkResult<BlockDownloadBatch>> + Send + Unpin {
    /// Downloader scheduling config.
    fn config(&self) -> &BlockDownloadConfig;

    /// Poll the next downloaded batch.
    ///
    /// Composition crates use this helper with `std::future::poll_fn` so they
    /// can consume the stream contract without adding their own direct
    /// dependency on `futures::StreamExt`.
    fn poll_next_batch(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<NetworkResult<BlockDownloadBatch>>> {
        Stream::poll_next(self, cx)
    }
}
