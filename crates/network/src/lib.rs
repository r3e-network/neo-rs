//! Neo Network Module
//!
//! This module provides comprehensive networking functionality for the Neo blockchain,
//! including P2P communication, message handling, peer management, and RPC services.
//!
//! ## Components
//!
//! - **P2P**: Peer-to-peer communication and protocol handling
//! - **Messages**: Network message types and serialization
//! - **Peers**: Peer discovery, connection management, and routing
//! - **Sync**: Blockchain synchronization and consensus
//! - **RPC**: JSON-RPC server for external API access
//! - **Server**: Network server coordination and management

pub mod composite_handler;
pub mod error;
pub mod error_handling;
pub mod handlers;
pub mod messages;
pub mod p2p;
pub mod p2p_node;
pub mod peer_manager;
pub mod peers;
pub mod relay_cache;
pub mod rpc;
pub mod server;
pub mod shutdown_impl;
pub mod snapshot_config;
pub mod sync;
pub mod transaction_relay;

// Constants
const UNKNOWN_PEER_ADDR: &str = "0.0.0.0:0";
const DEFAULT_MAINNET_PORT: &str = "10333";
const DEFAULT_PRIVNET_PORT: &str = "30333";
const DEFAULT_WS_PORT: &str = "10334";
const LOCALHOST: &str = "127.0.0.1";

// Re-export main types
pub use crate::error_handling::{
    ErrorStatistics, NetworkErrorEvent, NetworkErrorHandler, OperationContext, RecoveryStrategy,
};
pub use crate::handlers::TransactionMessageHandler;
pub use crate::messages::{
    InventoryItem, InventoryType, MessageCommand, MessageValidator, Neo3Message, NetworkMessage,
    ProtocolMessage,
};
pub use crate::p2p_node::{
    NodeCapability, NodeEvent, NodeStatistics, NodeStatus, P2pNode, PeerInfo,
};
pub use crate::peer_manager::{ConnectionStats, PeerConnection, PeerEvent, PeerManager, PeerState};
pub use crate::relay_cache::RelayCache;
pub use crate::transaction_relay::{
    RelayStatistics, TransactionRelay, TransactionRelayConfig, TransactionRelayEvent,
};
pub use error::{ErrorSeverity, NetworkError, NetworkResult, Result};

pub type P2PEvent = NodeEvent;
pub type P2PNode = P2pNode;

// Global sync manager reference for direct peer height updates
use once_cell::sync::Lazy;
use std::sync::Mutex;

pub static GLOBAL_SYNC_MANAGER: Lazy<Mutex<Option<std::sync::Arc<sync::SyncManager>>>> =
    Lazy::new(|| Mutex::new(None));

/// Set the global sync manager reference
pub fn set_global_sync_manager(sync_manager: std::sync::Arc<sync::SyncManager>) {
    if let Ok(mut guard) = GLOBAL_SYNC_MANAGER.lock() {
        *guard = Some(sync_manager);
    }
}
pub type RpcServer = crate::rpc::RpcServer;
pub type SyncManager = crate::sync::SyncManager;
pub type SyncEvent = crate::sync::SyncEvent;

// Configuration types are already defined in this file

use neo_config::DEFAULT_NEO_PORT;
use neo_config::DEFAULT_RPC_PORT;
use neo_config::DEFAULT_TESTNET_PORT;
use neo_config::DEFAULT_TESTNET_RPC_PORT;
use neo_config::{MAINNET_SEEDS, N3_TESTNET_SEEDS};
use neo_core::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::SocketAddr;
use thiserror::Error;

/// Default Neo network ports
/// Legacy error type for backward compatibility
///
/// **Deprecated**: Use [`NetworkError`] instead for new code.
#[deprecated(since = "0.3.0", note = "Use NetworkError instead")]
pub use LegacyError as Error;

/// Legacy network errors for backward compatibility
#[derive(Error, Debug)]
pub enum LegacyError {
    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Protocol error
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Peer error
    #[error("Peer error: {0}")]
    Peer(String),

    /// Synchronization error
    #[error("Synchronization error: {0}")]
    Sync(String),

    /// RPC error
    #[error("RPC error: {0}")]
    Rpc(String),

    /// Timeout error
    #[error("Timeout error: {0}")]
    Timeout(String),

    /// Authentication error
    #[error("Authentication error: {0}")]
    Authentication(String),

    /// Rate limiting error
    #[error("Rate limit exceeded: {0}")]
    RateLimit(String),

    /// Invalid message error
    #[error("Invalid message: {0}")]
    InvalidMessage(String),

    /// Invalid header error
    #[error("Invalid header: {0}")]
    InvalidHeader(String),

    /// Network configuration error
    #[error("Configuration error: {0}")]
    Configuration(String),

    /// Peer already connected error
    #[error("Peer already connected: {0}")]
    PeerAlreadyConnected(String),

    /// Connection limit reached error
    #[error("Connection limit reached")]
    ConnectionLimitReached,

    /// Connection failed error
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    /// Connection timeout error
    #[error("Connection timeout")]
    ConnectionTimeout,

    /// Peer not connected error
    #[error("Peer not connected: {0}")]
    PeerNotConnected(String),

    /// Message send failed error
    #[error("Message send failed: {0}")]
    MessageSendFailed(String),

    /// Handshake failed error
    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    /// Handshake timeout error
    #[error("Handshake timeout")]
    HandshakeTimeout,

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Ledger error
    #[error("Ledger error: {0}")]
    Ledger(String),

    /// Generic error
    #[error("Network error: {0}")]
    Generic(String),
}

// Legacy error conversions
impl From<neo_ledger::Error> for LegacyError {
    fn from(err: neo_ledger::Error) -> Self {
        LegacyError::Ledger(err.to_string())
    }
}

impl From<neo_io::Error> for LegacyError {
    fn from(err: neo_io::Error) -> Self {
        LegacyError::Serialization(err.to_string())
    }
}

impl From<neo_core::CoreError> for LegacyError {
    fn from(err: neo_core::CoreError) -> Self {
        LegacyError::Protocol(err.to_string())
    }
}

/// Network protocol version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProtocolVersion {
    /// Major version
    pub major: u32,
    /// Minor version
    pub minor: u32,
    /// Patch version
    pub patch: u32,
}

impl ProtocolVersion {
    /// Creates a new protocol version
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Current Neo protocol version  
    pub fn current() -> Self {
        // Neo N3 uses protocol version 0 (zero) consistently across all implementations
        // This is different from semantic versioning - it's just a single protocol version number
        Self::from_u32(0) // Official Neo N3 protocol version
    }

    /// Checks if this version is compatible with another
    pub fn is_compatible(&self, other: &ProtocolVersion) -> bool {
        self.major == other.major && self.minor >= other.minor
    }

    /// Converts to a single u32 value (for network protocol compatibility)
    pub fn as_u32(&self) -> u32 {
        ((self.major & 0xFF) << 24) | ((self.minor & 0xFF) << 16) | (self.patch & 0xFFFF)
    }

    /// Creates from a u32 value
    pub fn from_u32(value: u32) -> Self {
        Self {
            major: (value >> 24) & 0xFF,
            minor: (value >> 16) & 0xFF,
            patch: value & 0xFFFF,
        }
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Default for ProtocolVersion {
    fn default() -> Self {
        Self::current()
    }
}

/// Network node information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node ID
    pub id: UInt160,
    /// Node version
    pub version: ProtocolVersion,
    /// Node user agent
    pub user_agent: String,
    /// Node capabilities
    pub capabilities: Vec<String>,
    /// Node start height
    pub start_height: u32,
    /// Node timestamp
    pub timestamp: u64,
    /// Node nonce
    pub nonce: u32,
}

impl NodeInfo {
    /// Creates a new node info
    pub fn new(id: UInt160, start_height: u32) -> Self {
        Self {
            id,
            version: ProtocolVersion::current(),
            user_agent: "/Neo:3.0.0/".to_string(),
            capabilities: vec![
                "FullNode".to_string(),
                "TcpServer".to_string(),
                "WsServer".to_string(),
            ],
            start_height,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("Operation failed")
                .as_secs(),
            nonce: rand::random(),
        }
    }
}

/// Network statistics
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkStats {
    /// Number of connected peers
    pub peer_count: usize,
    /// Number of inbound connections
    pub inbound_connections: usize,
    /// Number of outbound connections
    pub outbound_connections: usize,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Messages sent per second
    pub messages_sent_per_sec: f64,
    /// Messages received per second
    pub messages_received_per_sec: f64,
    /// Average latency in milliseconds
    pub average_latency_ms: f64,
    /// Sync status
    pub sync_status: String,
    /// Current block height
    pub current_height: u32,
    /// Best known height
    pub best_known_height: u32,
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self {
            peer_count: 0,
            inbound_connections: 0,
            outbound_connections: 0,
            bytes_sent: 0,
            bytes_received: 0,
            messages_sent_per_sec: 0.0,
            messages_received_per_sec: 0.0,
            average_latency_ms: 0.0,
            sync_status: "Disconnected".to_string(),
            current_height: 0,
            best_known_height: 0,
        }
    }
}

/// Network command types for external control
#[derive(Debug, Clone)]
pub enum NetworkCommand {
    /// Connect to a specific peer
    ConnectToPeer(SocketAddr),
    /// Disconnect from a specific peer
    DisconnectPeer(SocketAddr),
    /// Send a message to a specific peer
    SendMessage {
        peer: SocketAddr,
        message: NetworkMessage,
    },
    /// Broadcast a message to all peers
    BroadcastMessage(NetworkMessage),
    /// Stop the network service
    Stop,
}

/// Message handler for protocol processing
pub struct MessageHandler {
    config: NetworkConfig,
}

impl MessageHandler {
    pub fn new(config: NetworkConfig) -> Result<Self> {
        Ok(Self { config })
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

/// Network event types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkEvent {
    /// Peer connected
    PeerConnected {
        peer_id: UInt160,
        address: SocketAddr,
    },
    /// Peer disconnected
    PeerDisconnected { peer_id: UInt160, reason: String },
    /// Message received
    MessageReceived {
        peer_id: UInt160,
        message_type: String,
    },
    /// Block received
    BlockReceived { block_hash: UInt256, height: u32 },
    /// Transaction received
    TransactionReceived { tx_hash: UInt256 },
    /// Sync started
    SyncStarted { target_height: u32 },
    /// Sync completed
    SyncCompleted { final_height: u32 },
    /// Sync failed
    SyncFailed { error: String },
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network magic number
    pub magic: u32,
    /// Protocol version
    pub protocol_version: ProtocolVersion,
    /// Node user agent
    pub user_agent: String,
    /// Listen address
    pub listen_address: SocketAddr,
    /// P2P configuration
    pub p2p_config: P2PConfig,
    /// RPC configuration
    pub rpc_config: Option<RpcConfig>,
    /// Maximum number of peers
    pub max_peers: usize,
    /// Maximum outbound connections
    pub max_outbound_connections: usize,
    /// Maximum inbound connections
    pub max_inbound_connections: usize,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Handshake timeout in seconds
    pub handshake_timeout: u64,
    /// Ping interval in seconds
    pub ping_interval: u64,
    /// Enable transaction relay
    pub enable_relay: bool,
    /// Seed nodes for peer discovery
    pub seed_nodes: Vec<SocketAddr>,
    /// P2P port
    pub port: u16,
    /// WebSocket enabled
    pub websocket_enabled: bool,
    /// WebSocket port
    pub websocket_port: u16,
}

/// P2P configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PConfig {
    pub listen_address: SocketAddr,
    pub max_peers: usize,
    pub connection_timeout: std::time::Duration,
    pub handshake_timeout: std::time::Duration,
    pub ping_interval: std::time::Duration,
    pub message_buffer_size: usize,
    pub enable_compression: bool,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            listen_address: format!("{}:{}", LOCALHOST, DEFAULT_MAINNET_PORT)
                .parse()
                .expect("value should parse"),
            max_peers: 100,
            connection_timeout: std::time::Duration::from_secs(30),
            handshake_timeout: std::time::Duration::from_secs(10),
            ping_interval: std::time::Duration::from_secs(30),
            message_buffer_size: 1000,
            enable_compression: false,
        }
    }
}

/// RPC configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub max_connections: usize,
    pub http_address: Option<SocketAddr>,
    pub ws_address: Option<SocketAddr>,
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_address: "localhost".to_string(),
            port: 10332,
            max_connections: 100,
            http_address: Some(
                format!("{}:{}", LOCALHOST, DEFAULT_RPC_PORT)
                    .parse()
                    .expect("value should parse"),
            ),
            ws_address: Some(
                format!("{}:{}", LOCALHOST, DEFAULT_WS_PORT)
                    .parse()
                    .expect("value should parse"),
            ),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            magic: 0x334F454E, // Neo N3 mainnet magic (860833102 in decimal)
            protocol_version: ProtocolVersion::current(),
            user_agent: "/Neo:3.0.0/".to_string(),
            listen_address: format!("{}:{}", LOCALHOST, DEFAULT_MAINNET_PORT)
                .parse()
                .expect("value should parse"),
            p2p_config: P2PConfig::default(),
            rpc_config: Some(RpcConfig::default()),
            max_peers: 100,
            max_outbound_connections: 10,
            max_inbound_connections: 40,
            connection_timeout: 30,
            handshake_timeout: 10,
            ping_interval: 30,
            enable_relay: true,
            seed_nodes: MAINNET_SEEDS
                .iter()
                .map(|s| {
                    s.parse()
                        .unwrap_or_else(|_| UNKNOWN_PEER_ADDR.parse().expect("value should parse"))
                })
                .collect(),
            port: 10333,
            websocket_enabled: false,
            websocket_port: 10334,
        }
    }
}

impl NetworkConfig {
    /// Gets connection timeout in seconds
    pub fn connection_timeout_secs(&self) -> u64 {
        self.connection_timeout
    }
    /// Creates a testnet configuration
    pub fn testnet() -> Self {
        Self {
            magic: 0x3554334E, // Neo N3 testnet magic (894448462 in decimal)
            listen_address: format!("{}:{}", LOCALHOST, DEFAULT_TESTNET_PORT)
                .parse()
                .expect("value should parse"),
            p2p_config: P2PConfig {
                listen_address: format!("{}:{}", LOCALHOST, DEFAULT_TESTNET_PORT)
                    .parse()
                    .expect("value should parse"),
                max_peers: 100,
                connection_timeout: std::time::Duration::from_secs(30),
                handshake_timeout: std::time::Duration::from_secs(10),
                ping_interval: std::time::Duration::from_secs(30),
                message_buffer_size: 1000,
                enable_compression: false,
            },
            seed_nodes: N3_TESTNET_SEEDS
                .iter()
                .map(|s| {
                    s.parse()
                        .unwrap_or_else(|_| UNKNOWN_PEER_ADDR.parse().expect("value should parse"))
                })
                .collect(),
            port: 20333,
            websocket_enabled: false,
            websocket_port: 20334,
            ..Default::default()
        }
    }

    /// Creates a private network configuration
    pub fn private() -> Self {
        Self {
            magic: 0x12345678, // Custom magic for private network
            listen_address: format!("{}:{}", LOCALHOST, DEFAULT_PRIVNET_PORT)
                .parse()
                .expect("value should parse"),
            p2p_config: P2PConfig {
                listen_address: format!("{}:{}", LOCALHOST, DEFAULT_PRIVNET_PORT)
                    .parse()
                    .expect("value should parse"),
                max_peers: 10,
                connection_timeout: std::time::Duration::from_secs(30),
                handshake_timeout: std::time::Duration::from_secs(10),
                ping_interval: std::time::Duration::from_secs(30),
                message_buffer_size: 1000,
                enable_compression: false,
            },
            seed_nodes: vec![], // No seed nodes for private network
            max_peers: 10,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NetworkError, NetworkStats, PeerInfo};
    use crate::{NetworkConfig, NodeInfo, ProtocolVersion};
    use neo_core::UInt160;

    #[test]
    fn test_protocol_version() {
        let v1 = ProtocolVersion::new(3, 6, 0);
        let v2 = ProtocolVersion::new(3, 5, 0);
        let v3 = ProtocolVersion::new(2, 6, 0);

        assert!(v1.is_compatible(&v2));
        assert!(!v1.is_compatible(&v3));
        assert_eq!(v1.to_string(), "3.6.0");
    }

    #[test]
    fn test_node_info() {
        let node_id = UInt160::zero();
        let info = NodeInfo::new(node_id, 100);

        assert_eq!(info.id, node_id);
        assert_eq!(info.start_height, 100);
        assert_eq!(info.version, ProtocolVersion::current());
        assert!(!info.capabilities.is_empty());
    }

    #[test]
    fn test_network_config() {
        let config = NetworkConfig::default();
        assert_eq!(config.magic, 0x334f454e);
        assert_eq!(config.max_peers, 100);
        assert!(!config.seed_nodes.is_empty());

        let testnet_config = NetworkConfig::testnet();
        assert_eq!(testnet_config.magic, 0x3554334e);

        let private_config = NetworkConfig::private();
        assert_eq!(private_config.magic, 0x12345678);
        assert!(private_config.seed_nodes.is_empty());
    }

    #[test]
    fn test_network_stats() {
        let stats = NetworkStats::default();
        assert_eq!(stats.peer_count, 0);
        assert_eq!(stats.current_height, 0);
        assert_eq!(stats.sync_status, "Disconnected");
    }
}
