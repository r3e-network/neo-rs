//! Blockchain event broadcast type.
//!
//! [`BlockchainEvent`] is the canonical notification type the blockchain
//! service broadcasts to every subscriber (consensus driver, RPC subscriptions,
//! network relay). It covers canonical-chain progress and relay outcomes. It
//! lives in `neo-runtime` — the shared service-contract layer — so subsystems
//! can react without depending on the concrete `neo-blockchain` service
//! implementation.
//!
//! The concrete command channel and handle live in `neo-blockchain`
//! (`BlockchainCommand` / `BlockchainHandle`): that crate owns the command
//! loop and the full set of per-request commands. `neo-runtime` deliberately
//! exposes only the event type plus the default channel capacities shared by
//! both the command and broadcast channels.

use neo_payloads::InventoryType;
use neo_primitives::{UInt256, VerifyResult};

/// Default capacity of the blockchain event broadcast channel. Sized to absorb
/// several 1000-block fast-sync request windows without lagging the producer,
/// while keeping the in-memory queue bounded.
pub const DEFAULT_EVENT_CAPACITY: usize = 4096;

/// Default capacity of the blockchain command channel. Sized to match the
/// broadcast capacity so a burst of peer/imported blocks does not block senders
/// before the broadcast queue fills up.
pub const DEFAULT_COMMAND_CAPACITY: usize = 4096;

/// Events broadcast by the blockchain service on its `subscribe` channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockchainEvent {
    /// A block was imported and became part of the canonical chain.
    Imported {
        /// Hash of the imported block.
        hash: UInt256,
        /// Height the block was assigned in the canonical chain.
        height: u32,
        /// Imported block timestamp in milliseconds since Unix epoch.
        timestamp: u64,
    },
    /// A previously imported block was reverted (re-org, rollback, …).
    Reverted {
        /// Hash of the reverted block.
        hash: UInt256,
        /// Height the block occupied before the revert.
        height: u32,
    },
    /// The canonical tip changed without a new block being imported
    /// (e.g. a fork-choice update chose a different chain tip).
    TipChanged {
        /// New tip hash.
        hash: UInt256,
        /// New tip height.
        height: u32,
    },
    /// A relay attempt completed.
    ///
    /// This mirrors the C# `RelayResult` event surface while keeping the
    /// runtime crate independent of `neo-blockchain`'s concrete command
    /// records. C# v3.10.1 deliberately suppresses failed
    /// `ExtensiblePayload` relay results before publication; the blockchain
    /// service enforces that gate before emitting this event.
    RelayResult {
        /// Hash of the relayed inventory.
        hash: UInt256,
        /// Inventory kind being relayed.
        inventory_type: InventoryType,
        /// Optional block index context for block relay outcomes.
        block_index: Option<u32>,
        /// Verification/admission result.
        result: VerifyResult,
    },
    /// The command loop has been shut down and no further events will
    /// be emitted.
    Shutdown,
}

#[cfg(test)]
#[path = "../tests/service/blockchain.rs"]
mod tests;
