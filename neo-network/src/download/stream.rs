//! Block downloader stream trait.
//!
//! Concrete implementations may pull from P2P `GetBlockByIndex`, a local
//! package, or a future state-sync source. Sync drivers consume this trait and
//! pass contiguous batches into `neo_runtime::ImportQueue` or
//! `neo_runtime::BlockImport`.

use futures::Stream;

use super::{BlockDownloadBatch, BlockDownloadConfig};
use crate::NetworkResult;

/// Stream of downloaded block batches.
pub trait BlockDownloader: Stream<Item = NetworkResult<BlockDownloadBatch>> + Send + Unpin {
    /// Downloader scheduling config.
    fn config(&self) -> &BlockDownloadConfig;
}
