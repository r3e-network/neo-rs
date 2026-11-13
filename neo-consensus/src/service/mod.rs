//! Consensus service - the main dBFT state machine.

mod accessors;
mod core;
mod handlers;
mod helpers;
mod lifecycle;
mod proposal;
mod types;

#[cfg(test)]
mod tests;

pub use core::ConsensusService;
pub use types::{BlockData, ConsensusCommand, ConsensusEvent};
