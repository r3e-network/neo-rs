//! # neo-network::download
//!
//! Stream-shaped block download contracts.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. It owns downloader-facing stream
//! contracts, peer bias, concurrency, range assignment, and retry policy. It
//! does not validate blocks, execute transactions, or persist state.
//!
//! ## Contents
//!
//! - `batch`: contiguous downloaded block batches passed into runtime sync.
//! - `channel`: channel-backed downloader adapter for tests and composition
//!   roots.
//! - `config`: bounded request concurrency, batch size, retry, and peer-bias
//!   settings.
//! - `range`: cross-peer range assignment and retry scheduling.
//! - `request`: per-peer `GetBlockByIndex` request-window scheduling.
//! - `stream`: stream trait consumed by sync/import drivers.

mod batch;
mod channel;
mod config;
mod range;
mod request;
mod stream;

pub use batch::BlockDownloadBatch;
pub use channel::ChannelBlockDownloader;
pub use config::BlockDownloadConfig;
pub use range::{BlockDownloadPeer, BlockRangeAssignment, CrossPeerBlockRangeScheduler};
pub use request::{BlockRequest, BlockRequestScheduler};
pub use stream::BlockDownloader;

#[cfg(test)]
#[path = "../tests/download/block_downloader.rs"]
mod tests;
