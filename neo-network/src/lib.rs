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
//! - `download`: Correlated header ranges plus stream-shaped body download
//!   contracts for staged sync drivers.
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `identity`: Peer identity, node keys, and advertised endpoint helpers.
//! - `peers`: Peer registry, scoring, and connection tracking logic.
//! - `proto`: Protocol message definitions and network payload framing.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `wire`: Wire encoders, decoders, and deterministic network framing
//!   helpers.

#![doc(html_root_url = "https://docs.rs/neo-network/0.11.0")]

mod download;
mod errors;
mod identity;
mod peers;
mod proto;
mod service;
mod wire;

// -----------------------------------------------------------------------------
// Public re-exports
// -----------------------------------------------------------------------------

pub use download::{
    BlockDownloadBatch, BlockDownloadConfig, BlockDownloadCoordinator, BlockDownloadPeer,
    BlockDownloader, BlockRangeAssignment, BlockRangeFetcher, BlockRequest, ChannelBlockDownloader,
    CrossPeerBlockRangeScheduler, HeaderDownloadBatch, HeaderRequest, OrderedBlockBatchBuffer,
};
pub use errors::{NetworkError, NetworkResult, error};
pub use identity::{LocalIdentity, local_identity};
pub use peers::{
    ConnectedPeerSnapshot, ConnectionTimeouts, PeerId, PeerRegistry, connection_timeouts, peer_id,
    peer_registry,
};
pub use service::{
    BlockSource, ConnectedPeer, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY, InboundInventory,
    InventoryItem, LocalNodeInfo, LocalNodeService, NetworkCommand, NetworkEvent, NetworkHandle,
    RemoteNodeCommand, RemoteNodeHandle, RemoteNodeService, RemoteNodeState, SharedNetworkHandle,
};
pub(crate) use service::{command, event, handle, remote_node};
pub use wire::{
    Message, MessageCodec, MessageHeader, NetworkMessage, PAYLOAD_MAX_SIZE, ProtocolMessage,
    WireError, WireResult,
};

pub use proto::{ChannelsConfig, MessageCommand, MessageCommandParseError, MessageFlags};
