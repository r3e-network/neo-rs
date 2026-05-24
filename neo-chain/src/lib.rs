//! # Neo Chain
//!
//! Standalone blockchain state management for Neo N3.
//!
//! Uses dBFT 2.0 consensus — no fork choice logic needed.

mod block_index;
mod chain_state;
mod error;

pub use block_index::{BlockIndex, BlockIndexEntry};
pub use chain_state::{ChainState, ChainStateSnapshot};
pub use error::{ChainError, ChainResult};

/// Genesis block height
pub const GENESIS_HEIGHT: u32 = 0;

/// Maximum allowed block time drift (in seconds)
pub const MAX_TIME_DRIFT_SECS: u64 = 15 * 60;
