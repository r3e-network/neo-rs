//! P2P actor orchestration and protocol types.
//!
//! This module contains the complete P2P networking layer, including:
//! - Actor layer: `LocalNode`, `RemoteNode`, `TaskManager`, `TaskSession`, `Peer`
//! - Wire protocol: messages, payloads, capabilities, framing
//! - Ports: hexagonal boundary traits
//! - Services: peer management and network coordination

// ============================================================================
// Module Declarations
// ============================================================================

// Actor layer modules
pub mod local_node;
pub mod peer;
pub mod remote_node;
pub mod task_manager;
pub mod task_session;

// Wire protocol modules
pub mod connection;
pub mod message;
pub mod messages;
pub mod payloads;

// Protocol support modules
pub mod channels_config;
pub mod contains_transaction_type;
pub mod error;
pub mod events;
pub mod framed;
pub mod framed_codec;
pub mod inventory_type;
pub mod message_command;
pub mod message_flags;
pub mod node_capability_type;
pub mod protocol_message;
pub mod services;
pub mod timeouts;
pub mod traits;
pub mod verify_result;

// Peer management
pub mod peer_policy;

// Hexagonal ports
pub mod ports;

// Session state
pub mod session_state;
pub mod sessions;

// Supporting functionality
pub mod alerts;
pub mod block_validation;
pub mod bloom_filter;
pub mod completion_flow;
pub mod handshake;
pub mod inventory;
pub mod outbound_queue;
pub mod peer_messages;
pub mod pending_known_hashes;
pub mod scheduling;

// ============================================================================
// Re-exports from neo-primitives
// ============================================================================

pub use neo_primitives::{
    InvalidWitnessScopeError, OracleResponseCode, TransactionAttributeType,
    TransactionRemovalReason, WitnessConditionType, WitnessRuleAction, WitnessScope,
};

// ============================================================================
// Public Type Re-exports
// ============================================================================

// Core types
pub use channels_config::ChannelsConfig;
pub use contains_transaction_type::ContainsTransactionType;
pub use error::{P2PError, P2PResult};
pub use events::{MessageReceivedHandler, NetworkMessageRef};
pub use framed::{FrameConfig, WriteBuffer};
pub use framed_codec::NeoMessageCodec;
pub use inventory_type::InventoryType;
pub use message::{Message, PAYLOAD_MAX_SIZE};
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use node_capability_type::NodeCapabilityType;
pub use protocol_message::{MessageHeader, NetworkMessage, ProtocolMessage};
pub use verify_result::VerifyResult;

// Connection
pub use connection::PeerConnection;

// Peer policy
pub use peer_policy::{
    BanEntry, BanList, DEFAULT_BAN_DURATION, DEFAULT_INBOUND_CONNECTION_BURST,
    DEFAULT_INBOUND_CONNECTION_RATE, DEFAULT_REPUTATION_THRESHOLD, InboundRateLimiter,
    PeerReputation, PeerReputationTracker, reputation, validate_peer_endpoint,
};

// Services
pub use services::PeerManagerService;

// Traits
pub use traits::{
    Broadcaster, DataRequester, P2PConfig, P2PEvent, P2PEventSubscriber, P2PService, PeerInfo,
    PeerManager,
};

// Actor layer exports
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

// Re-export capabilities for compatibility
pub use payloads::node_capability as capabilities;

// Signing utilities
pub use neo_protocol::signing_data::{get_sign_data, get_sign_data_vec};
