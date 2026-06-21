//! Outcome / payload types shared by the service traits.
//!
//! The execution, validation, and network-event payloads in this crate are
//! deliberately compact service-boundary DTOs. Rich per-subsystem details
//! remain in concrete implementation crates such as `neo-execution`,
//! `neo-blockchain`, and `neo-network`; this crate carries only the fields
//! that cross trait-object boundaries.
//!
//! `NetworkEvent` is a sealed-style sum type covering the events a
//! `NetworkService` is expected to broadcast on its `broadcast::Sender`.
//! Concrete `neo-network` code translates its richer internal events into
//! these stable runtime events before broadcasting across the service layer.

use std::net::SocketAddr;

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Result of executing a block via the [`crate::NeoEngine`] /
/// [`crate::BlockExecutor`] services.
///
/// This is the compact summary returned across runtime service boundaries.
/// Rich execution artifacts such as `ApplicationExecuted` records stay in
/// `neo-execution` / `neo-blockchain`, while callers here receive the block
/// identity, success flag, and total GAS consumed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    /// Hash of the block the outcome corresponds to.
    pub block_hash: UInt256,
    /// Height of the block the outcome corresponds to.
    pub block_height: u32,
    /// `true` when the block executed without a VM fault.
    pub ok: bool,
    /// Total GAS consumed by the block.
    pub gas_consumed: u64,
}

impl ExecutionOutcome {
    /// Convenience constructor for a successful outcome.
    pub fn success(block_hash: UInt256, block_height: u32, gas_consumed: u64) -> Self {
        Self {
            block_hash,
            block_height,
            ok: true,
            gas_consumed,
        }
    }

    /// Convenience constructor for a failed outcome.
    pub fn failure(block_hash: UInt256, block_height: u32) -> Self {
        Self {
            block_hash,
            block_height,
            ok: false,
            gas_consumed: 0,
        }
    }
}

/// Engine-API-style execution payload.
///
/// In reth this is the typed return value of `engine_executePayload`; in
/// neo-rs it is the analogous blob returned by [`crate::NeoEngine::execute_block`].
/// The payload currently wraps the compact [`ExecutionOutcome`]. Raw
/// execution notifications and state-root artifacts remain owned by the
/// concrete execution and blockchain crates.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPayload {
    /// The execution outcome for the block.
    pub outcome: ExecutionOutcome,
}

/// Result of validating (but not executing) a block.
///
/// The validation pipeline runs in two phases in the reth model: a cheap
/// *consensus* check (header / merkle / witness shape) and an expensive
/// *execution* check (state transition). [`crate::NeoEngine::validate_block`]
/// is the cheap, consensus-shaped variant — anything beyond header / merkle
/// belongs in `execute_block`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationResult {
    /// `true` when every check the validator is responsible for passed.
    pub valid: bool,
    /// Optional human-readable reason for a `valid == false` result.
    pub reason: Option<String>,
}

impl ValidationResult {
    /// Construct a `valid == true` result.
    pub fn ok() -> Self {
        Self {
            valid: true,
            reason: None,
        }
    }

    /// Construct a `valid == false` result with a reason.
    pub fn invalid<E: ToString>(reason: E) -> Self {
        Self {
            valid: false,
            reason: Some(reason.to_string()),
        }
    }
}

/// Events broadcast by a [`crate::NetworkService`] on its
/// `tokio::sync::broadcast::Sender`.
///
/// The variant set mirrors the small set of "lifecycle" events a
/// network service is expected to publish: peer changes, blocks
/// received, transactions received. Future stages will swap the inner
/// types for the canonical `neo-network::wire` / `neo-payloads` representations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEvent {
    /// A new peer has joined the connected set.
    PeerConnected {
        /// Stable identifier for the peer as exposed by the network service.
        peer_id: String,
        /// Reported endpoint of the peer, mirroring the
        /// `Remote.Address` / `ListenerTcpPort` pair C#'s
        /// `LocalNode.GetRemoteNodes` serves.
        ///
        /// For outbound peers this is the dialed endpoint (which *is*
        /// the peer's listener). For inbound peers the connection
        /// initially reports `(remote_ip, 0)` — the C# unknown-listener
        /// form (`RemoteNode.ListenerTcpPort` starts at 0) — and the
        /// per-peer service publishes this event *again* with the
        /// upgraded `(remote_ip, advertised_listener_port)` endpoint
        /// once the peer's version payload advertises a `TcpServer`
        /// capability (C# `LocalNode.AllowNewConnection` updating the
        /// connected endpoint to `node.Listener`). Consumers keying by
        /// `peer_id` must treat a repeated `PeerConnected` as an
        /// address update for the existing peer, not a new peer.
        /// `None` when the publisher does not know the transport
        /// address.
        address: Option<SocketAddr>,
    },
    /// A previously connected peer has dropped.
    PeerDisconnected {
        /// Stable identifier for the peer.
        peer_id: String,
    },
    /// A block was received from a remote peer.
    BlockReceived {
        /// Hash of the received block.
        block_hash: UInt256,
    },
    /// A transaction was received from a remote peer.
    TransactionReceived {
        /// Hash of the received transaction.
        tx_hash: UInt256,
    },
}

#[cfg(test)]
#[path = "tests/outcome.rs"]
mod tests;
