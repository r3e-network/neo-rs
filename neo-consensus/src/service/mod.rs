//! Consensus service - the main dBFT state machine.

mod accessors;
mod core;
mod handlers;
mod helpers;
mod lifecycle;

/// Pure consensus policy decisions (proposal selection, validation,
/// commit-egress authorization, recovery handling).
pub mod policy;

mod proposal;
mod types;

#[cfg(test)]
mod tests;

pub use core::ConsensusService;
pub use types::{BlockData, ConsensusCommand, ConsensusEvent};
