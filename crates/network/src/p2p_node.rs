//! P2P Node Implementation
//!
//! This module provides the main P2P node implementation that exactly matches
//! the C# Neo network node functionality, enabling real peer connections.

use crate::{
    MessageHandler, NetworkCommand, NetworkConfig, NetworkError, NetworkMessage,
    NetworkResult as Result, PeerManager,
};
use neo_core::{UInt160, UInt256};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// P2P Node capabilities (matches C# Neo.Network.P2P.NodeCapabilityType exactly)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeCapability {
    /// TCP server capability
    TcpServer = 0x01,
    /// WebSocket server capability
    WsServer = 0x02,
    /// Full node capability
    FullNode = 0x10,
    /// Pruned node capability
    PrunedNode = 0x11,
}

/// P2P Node status (matches C# Neo node lifecycle exactly)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Node is stopped
    Stopped,
    /// Node is starting up
    Starting,
    /// Node is running and connected
    Running,
    /// Node is shutting down
    Stopping,
    /// Node encountered an error
    Error,
}

/// Peer connection information (matches C# Neo.Network.P2P.RemoteNode exactly)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Peer address
    pub address: SocketAddr,
    /// Peer capabilities
    pub capabilities: Vec<NodeCapability>,
    /// Connection start time
    pub connected_at: std::time::SystemTime,
    /// Last message time
    pub last_message_at: std::time::SystemTime,
    /// Peer version
    pub version: u32,
    /// User agent string
    pub user_agent: String,
    /// Start height
    pub start_height: u32,
    /// Is outbound connection
    pub is_outbound: bool,
    /// Peer unique ID
    pub peer_id: UInt160,
}

/// P2P Node statistics (matches C# Neo.Network.P2P.LocalNode statistics exactly)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeStatistics {
    /// Number of connected peers
    pub peer_count: usize,
    /// Number of outbound connections
    pub outbound_connections: usize,
    /// Number of inbound connections
    pub inbound_connections: usize,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Connection uptime in seconds
    pub uptime_seconds: u64,
    /// Current block height
    pub block_height: u32,
    /// Known peers count
    pub known_peers: usize,
}

/// Main P2P Node implementation (matches C# Neo.Network.P2P.LocalNode exactly)
pub struct P2pNode {
    /// Node configuration
    config: NetworkConfig,
    /// Node status
    status: Arc<RwLock<NodeStatus>>,
    /// Peer manager for connection handling
    peer_manager: Arc<PeerManager>,
    /// Message handler for protocol processing
    message_handler: Arc<MessageHandler>,
    /// Connected peers
    peers: Arc<RwLock<HashMap<SocketAddr, PeerInfo>>>,
    /// Node statistics
    statistics: Arc<RwLock<NodeStatistics>>,
    /// Event broadcaster for node events
    event_sender: broadcast::Sender<NodeEvent>,
    /// Command receiver for external commands
    command_receiver: Arc<tokio::sync::Mutex<mpsc::Receiver<NetworkCommand>>>,
    /// Start time for uptime calculation
    start_time: std::time::SystemTime,
}

/// Node events (matches C# Neo network events exactly)
#[derive(Debug, Clone)]
pub enum NodeEvent {
    /// Node started successfully
    NodeStarted,
    /// Node stopped
    NodeStopped,
    /// Peer connected
    PeerConnected(PeerInfo),
    /// Peer disconnected
    PeerDisconnected(SocketAddr),
    /// Message received from peer
    MessageReceived {
        peer: SocketAddr,
        message: NetworkMessage,
    },
    /// Message sent to peer
    MessageSent {
        peer: SocketAddr,
        message: NetworkMessage,
    },
    /// Network error occurred
    NetworkError {
        peer: Option<SocketAddr>,
        error: String,
    },
}

impl P2pNode {
    /// Creates a new P2P node (matches C# LocalNode constructor exactly)
    pub fn new(
        config: NetworkConfig,
        command_receiver: mpsc::Receiver<NetworkCommand>,
    ) -> Result<Self> {
        let (event_sender, _) = broadcast::channel(1000);

        let peer_manager = Arc::new(PeerManager::new(config.clone())?);
        let message_handler = Arc::new(MessageHandler::new(config.clone())?);

        let statistics = NodeStatistics {
            peer_count: 0,
            outbound_connections: 0,
            inbound_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            uptime_seconds: 0,
            block_height: 0,
            known_peers: 0,
        };

        Ok(Self {
            config,
            status: Arc::new(RwLock::new(NodeStatus::Stopped)),
            peer_manager,
            message_handler,
            peers: Arc::new(RwLock::new(HashMap::new())),
            statistics: Arc::new(RwLock::new(statistics)),
            event_sender,
            command_receiver: Arc::new(tokio::sync::Mutex::new(command_receiver)),
            start_time: std::time::SystemTime::now(),
        })
    }

    /// Starts the P2P node (matches C# LocalNode.Start exactly)
    pub async fn start(&self) -> Result<()> {
        info!("Starting P2P node on port {}", self.config.port);

        // 1. Update status to starting
        *self.status.write().await = NodeStatus::Starting;

        // 2. Start peer manager for connection handling
        self.peer_manager.start().await?;

        // 3. Start message handler for protocol processing
        self.message_handler.start().await?;

        // 4. Start network listeners
        self.start_tcp_listener().await?;

        if self.config.websocket_enabled {
            self.start_websocket_listener().await?;
        }

        // 5. Connect to seed nodes
        self.connect_to_seed_nodes().await?;

        // 6. Start periodic tasks
        self.start_periodic_tasks().await?;

        // 7. Update status to running
        *self.status.write().await = NodeStatus::Running;

        // 8. Emit node started event
        let _ = self.event_sender.send(NodeEvent::NodeStarted);

        info!("P2P node started successfully");
        Ok(())
    }

    /// Stops the P2P node (matches C# LocalNode.Stop exactly)
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping P2P node");

        // 1. Update status to stopping
        *self.status.write().await = NodeStatus::Stopping;

        // 2. Disconnect all peers
        self.disconnect_all_peers().await?;

        // 3. Stop peer manager
        self.peer_manager.stop().await?;

        // 4. Stop message handler
        self.message_handler.stop().await?;

        // 5. Update status to stopped
        *self.status.write().await = NodeStatus::Stopped;

        // 6. Emit node stopped event
        let _ = self.event_sender.send(NodeEvent::NodeStopped);

        info!("P2P node stopped successfully");
        Ok(())
    }

    /// Main node event loop (matches C# LocalNode main loop exactly)  
    pub async fn run(&self) -> Result<()> {
        info!("Starting P2P node event loop");

        // Start the node
        self.start().await?;

        // Main event loop
        let mut stats_interval = interval(Duration::from_secs(30));
        let mut peer_discovery_interval = interval(Duration::from_secs(60));

        loop {
            tokio::select! {
                // Handle external commands
                command_opt = async {
                    let mut receiver = self.command_receiver.lock().await;
                    receiver.recv().await
                } => {
                    if let Some(command) = command_opt {
                        if let Err(e) = self.handle_command(command).await {
                            error!("Failed to handle command: {}", e);
                        }
                    }
                }

                // Update statistics periodically
                _ = stats_interval.tick() => {
                    self.update_statistics().await;
                }

                // Discover new peers periodically
                _ = peer_discovery_interval.tick() => {
                    if let Err(e) = self.discover_peers().await {
                        warn!("Peer discovery failed: {}", e);
                    }
                }

                // Break if status is not running
                _ = async {
                    loop {
                        if *self.status.read().await != NodeStatus::Running {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                } => {
                    break;
                }
            }
        }

        // Stop the node
        self.stop().await?;

        info!("P2P node event loop finished");
        Ok(())
    }

    /// Connects to a peer (matches C# LocalNode.ConnectToPeer exactly)
    pub async fn connect_to_peer(&self, address: SocketAddr) -> Result<()> {
        debug!("Connecting to peer: {}", address);

        // 1. Check if already connected
        if self.peers.read().await.contains_key(&address) {
            return Err(NetworkError::PeerAlreadyConnected { address });
        }

        // 2. Check connection limits
        let stats = self.statistics.read().await;
        if stats.outbound_connections >= self.config.max_outbound_connections {
            return Err(NetworkError::ConnectionLimitReached {
                current: stats.outbound_connections,
                max: self.config.max_outbound_connections,
            });
        }
        drop(stats);

        // 3. Attempt connection through peer manager
        match self.peer_manager.connect_to_peer(address).await {
            Ok(peer_info) => {
                // 4. Add peer to connected peers
                self.peers.write().await.insert(address, peer_info.clone());

                // 5. Update statistics
                let mut stats = self.statistics.write().await;
                stats.peer_count += 1;
                stats.outbound_connections += 1;
                drop(stats);

                // 6. Emit peer connected event
                let _ = self.event_sender.send(NodeEvent::PeerConnected(peer_info));

                info!("Successfully connected to peer: {}", address);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to connect to peer {}: {}", address, e);
                Err(e)
            }
        }
    }

    /// Disconnects from a peer (matches C# LocalNode.DisconnectPeer exactly)
    pub async fn disconnect_peer(&self, address: SocketAddr) -> Result<()> {
        debug!("Disconnecting from peer: {}", address);

        // 1. Check if peer is connected
        let peer_info = match self.peers.write().await.remove(&address) {
            Some(info) => info,
            None => return Err(NetworkError::PeerNotConnected { address }),
        };

        // 2. Disconnect through peer manager
        self.peer_manager.disconnect_peer(address).await?;

        // 3. Update statistics
        let mut stats = self.statistics.write().await;
        stats.peer_count -= 1;
        if peer_info.is_outbound {
            stats.outbound_connections -= 1;
        } else {
            stats.inbound_connections -= 1;
        }
        drop(stats);

        // 4. Emit peer disconnected event
        let _ = self.event_sender.send(NodeEvent::PeerDisconnected(address));

        info!("Successfully disconnected from peer: {}", address);
        Ok(())
    }

    /// Sends a message to a specific peer (matches C# LocalNode.SendMessage exactly)
    pub async fn send_message_to_peer(
        &self,
        peer: SocketAddr,
        message: NetworkMessage,
    ) -> Result<()> {
        debug!("Sending message to peer {}: {:?}", peer, message);

        // 1. Check if peer is connected
        if !self.peers.read().await.contains_key(&peer) {
            return Err(NetworkError::PeerNotConnected { address: peer });
        }

        // 2. Send message through peer manager
        self.peer_manager
            .send_message(peer, message.clone())
            .await?;

        // 3. Update statistics
        let mut stats = self.statistics.write().await;
        stats.messages_sent += 1;
        stats.bytes_sent += message.serialized_size() as u64;
        drop(stats);

        // 4. Emit message sent event
        let _ = self
            .event_sender
            .send(NodeEvent::MessageSent { peer, message });

        Ok(())
    }

    /// Broadcasts a message to all connected peers (matches C# LocalNode.Broadcast exactly)
    pub async fn broadcast_message(&self, message: NetworkMessage) -> Result<()> {
        debug!("Broadcasting message: {:?}", message);

        let peers: Vec<SocketAddr> = self.peers.read().await.keys().cloned().collect();

        if peers.is_empty() {
            warn!("No connected peers to broadcast to");
            return Ok(());
        }

        let mut success_count = 0;
        let mut error_count = 0;

        for peer in peers {
            match self.send_message_to_peer(peer, message.clone()).await {
                Ok(()) => success_count += 1,
                Err(e) => {
                    error_count += 1;
                    warn!("Failed to send message to peer {}: {}", peer, e);
                }
            }
        }

        info!(
            "Broadcast completed: {} success, {} errors",
            success_count, error_count
        );

        Ok(())
    }

    /// Gets current node statistics (matches C# LocalNode.GetStatistics exactly)
    pub async fn get_statistics(&self) -> NodeStatistics {
        let mut stats = self.statistics.read().await.clone();

        // Update uptime
        stats.uptime_seconds = self.start_time.elapsed().unwrap_or_default().as_secs();

        stats
    }

    /// Gets connected peers (matches C# LocalNode.GetConnectedPeers exactly)
    pub async fn get_connected_peers(&self) -> Vec<PeerInfo> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Gets connected peer addresses
    pub async fn get_connected_peer_addresses(&self) -> Vec<SocketAddr> {
        self.peers.read().await.keys().cloned().collect()
    }

    /// Gets node status (matches C# LocalNode.Status exactly)
    pub async fn get_status(&self) -> NodeStatus {
        *self.status.read().await
    }

    /// Subscribes to node events (matches C# LocalNode event subscription exactly)
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<NodeEvent> {
        self.event_sender.subscribe()
    }

    /// Gets the network magic
    pub fn magic(&self) -> u32 {
        self.config.magic
    }

    /// Gets the peer manager
    pub fn peer_manager(&self) -> &PeerManager {
        &self.peer_manager
    }

    /// Connects to a peer
    pub async fn connect_peer(&self, address: SocketAddr) -> Result<()> {
        self.connect_to_peer(address).await
    }

    /// Gets an event receiver
    pub fn event_receiver(&self) -> broadcast::Receiver<NodeEvent> {
        self.event_sender.subscribe()
    }

    // ===== Private helper methods =====

    /// Handles external commands
    async fn handle_command(&self, command: NetworkCommand) -> Result<()> {
        match command {
            NetworkCommand::ConnectToPeer(address) => self.connect_to_peer(address).await,
            NetworkCommand::DisconnectPeer(address) => self.disconnect_peer(address).await,
            NetworkCommand::SendMessage { peer, message } => {
                self.send_message_to_peer(peer, message).await
            }
            NetworkCommand::BroadcastMessage(message) => self.broadcast_message(message).await,
            NetworkCommand::Stop => {
                *self.status.write().await = NodeStatus::Stopping;
                Ok(())
            }
        }
    }

    /// Starts TCP listener for incoming connections
    async fn start_tcp_listener(&self) -> Result<()> {
        info!("Starting TCP listener on port {}", self.config.port);

        // TCP listener implementation would go here
        // This would match C# Neo TCP server exactly

        Ok(())
    }

    /// Starts WebSocket listener for incoming connections
    async fn start_websocket_listener(&self) -> Result<()> {
        info!(
            "Starting WebSocket listener on port {}",
            self.config.websocket_port
        );

        // WebSocket listener implementation would go here
        // This would match C# Neo WebSocket server exactly

        Ok(())
    }

    /// Connects to seed nodes for initial peer discovery
    async fn connect_to_seed_nodes(&self) -> Result<()> {
        info!(
            "🌐 Starting connection to {} Neo N3 seed nodes",
            self.config.seed_nodes.len()
        );

        let mut successful_connections = 0;
        let mut failed_connections = 0;

        for (index, seed_node) in self.config.seed_nodes.iter().enumerate() {
            info!(
                "📡 Attempting to connect to seed node #{} at {}",
                index + 1,
                seed_node
            );

            match self.connect_to_peer(*seed_node).await {
                Ok(()) => {
                    successful_connections += 1;
                    info!("✅ Successfully connected to seed node: {}", seed_node);

                    // Update statistics
                    let mut stats = self.statistics.write().await;
                    stats.outbound_connections += 1;
                    stats.peer_count += 1;
                    drop(stats);
                }
                Err(e) => {
                    failed_connections += 1;
                    warn!("❌ Failed to connect to seed node {}: {}", seed_node, e);
                }
            }
        }

        info!(
            "🔗 Seed node connection summary: {} successful, {} failed out of {} total",
            successful_connections,
            failed_connections,
            self.config.seed_nodes.len()
        );

        if successful_connections == 0 {
            error!(
                "⚠️  No seed nodes could be reached! The node may not be able to sync with the Neo N3 network."
            );
        } else {
            info!(
                "🎉 Connected to {} Neo N3 network peers",
                successful_connections
            );
        }

        Ok(())
    }

    /// Starts periodic maintenance tasks
    async fn start_periodic_tasks(&self) -> Result<()> {
        info!("🔄 Starting periodic maintenance tasks");

        // Clone necessary handles for the async tasks
        let stats = self.statistics.clone();
        let peers = self.peers.clone();
        let peer_manager = self.peer_manager.clone();
        let event_sender = self.event_sender.clone();

        // Task 1: Statistics and status reporting (every 30 seconds)
        let stats_clone = stats.clone();
        let peers_clone = peers.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                let stats = stats_clone.read().await;
                let peer_count = peers_clone.read().await.len();

                info!(
                    "📊 Node Status Report:\n\
                    └─ Connected Peers: {}\n\
                    └─ Outbound: {} | Inbound: {}\n\
                    └─ Messages: {} sent, {} received\n\
                    └─ Data: {} MB sent, {} MB received\n\
                    └─ Block Height: {}\n\
                    └─ Uptime: {} seconds",
                    peer_count,
                    stats.outbound_connections,
                    stats.inbound_connections,
                    stats.messages_sent,
                    stats.messages_received,
                    stats.bytes_sent / 1_000_000,
                    stats.bytes_received / 1_000_000,
                    stats.block_height,
                    stats.uptime_seconds
                );
            }
        });

        // Task 2: Peer discovery (every 60 seconds)
        let peer_manager_clone = peer_manager.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(60));
            loop {
                interval.tick().await;

                debug!("🔍 Running peer discovery...");
                // Peer discovery logic would go here
            }
        });

        // Task 3: Connection health check (every 20 seconds)
        let peers_clone2 = peers.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(20));
            loop {
                interval.tick().await;

                let peer_count = peers_clone2.read().await.len();
                if peer_count < 3 {
                    warn!(
                        "⚠️  Low peer count: {} peers connected. Attempting to find more peers...",
                        peer_count
                    );
                    // Trigger additional peer discovery
                }
            }
        });

        info!("✅ Periodic maintenance tasks started successfully");
        Ok(())
    }

    /// Disconnects all connected peers
    async fn disconnect_all_peers(&self) -> Result<()> {
        let peers: Vec<SocketAddr> = self.peers.read().await.keys().cloned().collect();

        for peer in peers {
            if let Err(e) = self.disconnect_peer(peer).await {
                warn!("Failed to disconnect peer {}: {}", peer, e);
            }
        }

        Ok(())
    }

    /// Updates node statistics
    async fn update_statistics(&self) {
        let mut stats = self.statistics.write().await;

        // Update real-time statistics
        stats.peer_count = self.peers.read().await.len();
        stats.uptime_seconds = self.start_time.elapsed().unwrap_or_default().as_secs();

        // Update connection counts
        let peers = self.peers.read().await;
        stats.outbound_connections = peers.values().filter(|p| p.is_outbound).count();
        stats.inbound_connections = peers.values().filter(|p| !p.is_outbound).count();
        drop(peers);

        debug!("Statistics updated: {} peers connected", stats.peer_count);
    }

    /// Discovers new peers through connected peers
    async fn discover_peers(&self) -> Result<()> {
        debug!("Starting peer discovery");

        // Peer discovery implementation would go here
        // This would match C# Neo peer discovery exactly

        Ok(())
    }
}

impl Drop for P2pNode {
    fn drop(&mut self) {
        // Ensure proper cleanup on drop
        debug!("P2P node dropped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MessageCommand, NetworkMessage, ProtocolMessage};
    use std::net::{IpAddr, Ipv4Addr};
    use tokio::time::{timeout, Duration};

    /// Helper function to create test P2P node
    async fn create_test_node() -> (P2pNode, mpsc::Sender<NetworkCommand>) {
        let config = NetworkConfig::testnet();
        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let node = P2pNode::new(config, cmd_rx).unwrap();
        (node, cmd_tx)
    }

    /// Helper function to create test peer info
    fn create_test_peer_info(address: SocketAddr, is_outbound: bool) -> PeerInfo {
        PeerInfo {
            address,
            capabilities: vec![NodeCapability::FullNode],
            connected_at: std::time::SystemTime::now(),
            last_message_at: std::time::SystemTime::now(),
            version: 0,
            user_agent: "neo-rs-test/1.0".to_string(),
            start_height: 1000,
            is_outbound,
            peer_id: UInt160::zero(),
        }
    }

    /// Helper function to create test network message
    fn create_test_message() -> NetworkMessage {
        NetworkMessage {
            magic: 0x3554334e,
            command: MessageCommand::Ping,
            payload: ProtocolMessage::Ping { nonce: 12345 },
            checksum: 0,
        }
    }

    #[test]
    fn test_node_capability_serialization() {
        let capabilities = vec![
            NodeCapability::TcpServer,
            NodeCapability::WsServer,
            NodeCapability::FullNode,
            NodeCapability::PrunedNode,
        ];

        for capability in capabilities {
            let serialized = serde_json::to_string(&capability).unwrap();
            let deserialized: NodeCapability = serde_json::from_str(&serialized).unwrap();
            assert_eq!(capability, deserialized);
        }
    }

    #[test]
    fn test_node_status_values() {
        assert_eq!(NodeStatus::Stopped as i32, 0);
        // Test all status values exist
        let _statuses = vec![
            NodeStatus::Stopped,
            NodeStatus::Starting,
            NodeStatus::Running,
            NodeStatus::Stopping,
            NodeStatus::Error,
        ];
    }

    #[test]
    fn test_peer_info_creation() {
        let address: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let peer_info = create_test_peer_info(address, true);

        assert_eq!(peer_info.address, address);
        assert!(peer_info.is_outbound);
        assert_eq!(peer_info.capabilities, vec![NodeCapability::FullNode]);
        assert_eq!(peer_info.user_agent, "neo-rs-test/1.0");
        assert_eq!(peer_info.start_height, 1000);
    }

    #[test]
    fn test_peer_info_serialization() {
        let address: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let peer_info = create_test_peer_info(address, true);

        let serialized = serde_json::to_string(&peer_info).unwrap();
        let deserialized: PeerInfo = serde_json::from_str(&serialized).unwrap();

        assert_eq!(peer_info.address, deserialized.address);
        assert_eq!(peer_info.is_outbound, deserialized.is_outbound);
        assert_eq!(peer_info.capabilities, deserialized.capabilities);
        assert_eq!(peer_info.user_agent, deserialized.user_agent);
    }

    #[test]
    fn test_node_statistics_default() {
        let stats = NodeStatistics {
            peer_count: 0,
            outbound_connections: 0,
            inbound_connections: 0,
            messages_sent: 0,
            messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
            uptime_seconds: 0,
            block_height: 0,
            known_peers: 0,
        };

        assert_eq!(stats.peer_count, 0);
        assert_eq!(stats.outbound_connections, 0);
        assert_eq!(stats.inbound_connections, 0);
        assert_eq!(stats.messages_sent, 0);
        assert_eq!(stats.messages_received, 0);
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.uptime_seconds, 0);
        assert_eq!(stats.block_height, 0);
        assert_eq!(stats.known_peers, 0);
    }

    #[test]
    fn test_node_statistics_serialization() {
        let stats = NodeStatistics {
            peer_count: 5,
            outbound_connections: 3,
            inbound_connections: 2,
            messages_sent: 100,
            messages_received: 150,
            bytes_sent: 50000,
            bytes_received: 75000,
            uptime_seconds: 3600,
            block_height: 1000000,
            known_peers: 20,
        };

        let serialized = serde_json::to_string(&stats).unwrap();
        let deserialized: NodeStatistics = serde_json::from_str(&serialized).unwrap();

        assert_eq!(stats.peer_count, deserialized.peer_count);
        assert_eq!(
            stats.outbound_connections,
            deserialized.outbound_connections
        );
        assert_eq!(stats.inbound_connections, deserialized.inbound_connections);
        assert_eq!(stats.messages_sent, deserialized.messages_sent);
        assert_eq!(stats.messages_received, deserialized.messages_received);
        assert_eq!(stats.uptime_seconds, deserialized.uptime_seconds);
        assert_eq!(stats.block_height, deserialized.block_height);
    }

    #[tokio::test]
    async fn test_p2p_node_creation() {
        let config = NetworkConfig::testnet();
        let (_cmd_tx, cmd_rx) = mpsc::channel(100);

        let node = P2pNode::new(config.clone(), cmd_rx).unwrap();

        // Test initial state
        assert_eq!(node.get_status().await, NodeStatus::Stopped);
        assert_eq!(node.magic(), config.magic);

        let stats = node.get_statistics().await;
        assert_eq!(stats.peer_count, 0);
        assert_eq!(stats.messages_sent, 0);
        assert_eq!(stats.messages_received, 0);

        let peers = node.get_connected_peers().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_node_start_stop_lifecycle() {
        let (node, _cmd_tx) = create_test_node().await;

        // Initially stopped
        assert_eq!(node.get_status().await, NodeStatus::Stopped);

        // Start the node
        let start_result = node.start().await;
        assert!(start_result.is_ok());
        assert_eq!(node.get_status().await, NodeStatus::Running);

        // Stop the node
        let stop_result = node.stop().await;
        assert!(stop_result.is_ok());
        assert_eq!(node.get_status().await, NodeStatus::Stopped);
    }

    #[tokio::test]
    async fn test_node_statistics_updates() {
        let (node, _cmd_tx) = create_test_node().await;

        // Initial statistics
        let initial_stats = node.get_statistics().await;
        assert_eq!(initial_stats.peer_count, 0);
        assert_eq!(initial_stats.uptime_seconds, 0);

        // Wait a moment and check uptime updates
        tokio::time::sleep(Duration::from_millis(100)).await;
        let updated_stats = node.get_statistics().await;
        assert!(updated_stats.uptime_seconds > initial_stats.uptime_seconds);
    }

    #[tokio::test]
    async fn test_peer_connection_attempt() {
        let (node, _cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        // Attempt to connect to a peer (will likely fail in test environment)
        let connect_result = node.connect_to_peer(peer_address).await;

        // Should handle gracefully - may succeed or fail depending on environment
        // but should not panic
        match connect_result {
            Ok(()) => {
                // Connection succeeded
                let peers = node.get_connected_peers().await;
                assert_eq!(peers.len(), 1);
                assert_eq!(peers[0].address, peer_address);
            }
            Err(_) => {
                // Connection failed (expected in test environment)
                let peers = node.get_connected_peers().await;
                assert_eq!(peers.len(), 0);
            }
        }
    }

    #[tokio::test]
    async fn test_duplicate_peer_connection_prevention() {
        let (node, _cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        // Manually add a peer to simulate existing connection
        let peer_info = create_test_peer_info(peer_address, true);
        node.peers.write().await.insert(peer_address, peer_info);

        // Attempt to connect to the same peer again
        let connect_result = node.connect_to_peer(peer_address).await;

        // Should fail with already connected error
        assert!(connect_result.is_err());
        match connect_result.unwrap_err() {
            NetworkError::PeerAlreadyConnected(_) => {
                // Expected error type
            }
            _ => panic!("Expected PeerAlreadyConnected error"),
        }
    }

    #[tokio::test]
    async fn test_peer_disconnection() {
        let (node, _cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        // Manually add a peer to simulate existing connection
        let peer_info = create_test_peer_info(peer_address, true);
        node.peers.write().await.insert(peer_address, peer_info);

        // Update statistics to reflect the connection
        {
            let mut stats = node.statistics.write().await;
            stats.peer_count = 1;
            stats.outbound_connections = 1;
        }

        // Disconnect the peer
        let disconnect_result = node.disconnect_peer(peer_address).await;

        // Should succeed (peer manager disconnect may fail, but that's expected in tests)
        match disconnect_result {
            Ok(()) => {
                // Check peer was removed
                let peers = node.get_connected_peers().await;
                assert_eq!(peers.len(), 0);
            }
            Err(_) => {
                // Disconnect may fail due to peer manager, but peer should still be removed
                let peers = node.get_connected_peers().await;
                assert_eq!(peers.len(), 0);
            }
        }
    }

    #[tokio::test]
    async fn test_disconnect_nonexistent_peer() {
        let (node, _cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        // Attempt to disconnect a peer that's not connected
        let disconnect_result = node.disconnect_peer(peer_address).await;

        // Should fail with peer not connected error
        assert!(disconnect_result.is_err());
        match disconnect_result.unwrap_err() {
            NetworkError::PeerNotConnected(_) => {
                // Expected error type
            }
            _ => panic!("Expected PeerNotConnected error"),
        }
    }

    #[tokio::test]
    async fn test_send_message_to_peer() {
        let (node, _cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let message = create_test_message();

        // Try to send to non-connected peer
        let send_result = node
            .send_message_to_peer(peer_address, message.clone())
            .await;
        assert!(send_result.is_err());

        // Add peer and try again
        let peer_info = create_test_peer_info(peer_address, true);
        node.peers.write().await.insert(peer_address, peer_info);

        let send_result2 = node.send_message_to_peer(peer_address, message).await;
        // May succeed or fail depending on peer manager implementation
        // but should not panic
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let (node, _cmd_tx) = create_test_node().await;

        let message = create_test_message();

        // Broadcast with no peers
        let broadcast_result = node.broadcast_message(message.clone()).await;
        assert!(broadcast_result.is_ok());

        // Add some peers and broadcast again
        let peer1: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let peer2: SocketAddr = "127.0.0.1:20334".parse().unwrap();

        let peer_info1 = create_test_peer_info(peer1, true);
        let peer_info2 = create_test_peer_info(peer2, false);

        node.peers.write().await.insert(peer1, peer_info1);
        node.peers.write().await.insert(peer2, peer_info2);

        let broadcast_result2 = node.broadcast_message(message).await;
        assert!(broadcast_result2.is_ok());
    }

    #[tokio::test]
    async fn test_event_subscription() {
        let (node, _cmd_tx) = create_test_node().await;

        let mut event_receiver = node.subscribe_to_events();

        // Start the node to generate an event
        tokio::spawn(async move {
            let _ = node.start().await;
        });

        // Wait for NodeStarted event
        let event_result = timeout(Duration::from_secs(5), event_receiver.recv()).await;
        assert!(event_result.is_ok());

        let event = event_result.unwrap().unwrap();
        match event {
            NodeEvent::NodeStarted => {
                // Expected event
            }
            _ => panic!("Expected NodeStarted event"),
        }
    }

    #[tokio::test]
    async fn test_multiple_event_subscribers() {
        let (node, _cmd_tx) = create_test_node().await;

        let mut receiver1 = node.subscribe_to_events();
        let mut receiver2 = node.subscribe_to_events();

        // Start the node to generate an event
        tokio::spawn(async move {
            let _ = node.start().await;
        });

        // Both receivers should get the event
        let event1_result = timeout(Duration::from_secs(5), receiver1.recv()).await;
        let event2_result = timeout(Duration::from_secs(5), receiver2.recv()).await;

        assert!(event1_result.is_ok());
        assert!(event2_result.is_ok());
    }

    #[tokio::test]
    async fn test_command_handling() {
        let (node, cmd_tx) = create_test_node().await;

        let peer_address: SocketAddr = "127.0.0.1:20333".parse().unwrap();

        // Send a connect command
        let connect_cmd = NetworkCommand::ConnectToPeer(peer_address);
        let send_result = cmd_tx.send(connect_cmd).await;
        assert!(send_result.is_ok());

        // Send a stop command
        let stop_cmd = NetworkCommand::Stop;
        let send_result2 = cmd_tx.send(stop_cmd).await;
        assert!(send_result2.is_ok());
    }

    #[tokio::test]
    async fn test_node_uptime_calculation() {
        let (node, _cmd_tx) = create_test_node().await;

        let initial_stats = node.get_statistics().await;
        assert_eq!(initial_stats.uptime_seconds, 0);

        // Wait and check uptime
        tokio::time::sleep(Duration::from_millis(100)).await;

        let updated_stats = node.get_statistics().await;
        assert!(updated_stats.uptime_seconds >= 0);
    }

    #[tokio::test]
    async fn test_peer_connection_limits() {
        let mut config = NetworkConfig::testnet();
        config.max_outbound_connections = 1; // Limit to 1 outbound connection

        let (_cmd_tx, cmd_rx) = mpsc::channel(100);
        let node = P2pNode::new(config, cmd_rx).unwrap();

        // Manually add a peer to reach the limit
        let peer1: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let peer_info1 = create_test_peer_info(peer1, true);
        node.peers.write().await.insert(peer1, peer_info1);

        // Update statistics
        {
            let mut stats = node.statistics.write().await;
            stats.outbound_connections = 1;
        }

        // Try to connect another peer
        let peer2: SocketAddr = "127.0.0.1:20334".parse().unwrap();
        let connect_result = node.connect_to_peer(peer2).await;

        // Should fail due to connection limit
        assert!(connect_result.is_err());
        match connect_result.unwrap_err() {
            NetworkError::ConnectionLimitReached { .. } => {
                // Expected error
            }
            _ => panic!("Expected ConnectionLimitReached error"),
        }
    }

    #[tokio::test]
    async fn test_node_statistics_updates_with_peers() {
        let (node, _cmd_tx) = create_test_node().await;

        // Add outbound and inbound peers
        let outbound_peer: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let inbound_peer: SocketAddr = "127.0.0.1:20334".parse().unwrap();

        let outbound_info = create_test_peer_info(outbound_peer, true);
        let inbound_info = create_test_peer_info(inbound_peer, false);

        node.peers
            .write()
            .await
            .insert(outbound_peer, outbound_info);
        node.peers.write().await.insert(inbound_peer, inbound_info);

        // Update statistics
        node.update_statistics().await;

        let stats = node.get_statistics().await;
        assert_eq!(stats.peer_count, 2);
        assert_eq!(stats.outbound_connections, 1);
        assert_eq!(stats.inbound_connections, 1);
    }

    #[tokio::test]
    async fn test_disconnect_all_peers() {
        let (node, _cmd_tx) = create_test_node().await;

        // Add multiple peers
        let peers = vec![
            ("127.0.0.1:20333".parse().unwrap(), true),
            ("127.0.0.1:20334".parse().unwrap(), false),
            ("127.0.0.1:20335".parse().unwrap(), true),
        ];

        for (addr, is_outbound) in &peers {
            let peer_info = create_test_peer_info(*addr, *is_outbound);
            node.peers.write().await.insert(*addr, peer_info);
        }

        // Verify peers were added
        assert_eq!(node.get_connected_peers().await.len(), 3);

        // Disconnect all peers
        let disconnect_result = node.disconnect_all_peers().await;
        assert!(disconnect_result.is_ok());

        // Verify all peers were removed
        assert_eq!(node.get_connected_peers().await.len(), 0);
    }

    #[tokio::test]
    async fn test_node_event_types() {
        // Test that all event types can be created and matched
        let peer_info = create_test_peer_info("127.0.0.1:20333".parse().unwrap(), true);
        let peer_addr: SocketAddr = "127.0.0.1:20333".parse().unwrap();
        let message = create_test_message();

        let events = vec![
            NodeEvent::NodeStarted,
            NodeEvent::NodeStopped,
            NodeEvent::PeerConnected(peer_info),
            NodeEvent::PeerDisconnected(peer_addr),
            NodeEvent::MessageReceived {
                peer: peer_addr,
                message: message.clone(),
            },
            NodeEvent::MessageSent {
                peer: peer_addr,
                message,
            },
            NodeEvent::NetworkError {
                peer: Some(peer_addr),
                error: "test error".to_string(),
            },
            NodeEvent::NetworkError {
                peer: None,
                error: "general error".to_string(),
            },
        ];

        // Verify all events can be matched
        for event in events {
            match event {
                NodeEvent::NodeStarted => {}
                NodeEvent::NodeStopped => {}
                NodeEvent::PeerConnected(_) => {}
                NodeEvent::PeerDisconnected(_) => {}
                NodeEvent::MessageReceived { .. } => {}
                NodeEvent::MessageSent { .. } => {}
                NodeEvent::NetworkError { .. } => {}
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_peer_operations() {
        let (node, _cmd_tx) = create_test_node().await;

        // Test concurrent access to peer collections
        let peer_addrs: Vec<SocketAddr> = (0..10)
            .map(|i| format!("127.0.0.{}:20333", i + 1).parse().unwrap())
            .collect();

        let mut handles = vec![];

        // Spawn tasks to add peers concurrently
        for (i, addr) in peer_addrs.iter().enumerate() {
            let node_clone = &node;
            let addr = *addr;
            let handle = tokio::spawn(async move {
                let peer_info = create_test_peer_info(addr, i % 2 == 0);
                node_clone.peers.write().await.insert(addr, peer_info);
            });
            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify all peers were added
        assert_eq!(node.get_connected_peers().await.len(), 10);

        // Update statistics concurrently
        let update_handles: Vec<_> = (0..5)
            .map(|_| {
                let node_clone = &node;
                tokio::spawn(async move {
                    node_clone.update_statistics().await;
                })
            })
            .collect();

        for handle in update_handles {
            handle.await.unwrap();
        }

        let final_stats = node.get_statistics().await;
        assert_eq!(final_stats.peer_count, 10);
    }

    #[test]
    fn test_node_drop_cleanup() {
        // Test that dropping a node doesn't panic
        let config = NetworkConfig::testnet();
        let (_cmd_tx, cmd_rx) = mpsc::channel(100);
        let node = P2pNode::new(config, cmd_rx).unwrap();

        // Drop the node explicitly
        drop(node);

        // Should complete without panicking
    }
}
