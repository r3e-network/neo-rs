//! Persistence/relay event subscription port.
//!
//! Replaces the task manager's `EventStreamHandle` subscription in
//! `attach_system`, which registered for `PersistCompleted`, `PersistFailed`,
//! and `RelayResult` from the system event stream and drove
//! `on_persist_completed` / `on_persist_failed` / `on_relay_result`. The
//! `EventStreamHandle` is an actor/system type, so this port defines neutral,
//! protocol-typed event values and hands the task manager a `tokio` receiver
//! instead of a system stream subscription.
//!
//! Only canonical protocol payloads, p2p's own `InventoryType`/`VerifyResult`,
//! and primitives cross the boundary.

use crate::network::p2p::InventoryType;
use crate::network::p2p::VerifyResult;
use neo_primitives::UInt256;
use neo_protocol::payloads::Block;

/// A block that was successfully committed to the local chain.
///
/// Mirrors `PersistCompleted`; carries the whole block because the task manager
/// reads both its index (to advance the persist frontier) and its hash (to
/// release the completed inventory task).
#[derive(Debug, Clone)]
pub struct PersistedBlock {
    /// The committed block.
    pub block: Block,
}

/// A verified block that could not be committed locally.
///
/// Mirrors `PersistFailed`. This is **not** a peer validation failure: the
/// payload already passed verification, so only the block index is carried —
/// the task manager releases the completed task and cached payload at that
/// height so it can be requested again, without pinning the sliding window.
#[derive(Debug, Clone, Copy)]
pub struct PersistFailure {
    /// Index of the block whose local commit failed.
    pub index: u32,
}

/// Outcome of relaying one inventory item.
///
/// Mirrors `RelayResult`. The task manager only reacts when an
/// [`InventoryType::Block`] relay comes back [`VerifyResult::Invalid`], to
/// disconnect the peer that supplied a bad block.
#[derive(Debug, Clone)]
pub struct RelayOutcome {
    /// Type of inventory that was relayed.
    pub inventory_type: InventoryType,
    /// Hash of the relayed inventory.
    pub hash: UInt256,
    /// Optional block index context.
    pub block_index: Option<u32>,
    /// Verification result of the relay.
    pub result: VerifyResult,
}

/// A persistence/relay notification delivered to the task manager.
#[derive(Debug, Clone)]
pub enum PersistenceEvent {
    /// A block was committed to the local chain.
    Persisted(PersistedBlock),
    /// A verified block failed to commit locally.
    Failed(PersistFailure),
    /// An inventory relay completed with a verification result.
    Relay(RelayOutcome),
}

/// Supplies persistence/relay events to the task manager.
///
/// # Fail-closed contract
///
/// The task manager advances its persist frontier only on a
/// [`PersistenceEvent::Persisted`] value. If an adapter cannot deliver events
/// (the receiver is closed or the stream stalls) the manager simply stops
/// seeing new persisted indices; its persistence-frontier timeout then treats
/// the node as not advancing rather than optimistically moving the frontier
/// forward. An adapter must therefore close the channel rather than fabricate
/// events, so the timeout — not a false positive — governs stalled sync.
pub trait PersistenceEventPort: Send + Sync {
    /// Returns a fresh receiver of persistence/relay events.
    ///
    /// Each call yields an independent subscription, replacing the per-actor
    /// `EventStreamHandle::subscribe` registration the task manager performed
    /// once on attach.
    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<PersistenceEvent>;
}
