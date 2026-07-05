//! Block downloader stream trait.

use futures::Stream;

use super::{BlockDownloadBatch, BlockDownloadConfig};
use crate::NetworkResult;

/// Stream of downloaded block batches.
pub trait BlockDownloader: Stream<Item = NetworkResult<BlockDownloadBatch>> + Send + Unpin {
    /// Downloader scheduling config.
    fn config(&self) -> &BlockDownloadConfig;
}
