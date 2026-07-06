//! # neo-network
//!
//! Neo P2P networking, peer management, wire codecs, and service integration.
//!
//! ## Boundary
//!
//! This service crate owns P2P transport and peer behavior and must not execute
//! blocks, own consensus rules, or mutate storage directly.
//!
//! ## Contents
//!
//! - `download`: Stream-shaped block download contracts for sync drivers.
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `identity`: Peer identity, node keys, and advertised endpoint helpers.
//! - `peers`: Peer registry, scoring, and connection tracking logic.
//! - `proto`: Protocol message definitions and network payload framing.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `wire`: Wire encoders, decoders, and deterministic network framing
//!   helpers.

#![doc(html_root_url = "https://docs.rs/neo-network/0.10.0")]

mod download;
mod errors;
mod identity;
mod peers;
pub mod proto;
mod service;
mod spawn;
pub mod wire;

// -----------------------------------------------------------------------------
// Public re-exports
// -----------------------------------------------------------------------------

pub use download::{
    BlockDownloadBatch, BlockDownloadConfig, BlockDownloadCoordinator, BlockDownloadPeer,
    BlockDownloader, BlockRangeAssignment, BlockRangeFetcher, BlockRequest, BlockRequestScheduler,
    ChannelBlockDownloader, CrossPeerBlockRangeScheduler, OrderedBlockBatchBuffer,
};
pub use errors::{NetworkError, NetworkResult, error};
pub use identity::{LocalIdentity, local_identity};
pub use peers::{
    ConnectionTimeouts, PeerId, PeerRegistry, connection_timeouts, peer_id, peer_registry,
};
pub use service::{
    BlockSource, BlockSyncMode, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY, InboundInventory,
    InventoryItem, LocalNodeService, NetworkCommand, NetworkEvent, NetworkHandle,
    RemoteNodeCommand, RemoteNodeHandle, RemoteNodeService, RemoteNodeState, SharedNetworkHandle,
    SyncTask, SyncTaskKind, TaskId, TaskManagerCommand, TaskManagerHandle, TaskManagerService,
    block_sync_mode, command, event, handle, local_node, remote_node, task_manager,
};
pub use wire::{
    Message, MessageCodec, MessageHeader, NetworkMessage, PAYLOAD_MAX_SIZE, ProtocolMessage,
    WireError, WireResult,
};

// -----------------------------------------------------------------------------
// P2P protocol primitives re-export
//
// The former standalone P2P shim was folded into `neo-network` (it was almost
// entirely a re-export layer over `neo-primitives`). Its types now live under
// the `proto` submodule and are re-exported here at the crate root.
// -----------------------------------------------------------------------------

pub use proto::{
    ChannelsConfig, ContainsTransactionType, InvalidWitnessScopeError, InventoryType,
    MessageCommand, MessageFlags, NodeCapabilityType, OracleResponseCode, P2PError, P2PResult,
    TransactionAttributeType, TransactionRemovalReason, VerifyResult, WitnessConditionType,
    WitnessRuleAction, WitnessScope,
};
