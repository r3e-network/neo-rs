//! Task-manager command port (bounded sync-task orchestration).
//!
//! Replaces the session's calls into the `TaskManager` actor
//! (`TaskManagerHandle::{register_peer, update_peer, new_tasks, restart_tasks,
//! broadcast_restart_tasks, inventory_completed, headers, forget_hash}`). Those
//! commands were delivered through an `ActorRef` mailbox and named
//! `NeoSystemContext` on attach; this port keeps the same command set but names
//! only the opaque peer handle, p2p's own payload types, primitives, and the
//! canonical protocol block.
//!
//! `AttachSystem` is deliberately **not** a port method: it is one-time wiring
//! the adapter performs at construction, not a per-session runtime command.

use super::{PeerHandle, PortResult};
use crate::payloads::{InvPayload, VersionPayload};
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;

/// Bounded inventory-task orchestration commands.
///
/// The task manager owns all sync-progress constants (`MAX_PENDING_TASKS`,
/// `MAX_RECEIVED_BLOCKS`, `MAX_RECEIVED_BLOCK_BYTES`, `MAX_CONCURRENT_TASKS`,
/// `BLOCK_INDEX_REQUEST_BATCH`, timeouts). Those limits stay inside the task
/// manager's own bounded state; this port only forwards commands to it.
///
/// # Fail-closed contract
///
/// A non-`Ok` result means the command was **not** delivered to the task
/// manager. The caller must not assume the peer's tasks were registered,
/// advanced, or released: an undelivered `inventory_completed` leaves the task
/// outstanding so its timeout retries it, and an undelivered `register_peer`
/// simply leaves the peer unscheduled. An adapter must return `Err` whenever
/// delivery cannot be guaranteed, never silently drop the command and report
/// success.
pub trait TaskManagerPort: Send + Sync {
    /// Registers a newly handshaked peer so it becomes schedulable.
    fn register_peer(&self, peer: PeerHandle, version: VersionPayload) -> PortResult<()>;

    /// Updates a peer's last advertised block index (drives scheduling window).
    fn update_peer(&self, peer: PeerHandle, last_block_index: u32) -> PortResult<()>;

    /// Announces new inventory the peer advertised (`Inv` → task request).
    fn new_tasks(&self, peer: PeerHandle, payload: InvPayload) -> PortResult<()>;

    /// Restarts inventory tasks attributed to a specific peer.
    fn restart_tasks(&self, peer: PeerHandle, payload: InvPayload) -> PortResult<()>;

    /// Broadcasts an inventory restart across all registered peers.
    fn broadcast_restart_tasks(&self, payload: InvPayload) -> PortResult<()>;

    /// Marks a peer-delivered inventory item as completed.
    ///
    /// * `block` — the received block when the completed item was a block, so
    ///   the manager can release its received-block budget and advance.
    /// * `block_index` — the block's index when known, for frontier accounting.
    fn inventory_completed(
        &self,
        peer: PeerHandle,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
    ) -> PortResult<()>;

    /// Records that a peer delivered a header batch.
    fn headers(&self, peer: PeerHandle) -> PortResult<()>;

    /// Forgets a hash whose processing failed before acceptance so it retries.
    fn forget_hash(&self, hash: UInt256) -> PortResult<()>;
}
