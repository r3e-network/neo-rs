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
//! - `request`: per-peer `GetBlockByIndex` request-window scheduling.

mod request;

pub use request::{BlockRequest, BlockRequestScheduler};

#[cfg(test)]
#[path = "../tests/download/block_downloader.rs"]
mod tests;
