//! Consensus service - the main dBFT state machine.

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
mod tests;

pub use block_data::BlockData;
pub use consensus_command::ConsensusCommand;
pub use consensus_event::ConsensusEvent;
pub use core::ConsensusService;
