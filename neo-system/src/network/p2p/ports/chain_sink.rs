//! Blockchain command sink port (validated-inventory forwarding).
//!
//! Replaces the session's `BlockchainCommand` mailbox sends
//! (`InventoryBlock`, `InventoryExtensible`, `Headers`). The port names only
//! canonical protocol payloads and the opaque peer handle, so the P2P layer
//! never references the blockchain actor, its command enum, or the runtime.

use super::{PeerHandle, PortResult};
use neo_protocol::payloads::{Block, Extensible, Header};

/// Forwards peer-delivered inventory to the chain/persistence actor.
///
/// # Fail-closed contract
///
/// A non-`Ok` result means the item was **not** queued for the chain actor. The
/// caller must not treat it as accepted: an undelivered block stays out of the
/// persist frontier and its index task is released for retry, and an
/// undelivered header batch simply does not advance the header tip. An adapter
/// must return `Err` whenever delivery to the chain actor cannot be guaranteed.
pub trait ChainSinkPort: Send + Sync {
    /// Submits a received block for validation and persistence.
    ///
    /// * `relay` — request network relay once the block is accepted.
    /// * `pre_verified` — whether the block already passed local verification;
    ///   inbound peer blocks are forwarded unverified (`false`) so the chain
    ///   actor re-validates them against canonical rules.
    /// * `source` — the peer that delivered the block.
    fn submit_inventory_block(
        &self,
        block: Block,
        relay: bool,
        pre_verified: bool,
        source: PeerHandle,
    ) -> PortResult<()>;

    /// Submits a received extensible payload (dBFT / state-service items).
    ///
    /// * `relay` — request network relay once the payload is accepted.
    /// * `source` — the peer that delivered the payload.
    fn submit_inventory_extensible(
        &self,
        payload: Extensible,
        relay: bool,
        source: PeerHandle,
    ) -> PortResult<()>;

    /// Submits a batch of received headers for storage.
    ///
    /// * `source` — the peer that delivered the headers.
    fn submit_headers(&self, headers: Vec<Header>, source: PeerHandle) -> PortResult<()>;
}
