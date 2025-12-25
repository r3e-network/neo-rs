//! Consensus service - the main dBFT state machine.

mod types;
mod core;
mod accessors;
mod handlers;
mod helpers;
mod lifecycle;
mod proposal;

#[cfg(test)]
mod tests;

pub use core::ConsensusService;
pub use types::{BlockData, ConsensusCommand, ConsensusEvent};
