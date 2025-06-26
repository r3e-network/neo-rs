//! P2P networking and communication.
//!
//! This module provides the core P2P networking functionality,
//! exactly matching C# Neo's network structure and organization.
//!
//! The implementation is split into modules that follow C# Neo structure:
//! - config: P2P configuration and settings (matches ProtocolSettings)
//! - events: P2P events and notifications (matches NetworkEventArgs)
//! - connection: Connection management and state (matches Peer and Connection classes)
//! - protocol: Protocol message handlers (matches RemoteNode message handling)
//! - node: Main P2P node implementation (matches LocalNode)
//! - tasks: Background task management (matches TaskManager pattern)

pub mod config;
pub mod connection;
pub mod events;
pub mod protocol;
// pub mod node; // Disabled - using p2p_node.rs instead
pub mod local_test_framework;
pub mod tasks;

// Re-export main types for convenience
pub use config::P2PConfig;
pub use connection::{ConnectionState, PeerConnection};
pub use events::P2PEvent;
pub use protocol::MessageHandler;
// pub use node::P2PNode; // Disabled - using p2p_node.rs instead
pub use local_test_framework::{LocalTestFramework, SyncTestResult, TestNode, TestSyncScenario};

// Main re-exports for P2P functionality

/// P2P system constants (matches C# Neo exactly)
pub const DEFAULT_PORT: u16 = 10333;
pub const MAX_PEERS: usize = 100;
pub const CONNECTION_TIMEOUT_SECS: u64 = 30;
pub const HANDSHAKE_TIMEOUT_SECS: u64 = 10;
pub const PING_INTERVAL_SECS: u64 = 30;
pub const MESSAGE_BUFFER_SIZE: usize = 1000;
