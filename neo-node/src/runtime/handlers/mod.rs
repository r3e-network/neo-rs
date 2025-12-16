//! Runtime event handlers.
//!
//! This module contains the event processing loops for different subsystems:
//! - `chain`: Handles blockchain state events (block added, tip changed, reorg)
//! - `consensus`: Handles dBFT consensus events (view change, block commit, broadcast)
//! - `p2p`: Handles P2P network events (peer connect/disconnect, block/tx received)

pub mod chain;
pub mod consensus;
pub mod p2p;

pub use chain::process_chain_events;
pub use consensus::process_consensus_events;
pub use p2p::process_p2p_events;
