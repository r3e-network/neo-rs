//! # neo-network::download
//!
//! Correlated header-range values and stream-shaped body download contracts.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. It owns downloader-facing stream
//! contracts, peer bias, concurrency, body-range assignment, and retry policy.
//! Header responses are transport-correlated here but protocol validation and
//! durable staging remain in the upper composition/blockchain layers. This
//! module does not execute transactions or persist state.
//!
//! ## Contents
//!
//! - `batch`: contiguous downloaded block/header batches passed into runtime sync.
//! - `channel`: channel-backed downloader adapter for tests and composition
//!   roots.
//! - `config`: bounded request concurrency, batch size, retry, and peer-bias
//!   settings.
//! - `coordinator`: transport-agnostic scheduler/buffer/fetcher composition.
//! - `order`: ordered response buffering for multi-peer downloads.
//! - `range`: cross-peer range assignment and retry scheduling.
//! - `request`: `GetHeaders`/`GetBlockByIndex` request values and protocol
//!   limits.
//! - `stream`: stream trait consumed by sync/import drivers.

mod batch;
mod channel;
mod config;
mod coordinator;
mod order;
mod range;
mod request;
mod stream;

pub use batch::{BlockDownloadBatch, HeaderDownloadBatch};
pub use channel::ChannelBlockDownloader;
pub use config::BlockDownloadConfig;
pub use coordinator::{BlockDownloadCoordinator, BlockRangeFetcher};
pub use order::OrderedBlockBatchBuffer;
pub use range::{BlockDownloadPeer, BlockRangeAssignment, CrossPeerBlockRangeScheduler};
pub use request::{BlockRequest, HeaderRequest};
pub use stream::BlockDownloader;

#[cfg(test)]
#[path = "../tests/download/block_downloader.rs"]
mod tests;
