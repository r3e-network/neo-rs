// Copyright (C) 2015-2025 The Neo Project.
//
// traits.rs is free software: you can redistribute it and/or modify
// it under the terms of the MIT License.

//! P2P networking traits for Neo N3.
//!
//! These traits define the interface for P2P operations without
//! specifying the implementation details. Implementations can use
//! different async runtimes (tokio, async-std) or actor frameworks.

use crate::{InventoryType, MessageCommand, P2PResult};
use neo_primitives::UInt256;
use std::net::SocketAddr;
use std::time::Duration;

/// Represents a connected peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Remote address of the peer.
    pub address: SocketAddr,
    /// Peer's listening port (if any).
    pub listen_port: Option<u16>,
    /// Peer's protocol version.
    pub version: u32,
    /// Peer's user agent string.
    pub user_agent: String,
    /// Peer's current block height.
    pub height: u32,
    /// Whether this is an inbound connection.
    pub is_inbound: bool,
    /// Connection latency in milliseconds.
    pub latency_ms: Option<u64>,
}

impl PeerInfo {
    /// Creates a new peer info.
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            listen_port: None,
            version: 0,
            user_agent: String::new(),
            height: 0,
            is_inbound: false,
            latency_ms: None,
        }
    }
}

/// Trait for peer management operations.
pub trait PeerManager: Send + Sync {
    /// Returns the number of connected peers.
    fn peer_count(&self) -> usize;

    /// Returns information about all connected peers.
    fn peers(&self) -> Vec<PeerInfo>;

    /// Adds a peer by address.
    fn add_peer(&self, address: SocketAddr) -> P2PResult<()>;

    /// Removes a peer by address.
    fn remove_peer(&self, address: SocketAddr) -> P2PResult<bool>;

    /// Checks if a peer is connected.
    fn is_connected(&self, address: &SocketAddr) -> bool;

    /// Bans a peer for a duration.
    fn ban_peer(&self, address: SocketAddr, duration: Duration) -> P2PResult<()>;

    /// Checks if a peer is banned.
    fn is_banned(&self, address: &SocketAddr) -> bool;
}

/// Trait for broadcasting messages to the network.
pub trait Broadcaster: Send + Sync {
    /// Broadcasts a transaction to all connected peers.
    fn broadcast_transaction(&self, tx_hash: UInt256, tx_data: Vec<u8>) -> P2PResult<()>;

    /// Broadcasts a block to all connected peers.
    fn broadcast_block(&self, block_hash: UInt256, block_data: Vec<u8>) -> P2PResult<()>;

    /// Broadcasts an inventory announcement.
    fn broadcast_inventory(&self, inv_type: InventoryType, hashes: Vec<UInt256>) -> P2PResult<()>;

    /// Sends a message to a specific peer.
    fn send_to_peer(&self, address: SocketAddr, command: MessageCommand, payload: Vec<u8>) -> P2PResult<()>;
}

/// Trait for requesting data from peers.
pub trait DataRequester: Send + Sync {
    /// Requests blocks by hash.
    fn request_blocks(&self, hashes: Vec<UInt256>) -> P2PResult<()>;

    /// Requests headers starting from a hash.
    fn request_headers(&self, start_hash: UInt256) -> P2PResult<()>;

    /// Requests transactions by hash.
    fn request_transactions(&self, hashes: Vec<UInt256>) -> P2PResult<()>;

    /// Requests data by inventory type and hashes.
    fn request_data(&self, inv_type: InventoryType, hashes: Vec<UInt256>) -> P2PResult<()>;
}

/// Events that can be received from the P2P network.
#[derive(Debug, Clone)]
pub enum P2PEvent {
    /// A new peer connected.
    PeerConnected(PeerInfo),
    /// A peer disconnected.
    PeerDisconnected(SocketAddr),
    /// Received a new transaction.
    TransactionReceived {
        hash: UInt256,
        data: Vec<u8>,
        from: SocketAddr,
    },
    /// Received a new block.
    BlockReceived {
        hash: UInt256,
        data: Vec<u8>,
        from: SocketAddr,
    },
    /// Received headers.
    HeadersReceived {
        headers: Vec<Vec<u8>>,
        from: SocketAddr,
    },
    /// Received an inventory announcement.
    InventoryReceived {
        inv_type: InventoryType,
        hashes: Vec<UInt256>,
        from: SocketAddr,
    },
    /// Received a consensus message.
    ConsensusReceived {
        data: Vec<u8>,
        from: SocketAddr,
    },
    /// Received a state root message.
    StateRootReceived {
        data: Vec<u8>,
        from: SocketAddr,
    },
}

/// Trait for subscribing to P2P events.
pub trait P2PEventSubscriber: Send + Sync {
    /// Called when a P2P event occurs.
    fn on_event(&self, event: P2PEvent);
}

/// Configuration for the P2P service.
#[derive(Debug, Clone)]
pub struct P2PConfig {
    /// Local listening address.
    pub listen_address: SocketAddr,
    /// Maximum number of inbound connections.
    pub max_inbound: usize,
    /// Maximum number of outbound connections.
    pub max_outbound: usize,
    /// Seed nodes to connect to.
    pub seed_nodes: Vec<SocketAddr>,
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Handshake timeout.
    pub handshake_timeout: Duration,
    /// Ping interval.
    pub ping_interval: Duration,
    /// Network magic number.
    pub network_magic: u32,
    /// Protocol version.
    pub protocol_version: u32,
    /// User agent string.
    pub user_agent: String,
}

impl Default for P2PConfig {
    fn default() -> Self {
        Self {
            listen_address: "0.0.0.0:10333".parse().unwrap(),
            max_inbound: 10,
            max_outbound: 10,
            seed_nodes: Vec::new(),
            connect_timeout: Duration::from_secs(5),
            handshake_timeout: Duration::from_secs(10),
            ping_interval: Duration::from_secs(30),
            network_magic: 0x4F454E, // "NEO" in hex
            protocol_version: 0,
            user_agent: "/neo-rs:0.7.0/".to_string(),
        }
    }
}

/// Main P2P service trait combining all P2P operations.
pub trait P2PService: PeerManager + Broadcaster + DataRequester + Send + Sync {
    /// Returns the P2P configuration.
    fn config(&self) -> &P2PConfig;

    /// Returns true if the service is running.
    fn is_running(&self) -> bool;

    /// Returns the local node's height.
    fn local_height(&self) -> u32;

    /// Sets the local node's height.
    fn set_local_height(&self, height: u32);

    /// Subscribes to P2P events.
    fn subscribe(&self, subscriber: Box<dyn P2PEventSubscriber>) -> u64;

    /// Unsubscribes from P2P events.
    fn unsubscribe(&self, subscription_id: u64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peer_info_creation() {
        let addr: SocketAddr = "127.0.0.1:10333".parse().unwrap();
        let info = PeerInfo::new(addr);

        assert_eq!(info.address, addr);
        assert!(info.listen_port.is_none());
        assert_eq!(info.version, 0);
        assert!(info.user_agent.is_empty());
    }

    #[test]
    fn test_p2p_config_default() {
        let config = P2PConfig::default();

        assert_eq!(config.max_inbound, 10);
        assert_eq!(config.max_outbound, 10);
        assert!(config.seed_nodes.is_empty());
        assert_eq!(config.connect_timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_p2p_event_variants() {
        let addr: SocketAddr = "127.0.0.1:10333".parse().unwrap();

        let event = P2PEvent::PeerConnected(PeerInfo::new(addr));
        assert!(matches!(event, P2PEvent::PeerConnected(_)));

        let event = P2PEvent::PeerDisconnected(addr);
        assert!(matches!(event, P2PEvent::PeerDisconnected(_)));

        let event = P2PEvent::TransactionReceived {
            hash: UInt256::default(),
            data: vec![],
            from: addr,
        };
        assert!(matches!(event, P2PEvent::TransactionReceived { .. }));
    }
}
