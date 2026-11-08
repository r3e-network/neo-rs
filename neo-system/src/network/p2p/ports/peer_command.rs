//! Outbound peer-command port (request/disconnect sends to a session).
//!
//! Replaces the task manager's `peer_commands` helpers
//! (`send_get_data_groups`, `send_get_headers`, `send_get_blocks_by_index`,
//! `send_mempool`, `disconnect`), each of which built a `ProtocolMessage` /
//! `RemoteNodeCommand` and `tell`-ed it into the peer's `ActorRef` mailbox. The
//! port names only the opaque peer handle, p2p's own `InventoryType`, and
//! primitives, so the task manager never touches the actor runtime or the
//! session's concrete command enums.

use super::{PeerHandle, PortResult};
use crate::network::p2p::InventoryType;
use neo_primitives::UInt256;

/// Outbound requests and control signals sent to a connected peer.
///
/// The task manager keeps the request-batching policy (`BLOCK_INDEX_REQUEST_BATCH`,
/// `MAX_BLOCK_INDEX_BATCH`/`MAX_HASHES_COUNT`, the `GetBlockByIndex` count of
/// `-1` for an open-ended header request). This port only carries the resolved
/// request to the wire adapter.
///
/// # Fail-closed contract
///
/// A non-`Ok` result means the message was **not** queued for the peer. The
/// caller must treat the requested inventory as not-yet-asked-for: the task
/// stays outstanding and is retried on the next scheduling pass or timed out,
/// rather than being marked complete. An adapter must return `Err` whenever the
/// send cannot be guaranteed, and must never fabricate success for a dropped
/// message.
pub trait PeerCommandPort: Send + Sync {
    /// Requests inventory by hash for a given type.
    ///
    /// The adapter groups `hashes` into `GetData` batches honoring the maximum
    /// hashes-per-message limit before sending.
    fn send_get_data(
        &self,
        peer: PeerHandle,
        inventory_type: InventoryType,
        hashes: Vec<UInt256>,
    ) -> PortResult<()>;

    /// Requests headers starting at `start_index` (open-ended batch).
    fn send_get_headers(&self, peer: PeerHandle, start_index: u32) -> PortResult<()>;

    /// Requests a bounded run of blocks by index.
    fn send_get_blocks_by_index(
        &self,
        peer: PeerHandle,
        start_index: u32,
        count: i16,
    ) -> PortResult<()>;

    /// Requests the peer's current memory-pool hashes.
    fn send_mempool(&self, peer: PeerHandle) -> PortResult<()>;

    /// Disconnects the peer with a human-readable reason.
    fn disconnect(&self, peer: PeerHandle, reason: String) -> PortResult<()>;
}
