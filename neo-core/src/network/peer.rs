use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime};
use std::sync::Arc;
use std::net::Ipv4Addr;
use neo_crypto::rand;
use crate::network::{ChannelsConfig, RemoteNode};

pub struct Peer {
    tcp_listener: Option<JoinHandle<()>>,
    timer: Option<JoinHandle<()>>,
    connected_addresses: Arc<RwLock<HashMap<IpAddr, usize>>>,
    connected_peers: Arc<RwLock<HashMap<mpsc::Sender<PeerMessage>, SocketAddr>>>,
    unconnected_peers: Arc<RwLock<HashSet<SocketAddr>>>,
    connecting_peers: Arc<RwLock<HashSet<SocketAddr>>>,
    trusted_ip_addresses: Arc<RwLock<HashSet<IpAddr>>>,
    listener_tcp_port: u16,
    max_connections_per_address: usize,
    min_desired_connections: usize,
    max_connections: usize,
    unconnected_max: usize,
}

pub enum PeerMessage {
    ChannelsConfig(ChannelsConfig),
    Timer,
    Peers { endpoints: Vec<SocketAddr> },
    Connect { endpoint: SocketAddr, is_trusted: bool },
    TcpConnected { remote: SocketAddr, local: SocketAddr },
    TcpBound,
    TcpCommandFailed(SocketAddr),
    Terminated(mpsc::Sender<PeerMessage>),
}

impl Peer {
    pub fn new(
        listener_tcp_port: u16,
        max_connections_per_address: usize,
        min_desired_connections: usize,
        max_connections: usize,
        unconnected_max: usize,
    ) -> Self {
        Self {
            tcp_listener: None,
            timer: None,
            connected_addresses: Arc::new(RwLock::new(HashMap::new())),
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
            unconnected_peers: Arc::new(RwLock::new(HashSet::new())),
            connecting_peers: Arc::new(RwLock::new(HashSet::new())),
            trusted_ip_addresses: Arc::new(RwLock::new(HashSet::new())),
            listener_tcp_port,
            max_connections_per_address,
            min_desired_connections,
            max_connections,
            unconnected_max,
        }
    }

    pub async fn add_trusted_peer(&self, address: IpAddr) {
        self.trusted_ip_addresses.write().await.insert(address);
    }

    pub async fn remove_trusted_peer(&self, address: &IpAddr) {
        self.trusted_ip_addresses.write().await.remove(address);
    }

    pub async fn connect(&self, endpoint: SocketAddr) {
        let is_trusted = self.trusted_ip_addresses.read().await.contains(&endpoint.ip());
        
        if self.connecting_peers.read().await.contains(&endpoint) {
            return; // Already connecting to this peer
        }

        if self.connected_peers.read().await.contains_key(&endpoint) {
            return; // Already connected to this peer
        }

        let connected_count = self.connected_peers.read().await.len();
        if connected_count >= self.max_connections {
            return; // Maximum connections reached
        }

        self.connecting_peers.write().await.insert(endpoint);

        // Attempt to establish a TCP connection
        match TcpStream::connect(endpoint).await {
            Ok(stream) => {
                // Handle successful connection
                self.handle_new_connection(stream, endpoint, is_trusted).await;
            }
            Err(_) => {
                // Handle connection failure
                self.connecting_peers.write().await.remove(&endpoint);
                self.unconnected_peers.write().await.insert(endpoint);
            }
        }
    }

    async fn on_timer(&self) {
        self.prune_unconnected_peers().await;
        self.attempt_new_connections().await;
    }

    async fn prune_unconnected_peers(&self) {
        let mut unconnected = self.unconnected_peers.write().await;
        if unconnected.len() > self.unconnected_max {
            let to_remove = unconnected.len() - self.unconnected_max;
            unconnected.retain(|_| rand::random::<f32>() > (to_remove as f32 / unconnected.len() as f32));
        }
    }

    async fn attempt_new_connections(&self) {
        let connected_count = self.connected_peers.read().await.len();
        if connected_count < self.min_desired_connections {
            let to_connect = self.min_desired_connections - connected_count;
            let unconnected = self.unconnected_peers.read().await;
            
            for endpoint in unconnected.iter().take(to_connect) {
                self.connect(*endpoint).await;
            }
        }
    }

    pub async fn run(&mut self) {
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        let peer = self.clone();
        self.tcp_listener = Some(tokio::spawn(async move {
            // TODO: Implement TCP listener logic
        }));

        let peer = self.clone();
        self.timer = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        peer.on_timer().await;
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }
        }));

        let (tx, mut rx) = mpsc::channel(100);

        // Main event loop
        loop {
            tokio::select! {
                Some(message) = rx.recv() => {
                    self.on_receive(message).await;
                }
                _ = &mut shutdown_rx => {
                    break;
                }
            }
        }

        // Shutdown logic
        if let Some(tcp_listener) = self.tcp_listener.take() {
            tcp_listener.abort();
        }
        if let Some(timer) = self.timer.take() {
            let _ = shutdown_tx.send(());
            timer.await.unwrap();
        }
    }

    async fn on_receive(&mut self, message: PeerMessage) {
        match message {
            PeerMessage::ChannelsConfig(config) => self.on_start(config).await,
            PeerMessage::Timer => self.on_timer().await,
            PeerMessage::Peers { endpoints } => self.add_peers(endpoints),
            PeerMessage::Connect { endpoint, is_trusted } => self.connect_to_peer(endpoint, is_trusted).await,
            PeerMessage::TcpConnected { remote, local } => self.on_tcp_connected(remote, local).await,
            PeerMessage::TcpBound => {
                // Handle TCP bound event
                log::info!("TCP listener bound successfully");
                self.is_listening = true;
                self.update_node_capabilities();
                self.broadcast_address_to_peers().await;
            },
            PeerMessage::TcpCommandFailed(remote) => self.on_tcp_command_failed(remote).await,
            PeerMessage::Terminated(peer) => self.on_peer_disconnected(peer).await,
        }
    }

    async fn on_start(&mut self, config: ChannelsConfig) {
        // Initialize channels based on the provided configuration
        self.channels = config.channels.clone();

        // Set up TCP listener if specified in the config
        if let Some(listen_address) = config.listen_address {
            match TcpListener::bind(listen_address).await {
                Ok(listener) => {
                    self.tcp_listener = Some(tokio::spawn(async move {
                        while let Ok((stream, addr)) = listener.accept().await {
                            // Handle new incoming connection
                            if let Err(e) = self.handle_incoming_connection(stream, addr).await {
                                log::error!("Failed to handle incoming connection from {}: {}", addr, e);
                            }
                        }
                    }));
                },
                Err(e) => log::error!("Failed to bind TCP listener: {}", e),
            }
        }

        // Initialize other components based on the config
        self.max_connections = config.max_connections;
        self.min_desirable_connections = config.min_desirable_connections;

        // Start the timer for periodic tasks
        let timer_interval = config.timer_interval;
        self.timer = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(timer_interval);
            loop {
                interval.tick().await;
                if let Err(e) = self.sender.send(PeerMessage::Timer).await {
                    log::error!("Failed to send timer message: {}", e);
                    break;
                }
            }
        }));

        // Initialize the peer discovery process
        self.discover_peers().await;

        log::info!("Peer network started with configuration: {:?}", config);
    }

    async fn on_timer(&mut self) {
        // Implement periodic tasks
        self.check_dead_peers().await;
        self.connect_to_new_peers().await;
        self.broadcast_inventory().await;
    }

    async fn add_peers(&mut self, endpoints: Vec<SocketAddr>) {
        for endpoint in endpoints {
            if !self.known_addresses.contains(&endpoint) {
                self.known_addresses.insert(endpoint);
                // Attempt to connect if we have room for more connections
                if self.connected_peers.len() < self.max_connections {
                    self.connect_to_peer(endpoint, false).await;
                }
            }
        }
    }

    async fn connect_to_peer(&mut self, endpoint: SocketAddr, is_trusted: bool) {
        // Implement connection logic
        // Use tokio's TcpStream to establish connection
        match TcpStream::connect(endpoint).await {
            Ok(stream) => {
                let peer = RemoteNode::new(stream, is_trusted);
                self.connected_peers.insert(endpoint, peer);
                // Initiate handshake process
                self.initiate_handshake(endpoint).await;
            }
            Err(e) => {
                log::warn!("Failed to connect to peer {}: {}", endpoint, e);
            }
        }
    }

    async fn on_tcp_connected(&mut self, remote: SocketAddr, local: SocketAddr) {
        log::info!("Connected to peer {} from local address {}", remote, local);
        
        // Update the peer's connection status
        if let Some(peer_sender) = self.connected_peers.write().await.values().find(|&&addr| addr == remote) {
            let status_update = PeerMessage::SetStatus(ConnectionStatus::Connected);
            if let Err(e) = peer_sender.send(status_update).await {
                log::error!("Failed to update peer status: {}", e);
            }
        }

        // Send version message to initiate handshake
        if let Some(peer_sender) = self.connected_peers.read().await.values().find(|&&addr| addr == remote) {
            let version_payload = self.create_version_payload(remote);
            let version_message = PeerMessage::SendMessage(Message::Version(version_payload));
            if let Err(e) = peer_sender.send(version_message).await {
                log::error!("Failed to send version message: {}", e);
            }
        }

        // Start ping/pong timer for the new connection
        self.start_ping_timer(remote);

        // Update last connection time
        self.last_connection_time = SystemTime::now();

        // Notify other components about the new connection
        self.notify_new_connection(remote, local);
    }

    async fn on_tcp_command_failed(&mut self, remote: SocketAddr) {
        log::warn!("TCP command failed for peer {}", remote);
        self.disconnect_peer(remote).await;
    }

    async fn on_peer_disconnected(&mut self, peer: SocketAddr) {
        log::info!("Peer {} disconnected", peer);
        self.connected_peers.remove(&peer);
        self.known_addresses.remove(&peer);
    }

    async fn check_dead_peers(&mut self) {
        let mut dead_peers = Vec::new();
        for (peer, node) in &mut self.connected_peers {
            if !node.is_alive() {
                dead_peers.push(*peer);
            }
        }
        for peer in dead_peers {
            log::info!("Removing unresponsive peer: {}", peer);
            self.disconnect_peer(peer).await;
        }
    }

    async fn connect_to_new_peers(&mut self) {
        let current_connections = self.connected_peers.len();
        if current_connections < self.min_desired_connections {
            let peers_to_connect = self.min_desired_connections - current_connections;
            let mut new_peers = self.known_addresses
                .difference(&self.connected_peers.keys().cloned().collect())
                .cloned()
                .collect::<Vec<_>>();
            new_peers.shuffle(&mut rand::thread_rng());
            for peer in new_peers.iter().take(peers_to_connect) {
                self.connect_to_peer(*peer, false).await;
            }
        }
    }

    async fn broadcast_inventory(&mut self) {
        if let Some(inventory) = self.get_latest_inventory() {
            for (_, node) in &mut self.connected_peers {
                node.send_inventory(inventory.clone()).await;
            }
        }
    }

    async fn initiate_handshake(&mut self, peer: SocketAddr) {
        if let Some(peer_sender) = self.connected_peers.read().await.values().find(|&&addr| addr == peer) {
            let version_payload = self.create_version_payload();
            match node.send_version(version_payload).await {
                Ok(_) => {
                    log::info!("Handshake initiated with peer: {}", peer);
                    // Wait for version message from peer
                    match node.wait_for_version().await {
                        Ok(peer_version) => {
                            log::info!("Received version from peer {}: {:?}", peer, peer_version);
                            // Send verack message
                            if let Err(e) = node.send_verack().await {
                                log::error!("Failed to send verack to peer {}: {}", peer, e);
                                self.disconnect_peer(peer).await;
                            }
                        },
                        Err(e) => {
                            log::error!("Failed to receive version from peer {}: {}", peer, e);
                            self.disconnect_peer(peer).await;
                        }
                    }
                },
                Err(e) => {
                    log::error!("Failed to initiate handshake with peer {}: {}", peer, e);
                    self.disconnect_peer(peer).await;
                }
            }
        }
    }

    async fn disconnect_peer(&mut self, peer: SocketAddr) {
        if let Some(node) = self.connected_peers.remove(&peer) {
            // Implement proper disconnection logic
            node.close().await;
        }
    }
}
