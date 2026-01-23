//! CLI command implementations
//!
//! Each submodule implements a specific CLI command that communicates
//! with the Neo node via RPC.

pub mod balance;
pub mod best_block_hash;
pub mod block;
pub mod block_count;
pub mod block_hash;
pub mod broadcast;
pub mod consensus;
pub mod contract;
pub mod export;
pub mod gas;
pub mod header;
pub mod invoke;
pub mod mempool;
pub mod native;
pub mod peers;
pub mod plugins;
pub mod relay;
pub mod send;
pub mod state;
pub mod test_invoke;
pub mod tools;
pub mod transfer;
pub mod transfers;
pub mod tx;
pub mod validate;
pub mod version;
pub mod vote;
pub mod wallet;

use anyhow::Result;

/// Common result type for CLI commands
pub type CommandResult = Result<String>;
