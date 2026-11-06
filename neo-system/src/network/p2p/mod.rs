//! P2P actor orchestration moved from `neo_core::network::p2p` (task W3-C2).
//!
//! The actor layer (`LocalNode`, `RemoteNode`, `TaskManager`, `TaskSession`,
//! `Peer`) is owned by this crate. The wire vocabulary modules it orchestrates
//! — [`payloads`], [`messages`], [`capabilities`], [`connection`],
//! [`timeouts`] and friends — remain in `neo-core::network::p2p` and are
//! re-exported here so the historical `crate::network::p2p::*` paths used by
//! the moved actor code and by consumers keep resolving unchanged.

pub mod local_node;
pub mod peer;
pub mod remote_node;
pub mod task_manager;
pub mod task_session;

// Wire vocabulary re-exported from neo-core (except connection and upnp which moved here).
pub use neo_core::network::p2p::capabilities;
pub use neo_core::network::p2p::channels_config;
pub use neo_core::network::p2p::framed;
pub use neo_core::network::p2p::helper;
pub use neo_core::network::p2p::message;
pub use neo_core::network::p2p::message_command;
pub use neo_core::network::p2p::message_flags;
pub use neo_core::network::p2p::messages;
pub use neo_core::network::p2p::payloads;
pub use neo_core::network::p2p::timeouts;

// connection and upnp modules now live in neo-system (moved from neo-core).
pub mod connection;
pub mod upnp;

// Re-export commonly used types (actor layer local, vocabulary via neo-core).
pub use local_node::{
    BroadcastEvent, LocalNode, LocalNodeActor, LocalNodeCommand, LocalNodeHandle, RelayInventory,
    RemoteNodeSnapshot,
};
pub use peer::{ConnectedPeer, MAX_COUNT_FROM_SEED_LIST, PeerCommand, PeerState, PeerTimer};
pub use remote_node::{
    MessageHandlerSubscription, RemoteNode, RemoteNodeCommand, register_message_received_handler,
    unregister_message_received_handler,
};
pub use task_manager::{TaskManager, TaskManagerActor, TaskManagerCommand, TaskManagerHandle};
pub use task_session::TaskSession;

// Security hardening: rate limiting, peer reputation and ban management lives
// in `neo_p2p::peer_policy`; re-exported here for compatibility.
pub use neo_p2p::peer_policy::{
    BanEntry, BanList, DEFAULT_BAN_DURATION, DEFAULT_INBOUND_CONNECTION_BURST,
    DEFAULT_INBOUND_CONNECTION_RATE, DEFAULT_REPUTATION_THRESHOLD, InboundRateLimiter,
    PeerReputation, PeerReputationTracker, reputation, validate_peer_endpoint,
};

// Flat type re-exports mirroring the old neo-core p2p module surface.
pub use neo_core::network::p2p::channels_config::ChannelsConfig;
pub use connection::PeerConnection;
pub use neo_core::network::p2p::framed::FrameConfig;
pub use neo_core::network::p2p::helper::{get_sign_data, get_sign_data_vec};
pub use neo_core::network::p2p::message::Message;
pub use neo_core::network::p2p::message_command::MessageCommand;
pub use neo_core::network::p2p::message_flags::MessageFlags;
pub use neo_core::network::p2p::messages::{MessageHeader, NetworkMessage, ProtocolMessage};
