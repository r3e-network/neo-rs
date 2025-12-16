//! P2P Service for neo-node runtime
//!
//! This module provides a high-level P2P service that wraps neo-core's
//! P2P implementation and integrates with the node runtime via tokio channels.
//!
//! ## Protocol Implementation
//!
//! The service implements the full Neo N3 P2P protocol:
//! - Version/Verack handshake
//! - Ping/Pong heartbeat
//! - Block/Header synchronization
//! - Transaction relay

use neo_core::neo_io::SerializableExt;
use neo_core::network::p2p::{
    capabilities::NodeCapability,
    payloads::{GetBlockByIndexPayload, InvPayload, InventoryType, PingPayload, VersionPayload},
    ChannelsConfig, NetworkMessage, PeerConnection, ProtocolMessage,
};
use neo_core::UInt256;
use neo_p2p::{P2PConfig, P2PEvent, PeerInfo};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Maximum blocks to request in a single batch
const BLOCK_BATCH_SIZE: usize = 500;
/// Maximum pending block requests before pausing header sync
#[allow(dead_code)] // Will be used when full sync throttling is implemented
const MAX_PENDING_BLOCKS: usize = 2000;

/// P2P service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P2PServiceState {
    /// Service not started
    Stopped,
    /// Service starting up
    Starting,
    /// Service running
    Running,
    /// Service shutting down
    Stopping,
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// TCP connection established
    Connected,
    /// Performing protocol handshake
    Handshaking,
    /// Fully connected and ready
    Ready,
    /// Connection closed
    Disconnected,
}

/// Connected peer information
#[derive(Debug, Clone)]
pub struct ConnectedPeer {
    /// Peer address
    pub address: SocketAddr,
    /// Connection state
    pub state: ConnectionState,
    /// Peer's block height
    pub height: u32,
    /// User agent
    pub user_agent: String,
    /// Is inbound connection
    pub is_inbound: bool,
    /// Last activity timestamp
    pub last_activity: u64,
}

/// Message to broadcast to all peers
#[derive(Debug, Clone)]
pub struct BroadcastMessage {
    /// The protocol message to broadcast
    pub message: Vec<u8>,
    /// Message category (e.g., "dBFT" for consensus)
    pub category: String,
}

/// P2P Service managing peer connections
pub struct P2PService {
    /// Configuration
    config: P2PConfig,
    /// Service state
    state: Arc<RwLock<P2PServiceState>>,
    /// Connected peers
    peers: Arc<RwLock<HashMap<SocketAddr, ConnectedPeer>>>,
    /// Event sender
    event_tx: mpsc::Sender<P2PEvent>,
    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
    /// Local node height
    local_height: Arc<RwLock<u32>>,
    /// Pending block hashes to request (queue for continuous sync)
    pending_block_hashes: Arc<RwLock<VecDeque<UInt256>>>,
    /// Last header index received (for continuous header sync)
    last_header_index: Arc<RwLock<u32>>,
    /// Best known peer height
    best_peer_height: Arc<RwLock<u32>>,
    /// Broadcast channel for sending messages to all peers
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    /// Chain state for responding to GetHeaders requests
    chain: Option<Arc<RwLock<neo_chain::ChainState>>>,
}

impl P2PService {
    /// Creates a new P2P service
    pub fn new(config: P2PConfig, event_tx: mpsc::Sender<P2PEvent>) -> Self {
        let (shutdown_tx, _) = broadcast::channel(8);
        let (broadcast_tx, _) = broadcast::channel(256);

        Self {
            config,
            state: Arc::new(RwLock::new(P2PServiceState::Stopped)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            shutdown_tx,
            local_height: Arc::new(RwLock::new(0)),
            pending_block_hashes: Arc::new(RwLock::new(VecDeque::new())),
            last_header_index: Arc::new(RwLock::new(0)),
            best_peer_height: Arc::new(RwLock::new(0)),
            broadcast_tx,
            chain: None,
        }
    }

    /// Returns a sender for broadcasting messages to all peers
    pub fn broadcast_sender(&self) -> broadcast::Sender<BroadcastMessage> {
        self.broadcast_tx.clone()
    }

    /// Sets the chain state for responding to GetHeaders requests
    pub fn set_chain(&mut self, chain: Arc<RwLock<neo_chain::ChainState>>) {
        self.chain = Some(chain);
    }

    /// Returns the current service state
    pub async fn state(&self) -> P2PServiceState {
        *self.state.read().await
    }

    /// Returns the number of connected peers
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Returns information about all connected peers
    pub async fn peers(&self) -> Vec<ConnectedPeer> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Sets the local node height
    pub async fn set_local_height(&self, height: u32) {
        *self.local_height.write().await = height;
    }

    /// Starts the P2P service
    pub async fn start(&self) -> anyhow::Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != P2PServiceState::Stopped {
                anyhow::bail!("P2P service is already running");
            }
            *state = P2PServiceState::Starting;
        }

        info!(
            target: "neo::p2p",
            listen = %self.config.listen_address,
            max_inbound = self.config.max_inbound,
            max_outbound = self.config.max_outbound,
            "starting P2P service"
        );

        // Start listener task
        let listener_config = self.config.clone();
        let listener_peers = self.peers.clone();
        let listener_event_tx = self.event_tx.clone();
        let listener_local_height = self.local_height.clone();
        let listener_pending_blocks = self.pending_block_hashes.clone();
        let listener_last_header = self.last_header_index.clone();
        let listener_best_height = self.best_peer_height.clone();
        let listener_chain = self.chain.clone();
        let mut listener_shutdown = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            Self::run_listener(
                listener_config,
                listener_peers,
                listener_event_tx,
                listener_local_height,
                listener_pending_blocks,
                listener_last_header,
                listener_best_height,
                listener_chain,
                &mut listener_shutdown,
            )
            .await;
        });

        // Start connector task for seed nodes
        let connector_config = self.config.clone();
        let connector_peers = self.peers.clone();
        let connector_event_tx = self.event_tx.clone();
        let connector_local_height = self.local_height.clone();
        let connector_pending_blocks = self.pending_block_hashes.clone();
        let connector_last_header = self.last_header_index.clone();
        let connector_best_height = self.best_peer_height.clone();
        let connector_chain = self.chain.clone();
        let mut connector_shutdown = self.shutdown_tx.subscribe();

        tokio::spawn(async move {
            Self::run_connector(
                connector_config,
                connector_peers,
                connector_event_tx,
                connector_local_height,
                connector_pending_blocks,
                connector_last_header,
                connector_best_height,
                connector_chain,
                &mut connector_shutdown,
            )
            .await;
        });

        *self.state.write().await = P2PServiceState::Running;
        info!(target: "neo::p2p", "P2P service started");

        Ok(())
    }

    /// Stops the P2P service
    pub async fn stop(&self) -> anyhow::Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != P2PServiceState::Running {
                return Ok(());
            }
            *state = P2PServiceState::Stopping;
        }

        info!(target: "neo::p2p", "stopping P2P service");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        // Clear peers
        self.peers.write().await.clear();

        *self.state.write().await = P2PServiceState::Stopped;
        info!(target: "neo::p2p", "P2P service stopped");

        Ok(())
    }

    /// Runs the TCP listener for inbound connections
    async fn run_listener(
        config: P2PConfig,
        peers: Arc<RwLock<HashMap<SocketAddr, ConnectedPeer>>>,
        event_tx: mpsc::Sender<P2PEvent>,
        local_height: Arc<RwLock<u32>>,
        pending_block_hashes: Arc<RwLock<VecDeque<UInt256>>>,
        last_header_index: Arc<RwLock<u32>>,
        best_peer_height: Arc<RwLock<u32>>,
        chain: Option<Arc<RwLock<neo_chain::ChainState>>>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        let listener = match TcpListener::bind(config.listen_address).await {
            Ok(l) => l,
            Err(e) => {
                error!(target: "neo::p2p", error = %e, "failed to bind listener");
                return;
            }
        };

        info!(
            target: "neo::p2p",
            address = %config.listen_address,
            "listening for inbound connections"
        );

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            // Check if we can accept more inbound connections
                            let current_inbound = peers.read().await
                                .values()
                                .filter(|p| p.is_inbound)
                                .count();

                            if current_inbound >= config.max_inbound {
                                warn!(
                                    target: "neo::p2p",
                                    addr = %addr,
                                    "rejecting inbound connection: max reached"
                                );
                                continue;
                            }

                            info!(target: "neo::p2p", addr = %addr, "accepted inbound connection");

                            // Add peer
                            let peer = ConnectedPeer {
                                address: addr,
                                state: ConnectionState::Connected,
                                height: 0,
                                user_agent: String::new(),
                                is_inbound: true,
                                last_activity: current_timestamp(),
                            };

                            peers.write().await.insert(addr, peer.clone());

                            // Spawn handler for this connection
                            let handler_peers = peers.clone();
                            let handler_event_tx = event_tx.clone();
                            let handler_local_height = local_height.clone();
                            let handler_pending_blocks = pending_block_hashes.clone();
                            let handler_last_header = last_header_index.clone();
                            let handler_best_height = best_peer_height.clone();
                            let handler_chain = chain.clone();
                            let network_magic = config.network_magic;
                            tokio::spawn(async move {
                                Self::handle_connection(
                                    stream,
                                    addr,
                                    handler_peers,
                                    handler_event_tx,
                                    network_magic,
                                    handler_local_height,
                                    handler_pending_blocks,
                                    handler_last_header,
                                    handler_best_height,
                                    handler_chain,
                                    true, // is_inbound
                                ).await;
                            });
                        }
                        Err(e) => {
                            error!(target: "neo::p2p", error = %e, "accept error");
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!(target: "neo::p2p", "listener shutting down");
                    break;
                }
            }
        }
    }

    /// Runs the connector for outbound connections to seed nodes
    async fn run_connector(
        config: P2PConfig,
        peers: Arc<RwLock<HashMap<SocketAddr, ConnectedPeer>>>,
        event_tx: mpsc::Sender<P2PEvent>,
        local_height: Arc<RwLock<u32>>,
        pending_block_hashes: Arc<RwLock<VecDeque<UInt256>>>,
        last_header_index: Arc<RwLock<u32>>,
        best_peer_height: Arc<RwLock<u32>>,
        chain: Option<Arc<RwLock<neo_chain::ChainState>>>,
        shutdown_rx: &mut broadcast::Receiver<()>,
    ) {
        // Initial delay before connecting
        tokio::time::sleep(Duration::from_secs(1)).await;

        for seed in &config.seed_nodes {
            // Check shutdown
            if shutdown_rx.try_recv().is_ok() {
                break;
            }

            // Check if we need more outbound connections
            let current_outbound = peers
                .read()
                .await
                .values()
                .filter(|p| !p.is_inbound)
                .count();

            if current_outbound >= config.max_outbound {
                break;
            }

            // Skip if already connected
            if peers.read().await.contains_key(seed) {
                continue;
            }

            info!(target: "neo::p2p", addr = %seed, "connecting to seed node");

            match tokio::time::timeout(config.connect_timeout, TcpStream::connect(seed)).await {
                Ok(Ok(stream)) => {
                    info!(target: "neo::p2p", addr = %seed, "connected to seed node");

                    let peer = ConnectedPeer {
                        address: *seed,
                        state: ConnectionState::Connected,
                        height: 0,
                        user_agent: String::new(),
                        is_inbound: false,
                        last_activity: current_timestamp(),
                    };

                    peers.write().await.insert(*seed, peer);

                    // Spawn handler
                    let handler_peers = peers.clone();
                    let handler_event_tx = event_tx.clone();
                    let handler_local_height = local_height.clone();
                    let handler_pending_blocks = pending_block_hashes.clone();
                    let handler_last_header = last_header_index.clone();
                    let handler_best_height = best_peer_height.clone();
                    let handler_chain = chain.clone();
                    let network_magic = config.network_magic;
                    let addr = *seed;
                    tokio::spawn(async move {
                        Self::handle_connection(
                            stream,
                            addr,
                            handler_peers,
                            handler_event_tx,
                            network_magic,
                            handler_local_height,
                            handler_pending_blocks,
                            handler_last_header,
                            handler_best_height,
                            handler_chain,
                            false, // is_inbound
                        )
                        .await;
                    });
                }
                Ok(Err(e)) => {
                    warn!(target: "neo::p2p", addr = %seed, error = %e, "failed to connect to seed");
                }
                Err(_) => {
                    warn!(target: "neo::p2p", addr = %seed, "connection timeout");
                }
            }
        }

        info!(target: "neo::p2p", "connector task completed");
    }

    /// Handles a single peer connection with full Neo P2P protocol
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        peers: Arc<RwLock<HashMap<SocketAddr, ConnectedPeer>>>,
        event_tx: mpsc::Sender<P2PEvent>,
        network_magic: u32,
        local_height: Arc<RwLock<u32>>,
        pending_block_hashes: Arc<RwLock<VecDeque<UInt256>>>,
        last_header_index: Arc<RwLock<u32>>,
        best_peer_height: Arc<RwLock<u32>>,
        chain: Option<Arc<RwLock<neo_chain::ChainState>>>,
        is_inbound: bool,
    ) {
        debug!(target: "neo::p2p", addr = %addr, inbound = is_inbound, "starting protocol handler");

        // Create peer connection with neo-core's PeerConnection
        let channels_config = ChannelsConfig::default();
        let mut connection =
            PeerConnection::from_channels_config(stream, addr, is_inbound, &channels_config);

        // Generate random nonce for this connection
        let nonce: u32 = rand::random();
        let height = *local_height.read().await;

        // Create capabilities (FullNode with current height)
        let capabilities = vec![NodeCapability::full_node(height)];

        // Create version payload
        let version = VersionPayload::create(
            network_magic,
            nonce,
            "/neo-rs:0.7.0/".to_string(),
            capabilities,
        );

        // For outbound connections, we initiate the handshake
        if !is_inbound {
            let version_msg = NetworkMessage::new(ProtocolMessage::Version(version.clone()));
            if let Err(e) = connection.send_message(&version_msg).await {
                error!(target: "neo::p2p", addr = %addr, error = %e, "failed to send version");
                peers.write().await.remove(&addr);
                let _ = event_tx.send(P2PEvent::PeerDisconnected(addr)).await;
                return;
            }
            debug!(target: "neo::p2p", addr = %addr, "sent version message");
        }

        // Handshake state
        let mut version_received = false;
        let mut verack_received = false;
        let mut peer_version: Option<VersionPayload> = None;

        // Message loop
        loop {
            // Receive message with handshake timeout
            let handshake_complete = version_received && verack_received;
            let message = match connection.receive_message(handshake_complete).await {
                Ok(msg) => msg,
                Err(e) => {
                    if !handshake_complete {
                        warn!(target: "neo::p2p", addr = %addr, error = %e, "handshake failed");
                    } else {
                        debug!(target: "neo::p2p", addr = %addr, error = %e, "connection closed");
                    }
                    break;
                }
            };

            let command = message.command();
            debug!(target: "neo::p2p", addr = %addr, ?command, "received message");

            match message.payload {
                ProtocolMessage::Version(remote_version) => {
                    if version_received {
                        warn!(target: "neo::p2p", addr = %addr, "duplicate version message");
                        break;
                    }

                    // Validate network magic
                    if remote_version.network != network_magic {
                        warn!(
                            target: "neo::p2p",
                            addr = %addr,
                            expected = format!("0x{:08x}", network_magic),
                            received = format!("0x{:08x}", remote_version.network),
                            "network magic mismatch"
                        );
                        break;
                    }

                    info!(
                        target: "neo::p2p",
                        addr = %addr,
                        user_agent = %remote_version.user_agent,
                        version = remote_version.version,
                        "received version"
                    );

                    // Update peer info
                    {
                        let mut peers_guard = peers.write().await;
                        if let Some(peer) = peers_guard.get_mut(&addr) {
                            peer.user_agent = remote_version.user_agent.clone();
                            // Extract height from capabilities
                            for cap in &remote_version.capabilities {
                                if let NodeCapability::FullNode { start_height } = cap {
                                    peer.height = *start_height;
                                }
                            }
                            peer.state = ConnectionState::Handshaking;
                        }
                    }

                    // Enable compression if both sides support it
                    connection.compression_allowed = remote_version.allow_compression;
                    peer_version = Some(remote_version);
                    version_received = true;

                    // For inbound connections, send our version after receiving theirs
                    if is_inbound {
                        let version_msg =
                            NetworkMessage::new(ProtocolMessage::Version(version.clone()));
                        if let Err(e) = connection.send_message(&version_msg).await {
                            error!(target: "neo::p2p", addr = %addr, error = %e, "failed to send version");
                            break;
                        }
                    }

                    // Send verack
                    let verack_msg = NetworkMessage::new(ProtocolMessage::Verack);
                    if let Err(e) = connection.send_message(&verack_msg).await {
                        error!(target: "neo::p2p", addr = %addr, error = %e, "failed to send verack");
                        break;
                    }
                    debug!(target: "neo::p2p", addr = %addr, "sent verack");
                }

                ProtocolMessage::Verack => {
                    if verack_received {
                        warn!(target: "neo::p2p", addr = %addr, "duplicate verack message");
                        break;
                    }
                    verack_received = true;
                    debug!(target: "neo::p2p", addr = %addr, "received verack");

                    // Handshake complete
                    if version_received {
                        info!(target: "neo::p2p", addr = %addr, "handshake complete");
                        {
                            let mut peers_guard = peers.write().await;
                            if let Some(peer) = peers_guard.get_mut(&addr) {
                                peer.state = ConnectionState::Ready;
                            }
                        }

                        // Emit peer connected event with full info
                        if let Some(ref pv) = peer_version {
                            let mut peer_height = 0u32;
                            for cap in &pv.capabilities {
                                if let NodeCapability::FullNode { start_height } = cap {
                                    peer_height = *start_height;
                                }
                            }
                            let _ = event_tx
                                .send(P2PEvent::PeerConnected(PeerInfo {
                                    address: addr,
                                    listen_port: Some(addr.port()),
                                    version: pv.version,
                                    user_agent: pv.user_agent.clone(),
                                    height: peer_height,
                                    is_inbound,
                                    latency_ms: None,
                                }))
                                .await;

                            // Update best known peer height for continuous sync
                            {
                                let mut best = best_peer_height.write().await;
                                if peer_height > *best {
                                    *best = peer_height;
                                    info!(
                                        target: "neo::p2p",
                                        addr = %addr,
                                        best_height = peer_height,
                                        "updated best known peer height"
                                    );
                                }
                            }

                            // Request blocks from genesis (index 0) for full sync
                            // Use local height as starting point, or 0 for fresh sync
                            let start_index = *local_height.read().await;
                            info!(
                                target: "neo::p2p",
                                addr = %addr,
                                start_index,
                                peer_height,
                                "requesting blocks from peer for sync"
                            );

                            // Request headers/blocks starting from our current height
                            let getheaders = GetBlockByIndexPayload::create(start_index, -1);
                            let getheaders_msg =
                                NetworkMessage::new(ProtocolMessage::GetHeaders(getheaders));
                            if let Err(e) = connection.send_message(&getheaders_msg).await {
                                warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send getheaders");
                            } else {
                                debug!(target: "neo::p2p", addr = %addr, start_index, "sent getheaders request");
                            }
                        }
                    }
                }

                ProtocolMessage::Ping(ping) => {
                    // Respond with pong
                    let pong =
                        PingPayload::create_with_nonce(*local_height.read().await, ping.nonce);
                    let pong_msg = NetworkMessage::new(ProtocolMessage::Pong(pong));
                    if let Err(e) = connection.send_message(&pong_msg).await {
                        error!(target: "neo::p2p", addr = %addr, error = %e, "failed to send pong");
                        break;
                    }
                    debug!(target: "neo::p2p", addr = %addr, "sent pong");
                }

                ProtocolMessage::Pong(pong) => {
                    // Update peer height
                    let mut peers_guard = peers.write().await;
                    if let Some(peer) = peers_guard.get_mut(&addr) {
                        peer.height = pong.last_block_index;
                        peer.last_activity = current_timestamp();
                    }
                    debug!(target: "neo::p2p", addr = %addr, height = pong.last_block_index, "received pong");
                }

                ProtocolMessage::GetHeaders(payload) => {
                    let local_h = *local_height.read().await;
                    debug!(
                        target: "neo::p2p",
                        addr = %addr,
                        start = payload.index_start,
                        count = payload.count,
                        local_height = local_h,
                        "received getheaders request"
                    );

                    // Respond with headers if we have chain state access
                    if let Some(ref chain_state) = chain {
                        let chain_guard = chain_state.read().await;
                        let start = payload.index_start;
                        // Neo protocol: count=-1 means "as many as possible" (max 2000)
                        let max_headers = if payload.count < 0 { 2000i16 } else { payload.count.min(2000) };
                        let end = (start as i64 + max_headers as i64).min(local_h as i64 + 1) as u32;

                        if start <= local_h {
                            // Collect block hashes from chain state for the requested range
                            let mut header_hashes = Vec::new();
                            for height in start..end {
                                if let Some(entry) = chain_guard.get_block_at_height(height) {
                                    header_hashes.push(entry.hash);
                                }
                            }

                            if !header_hashes.is_empty() {
                                info!(
                                    target: "neo::p2p",
                                    addr = %addr,
                                    start,
                                    count = header_hashes.len(),
                                    "responding with block hashes for getheaders"
                                );
                                // Note: Full header response would require storing full headers
                                // in ChainState. Currently we only have BlockIndexEntry with hash.
                                // Send inventory of block hashes for header sync
                                let inv = InvPayload::create(InventoryType::Block, &header_hashes);
                                let inv_msg = NetworkMessage::new(ProtocolMessage::Inv(inv));
                                if let Err(e) = connection.send_message(&inv_msg).await {
                                    warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send inv response");
                                }
                            }
                        } else {
                            debug!(
                                target: "neo::p2p",
                                addr = %addr,
                                "peer requesting headers beyond our chain height"
                            );
                        }
                    } else {
                        debug!(
                            target: "neo::p2p",
                            addr = %addr,
                            "no chain state available to respond to getheaders"
                        );
                    }
                }

                ProtocolMessage::Headers(mut headers) => {
                    let count = headers.headers.len();
                    info!(target: "neo::p2p", addr = %addr, count, "received headers");

                    // Extract block hashes and indices from headers for sync
                    let mut max_index = 0u32;
                    let block_hashes: Vec<neo_core::UInt256> = headers
                        .headers
                        .iter_mut()
                        .map(|h| {
                            max_index = max_index.max(h.index());
                            h.hash()
                        })
                        .collect();

                    // Convert headers to raw bytes for the event
                    let header_bytes: Vec<Vec<u8>> =
                        block_hashes.iter().map(|h| h.to_bytes().to_vec()).collect();
                    let _ = event_tx
                        .send(P2PEvent::HeadersReceived {
                            headers: header_bytes,
                            from: addr,
                        })
                        .await;

                    // Add ALL block hashes to pending queue for continuous sync
                    if !block_hashes.is_empty() {
                        {
                            let mut pending = pending_block_hashes.write().await;
                            for hash in &block_hashes {
                                pending.push_back(*hash);
                            }
                            info!(
                                target: "neo::p2p",
                                addr = %addr,
                                new_hashes = block_hashes.len(),
                                total_pending = pending.len(),
                                max_index,
                                "added block hashes to sync queue"
                            );
                        }

                        // Update last header index for continuous sync
                        {
                            let mut last_idx = last_header_index.write().await;
                            if max_index > *last_idx {
                                *last_idx = max_index;
                            }
                        }

                        // Request first batch of blocks from queue
                        let batch: Vec<neo_core::UInt256> = {
                            let mut pending = pending_block_hashes.write().await;
                            let batch_size = std::cmp::min(BLOCK_BATCH_SIZE, pending.len());
                            pending.drain(..batch_size).collect()
                        };

                        if !batch.is_empty() {
                            info!(
                                target: "neo::p2p",
                                addr = %addr,
                                batch_size = batch.len(),
                                "requesting blocks batch for sync"
                            );

                            let getdata = InvPayload::create(InventoryType::Block, &batch);
                            let getdata_msg =
                                NetworkMessage::new(ProtocolMessage::GetData(getdata));
                            if let Err(e) = connection.send_message(&getdata_msg).await {
                                warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send getdata for blocks");
                            }
                        }

                        // If we received max headers (2000), request more headers for continuous sync
                        let best_height = *best_peer_height.read().await;
                        if count >= 2000 && max_index < best_height {
                            let next_start = max_index + 1;
                            info!(
                                target: "neo::p2p",
                                addr = %addr,
                                next_start,
                                best_height,
                                "requesting more headers for continuous sync"
                            );

                            let getheaders = GetBlockByIndexPayload::create(next_start, -1);
                            let getheaders_msg =
                                NetworkMessage::new(ProtocolMessage::GetHeaders(getheaders));
                            if let Err(e) = connection.send_message(&getheaders_msg).await {
                                warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send getheaders");
                            }
                        }
                    }
                }

                ProtocolMessage::Inv(inv) => {
                    debug!(target: "neo::p2p", addr = %addr, inv_type = ?inv.inventory_type, count = inv.hashes.len(), "received inv");
                    let _ = event_tx
                        .send(P2PEvent::InventoryReceived {
                            inv_type: inv.inventory_type.into(),
                            hashes: inv.hashes.clone(),
                            from: addr,
                        })
                        .await;

                    // Auto-request blocks when we receive block inventory
                    if inv.inventory_type == InventoryType::Block && !inv.hashes.is_empty() {
                        let getdata = InvPayload::create(InventoryType::Block, &inv.hashes);
                        let getdata_msg = NetworkMessage::new(ProtocolMessage::GetData(getdata));
                        if let Err(e) = connection.send_message(&getdata_msg).await {
                            warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send getdata for blocks");
                        } else {
                            debug!(target: "neo::p2p", addr = %addr, count = inv.hashes.len(), "sent getdata for blocks");
                        }
                    }
                }

                ProtocolMessage::Block(mut block) => {
                    let hash = block.hash();
                    let height = block.index();
                    let tx_count = block.transactions.len();
                    info!(target: "neo::p2p", addr = %addr, height, hash = %hash, tx_count, "received block");

                    // Serialize block for event
                    let data = block.to_array().unwrap_or_default();
                    let _ = event_tx
                        .send(P2PEvent::BlockReceived {
                            hash,
                            data,
                            from: addr,
                        })
                        .await;

                    // Update local height tracking
                    {
                        let mut local = local_height.write().await;
                        if height > *local {
                            *local = height;
                        }
                    }

                    // Request next batch of blocks from pending queue for continuous sync
                    let pending_count = pending_block_hashes.read().await.len();
                    if pending_count > 0 {
                        let batch: Vec<neo_core::UInt256> = {
                            let mut pending = pending_block_hashes.write().await;
                            let batch_size = std::cmp::min(BLOCK_BATCH_SIZE, pending.len());
                            pending.drain(..batch_size).collect()
                        };

                        if !batch.is_empty() {
                            debug!(
                                target: "neo::p2p",
                                addr = %addr,
                                batch_size = batch.len(),
                                remaining = pending_count - batch.len(),
                                "requesting next blocks batch"
                            );

                            let getdata = InvPayload::create(InventoryType::Block, &batch);
                            let getdata_msg =
                                NetworkMessage::new(ProtocolMessage::GetData(getdata));
                            if let Err(e) = connection.send_message(&getdata_msg).await {
                                warn!(target: "neo::p2p", addr = %addr, error = %e, "failed to send getdata for next batch");
                            }
                        }
                    }
                }

                ProtocolMessage::Transaction(tx) => {
                    let hash = tx.hash();
                    debug!(target: "neo::p2p", addr = %addr, hash = %hash, "received transaction");

                    // Serialize transaction for event
                    let data = tx.to_array().unwrap_or_default();
                    let _ = event_tx
                        .send(P2PEvent::TransactionReceived {
                            hash,
                            data,
                            from: addr,
                        })
                        .await;
                }

                ProtocolMessage::Extensible(payload) => {
                    debug!(target: "neo::p2p", addr = %addr, category = %payload.category, "received extensible");
                    match payload.category.as_str() {
                        "dBFT" => {
                            let _ = event_tx
                                .send(P2PEvent::ConsensusReceived {
                                    data: payload.data.clone(),
                                    from: addr,
                                })
                                .await;
                        }
                        "StateRoot" => {
                            info!(target: "neo::p2p", addr = %addr, "received state root message");
                            let _ = event_tx
                                .send(P2PEvent::StateRootReceived {
                                    data: payload.data.clone(),
                                    from: addr,
                                })
                                .await;
                        }
                        _ => {
                            debug!(target: "neo::p2p", addr = %addr, category = %payload.category, "unknown extensible category");
                        }
                    }
                }

                _ => {
                    debug!(target: "neo::p2p", addr = %addr, ?command, "unhandled message type");
                }
            }
        }

        // Cleanup on disconnect
        let _ = connection.close().await;
        peers.write().await.remove(&addr);
        let _ = event_tx.send(P2PEvent::PeerDisconnected(addr)).await;
        info!(target: "neo::p2p", addr = %addr, "peer disconnected");
    }
}

/// Gets current timestamp in seconds
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_p2p_service_creation() {
        let (tx, _rx) = mpsc::channel(100);
        let config = P2PConfig::default();
        let service = P2PService::new(config, tx);

        assert_eq!(service.state().await, P2PServiceState::Stopped);
        assert_eq!(service.peer_count().await, 0);
    }

    #[tokio::test]
    async fn test_p2p_service_start_stop() {
        let (tx, _rx) = mpsc::channel(100);
        let config = P2PConfig {
            listen_address: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };
        let service = P2PService::new(config, tx);

        service.start().await.unwrap();
        assert_eq!(service.state().await, P2PServiceState::Running);

        service.stop().await.unwrap();
        assert_eq!(service.state().await, P2PServiceState::Stopped);
    }
}
