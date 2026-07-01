//! # neo-consensus::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-consensus`. This protocol/service crate owns
//! dBFT state and messages and must not own ledger persistence, RPC transport,
//! or application startup.
//!
//! ## Contents
//!
//! - `accessors`: consensus service read accessors.
//! - `block_data`: consensus proposal block data helpers.
//! - `consensus_command`: consensus service command records.
//! - `consensus_event`: consensus service event records.
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `handlers`: service message handlers.
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `lifecycle`: Service startup, shutdown, and background processing
//!   lifecycle helpers.
//! - `proposal`: consensus proposal construction helpers.
//! - `tests`: Module-local tests and regression coverage.

mod accessors;
mod block_data;
mod consensus_command;
mod consensus_event;
mod core;
mod handlers;
mod helpers;
mod lifecycle;
mod proposal;

#[cfg(test)]
#[path = "../tests/service/mod.rs"]
mod tests;

pub use block_data::BlockData;
pub use consensus_command::ConsensusCommand;
pub use consensus_event::ConsensusEvent;
pub use core::ConsensusService;
