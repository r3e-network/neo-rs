//
// state.rs - LocalNode state management
//

use super::helpers::current_unix_timestamp;
use super::*;
use crate::network::p2p::{BanList, InboundRateLimiter, PeerReputationTracker};
use crate::wallets::KeyPair;

#[derive(Debug, Clone)]
pub(super) struct RemoteActorEntry {
    pub(super) actor: ActorRef,
    pub(super) snapshot: RemoteNodeSnapshot,
    pub(super) version: VersionPayload,
}

/// Represents the local node in the P2P network (mirrors C# Neo.Network.P2P.LocalNode).
#[derive(Debug)]
pub struct LocalNode {
    /// Runtime protocol settings shared with the wider system.
    settings: Arc<ProtocolSettings>,
    /// Last applied channel configuration (parity with C# LocalNode.Config).
    config: RwLock<ChannelsConfig>,
    /// Random nonce identifying this node instance.
    pub nonce: u32,
    /// User agent advertised during version handshake.
    pub user_agent: String,
    /// Node identity key pair for P2P authentication (C# v3.9.2+).
    #[allow(dead_code)]
    node_key: Arc<KeyPair>,
    /// Listening port for inbound connections.
    port: RwLock<u16>,
    /// Supported node capabilities.
    capabilities: Arc<RwLock<Vec<NodeCapability>>>,
    /// Connected peers keyed by their remote socket address.
    peers: Arc<RwLock<HashMap<SocketAddr, RemoteNodeSnapshot>>>,
    /// SECURITY: Index of peer addresses by IP for O(1) connection-per-IP lookups.
    /// This prevents DoS attacks where checking max_connections_per_address was O(n).
    peers_by_ip: Arc<RwLock<HashMap<IpAddr, HashSet<SocketAddr>>>>,
    /// Remote node actors keyed by their path string.
    remote_nodes: Arc<RwLock<HashMap<String, RemoteActorEntry>>>,
    /// History of broadcast operations for diagnostics/testing.
    broadcasts: Arc<RwLock<Vec<BroadcastEvent>>>,
    /// Seed addresses configured via protocol settings.
    seed_list: Arc<RwLock<Vec<String>>>,
    /// Pending outbound connection attempts keyed by endpoint.
    pending_connections: Arc<RwLock<HashSet<SocketAddr>>>,
    /// Shared system context providing access to global actors/services.
    system_context: RwLock<Option<Arc<NeoSystemContext>>>,
    /// SECURITY: Rate limiter for inbound connections to prevent DoS attacks.
    inbound_rate_limiter: RwLock<InboundRateLimiter>,
    /// SECURITY: Ban list for misbehaving peers.
    ban_list: RwLock<BanList>,
    /// SECURITY: Peer reputation tracker for identifying good/bad peers.
    reputation_tracker: Arc<PeerReputationTracker>,
}

impl PeerManagerService for LocalNode {
    fn peer_count(&self) -> usize {
        self.peers.read().len()
    }
}

impl LocalNode {
    pub const PROTOCOL_VERSION: u32 = 0;

    /// Creates a new local node matching the behaviour of the C# constructor.
    pub fn new(settings: Arc<ProtocolSettings>, port: u16, user_agent: String) -> Self {
        let node_key = Arc::new(KeyPair::generate().expect("Failed to generate node key pair"));
        Self::with_key(settings, port, user_agent, node_key)
    }

    /// Creates a local node with a specific identity key pair (C# v3.9.2+).
    pub fn with_key(
        settings: Arc<ProtocolSettings>,
        port: u16,
        user_agent: String,
        node_key: Arc<KeyPair>,
    ) -> Self {
        let mut nonce_bytes = [0u8; 4];
        OsRng.fill_bytes(&mut nonce_bytes);
        Self {
            settings,
            config: RwLock::new(ChannelsConfig::default()),
            nonce: u32::from_le_bytes(nonce_bytes),
            user_agent,
            node_key,
            port: RwLock::new(port),
            capabilities: Arc::new(RwLock::new(vec![
                NodeCapability::tcp_server(port),
                NodeCapability::full_node(0),
            ])),
            peers: Arc::new(RwLock::new(HashMap::new())),
            peers_by_ip: Arc::new(RwLock::new(HashMap::new())),
            remote_nodes: Arc::new(RwLock::new(HashMap::new())),
            broadcasts: Arc::new(RwLock::new(Vec::new())),
            seed_list: Arc::new(RwLock::new(Vec::new())),
            pending_connections: Arc::new(RwLock::new(HashSet::new())),
            system_context: RwLock::new(None),
            inbound_rate_limiter: RwLock::new(InboundRateLimiter::default()),
            ban_list: RwLock::new(BanList::new()),
            reputation_tracker: Arc::new(PeerReputationTracker::new()),
        }
    }

    /// Adds a capability to the node.
    pub fn add_capability(&self, capability: NodeCapability) {
        self.capabilities.write().push(capability);
    }

    /// Returns the number of connected peers.
    pub fn connected_peers_count(&self) -> usize {
        self.read_peers().len()
    }

    /// Adds or updates a connected peer snapshot.
    /// SECURITY: Also maintains the peers_by_ip index for O(1) connection-per-IP lookups.
    pub fn add_peer(
        &self,
        remote_address: SocketAddr,
        listener_tcp_port: Option<u16>,
        version: u32,
        services: u64,
        last_block_index: u32,
    ) {
        let timestamp = current_unix_timestamp();
        let listen_tcp_port = listener_tcp_port.unwrap_or_else(|| remote_address.port());
        let remote_ip = Self::normalize_ip(remote_address);

        let mut peers = self.write_peers();
        let is_new = !peers.contains_key(&remote_address);

        peers
            .entry(remote_address)
            .and_modify(|snapshot| snapshot.touch(last_block_index, timestamp))
            .or_insert_with(|| RemoteNodeSnapshot {
                remote_address,
                remote_port: remote_address.port(),
                listen_tcp_port,
                last_block_index,
                version,
                services,
                timestamp,
            });

        // SECURITY: Maintain IP index for O(1) lookups
        if is_new {
            drop(peers); // Release peers lock before acquiring peers_by_ip lock
            let mut by_ip = self.peers_by_ip.write();
            by_ip.entry(remote_ip).or_default().insert(remote_address);
        }
    }

    /// Updates the last known block height for the specified peer.
    pub fn update_peer_height(&self, remote_address: &SocketAddr, last_block_index: u32) {
        let timestamp = current_unix_timestamp();
        if let Some(snapshot) = self.write_peers().get_mut(remote_address) {
            snapshot.touch(last_block_index, timestamp);
        }
    }

    /// Removes a peer from the local registry.
    /// SECURITY: Also maintains the peers_by_ip index for O(1) connection-per-IP lookups.
    pub fn remove_peer(&self, address: &SocketAddr) -> bool {
        let removed = self.write_peers().remove(address).is_some();

        // SECURITY: Maintain IP index for O(1) lookups
        if removed {
            let remote_ip = Self::normalize_ip(*address);
            let mut by_ip = self.peers_by_ip.write();
            if let Some(addrs) = by_ip.get_mut(&remote_ip) {
                addrs.remove(address);
                if addrs.is_empty() {
                    by_ip.remove(&remote_ip);
                }
            }
        }

        removed
    }

    /// Returns the list of connected peer endpoints.
    pub fn get_peers(&self) -> Vec<SocketAddr> {
        self.read_peers().keys().copied().collect()
    }

    /// Returns detailed snapshots of connected peers, mirroring the data exposed by C# RemoteNode.
    pub fn remote_nodes(&self) -> Vec<RemoteNodeSnapshot> {
        self.read_peers().values().cloned().collect()
    }

    pub(super) fn remote_entries(&self) -> Vec<RemoteActorEntry> {
        self.remote_nodes.read().values().cloned().collect()
    }

    /// Returns snapshots for remote peers tracked by the local node.
    pub fn remote_snapshots(&self) -> Vec<RemoteNodeSnapshot> {
        self.remote_nodes
            .read()
            .values()
            .map(|entry| entry.snapshot.clone())
            .collect()
    }

    /// Returns the maximum reported block height across connected peers.
    pub fn max_peer_block_height(&self) -> u32 {
        self.remote_nodes
            .read()
            .values()
            .map(|entry| entry.snapshot.last_block_index)
            .max()
            .unwrap_or(0)
    }

    /// Records a relay broadcast (mirrors LocalNode.OnRelayDirectly semantics).
    pub fn record_relay(&self, inventory: &RelayInventory) {
        self.record_broadcast(BroadcastEvent::Relay(inventory.to_bytes()));
    }

    /// Records a direct send broadcast.
    pub fn record_send(&self, inventory: &RelayInventory) {
        self.record_broadcast(BroadcastEvent::Direct(inventory.to_bytes()));
    }

    /// Returns the captured broadcast history.
    pub fn broadcast_history(&self) -> Vec<BroadcastEvent> {
        self.broadcasts.read().clone()
    }

    /// Replaces the seed list used when requesting new peers.
    pub fn set_seed_list<S>(&self, seeds: S)
    where
        S: Into<Vec<String>>,
    {
        *self.seed_list.write() = seeds.into();
    }

    /// Returns the configured seed list.
    pub fn seed_list(&self) -> Vec<String> {
        self.seed_list.read().clone()
    }

    /// Returns the TCP listening port set for this node.
    pub fn port(&self) -> u16 {
        *self.port.read()
    }

    /// Updates the TCP listening port.
    pub fn set_port(&self, port: u16) {
        *self.port.write() = port;
    }

    /// Associates the Neo system context with this local node.
    pub fn set_system_context(&self, context: Arc<NeoSystemContext>) {
        *self.system_context.write() = Some(context);
    }

    /// Returns the previously attached system context if available.
    pub fn system_context(&self) -> Option<Arc<NeoSystemContext>> {
        self.system_context.read().as_ref().cloned()
    }

    /// Provides a handle to the shared protocol settings.
    pub fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    /// Returns the last applied channel configuration snapshot.
    pub fn config(&self) -> ChannelsConfig {
        self.config.read().clone()
    }

    /// Generates the version payload broadcast during handshake.
    pub fn version_payload(&self) -> VersionPayload {
        let mut capabilities = self.capabilities.read().clone();

        let context = self.system_context();
        let current_height = context
            .as_ref()
            .map(|ctx| ctx.current_block_index())
            .unwrap_or(0);

        let mut has_full_node = false;
        for capability in &mut capabilities {
            if let NodeCapability::FullNode { start_height } = capability {
                *start_height = current_height;
                has_full_node = true;
            }
        }
        if !has_full_node {
            capabilities.push(NodeCapability::full_node(current_height));
        }

        if !capabilities
            .iter()
            .any(|cap| matches!(cap, NodeCapability::ArchivalNode))
        {
            capabilities.push(NodeCapability::archival_node());
        }

        VersionPayload::create(
            &self.settings,
            self.nonce,
            self.user_agent.clone(),
            capabilities,
        )
    }

    /// Returns a list of network addresses used to respond to `GetAddr` messages.
    pub fn address_book(&self) -> Vec<NetworkAddressWithTime> {
        let guard = self.remote_nodes.read();

        let mut rng = thread_rng();
        guard
            .values()
            .filter(|entry| entry.snapshot.listen_tcp_port > 0)
            .choose_multiple(&mut rng, MAX_COUNT_TO_SEND)
            .into_iter()
            .map(|entry| {
                let ip = entry.snapshot.remote_address.ip();
                let mut capabilities = entry.version.capabilities.clone();

                let mut has_tcp = false;
                for capability in &mut capabilities {
                    if let NodeCapability::TcpServer { port } = capability {
                        *port = entry.snapshot.listen_tcp_port;
                        has_tcp = true;
                    }
                }

                if !has_tcp {
                    capabilities.push(NodeCapability::tcp_server(entry.snapshot.listen_tcp_port));
                }

                NetworkAddressWithTime::new(entry.snapshot.timestamp as u32, ip, capabilities)
            })
            .collect()
    }

    /// Applies channel configuration updates to the internal capability view.
    pub fn apply_channels_config(&self, config: &ChannelsConfig) {
        *self.config.write() = config.clone();

        let listener_port = config.tcp.map(|endpoint| endpoint.port()).unwrap_or(0);
        self.set_port(listener_port);

        let mut capabilities = self.capabilities.write();

        // Ensure TCP capability is aligned with configured port.
        if let Some(endpoint) = config.tcp {
            let mut updated = false;
            for capability in capabilities.iter_mut() {
                if let NodeCapability::TcpServer { port } = capability {
                    *port = endpoint.port();
                    updated = true;
                    break;
                }
            }
            if !updated {
                capabilities.push(NodeCapability::tcp_server(endpoint.port()));
            }
        } else {
            capabilities.retain(|cap| !matches!(cap, NodeCapability::TcpServer { .. }));
        }

        // Toggle compression capability.
        capabilities.retain(|cap| !matches!(cap, NodeCapability::DisableCompression));
        if !config.enable_compression {
            capabilities.push(NodeCapability::disable_compression());
        }
    }

    fn record_broadcast(&self, event: BroadcastEvent) {
        let limit = self.config.read().broadcast_history_limit;
        if limit == 0 {
            return;
        }

        let mut guard = self.broadcasts.write();
        guard.push(event);
        if guard.len() > limit {
            let excess = guard.len() - limit;
            guard.drain(0..excess);
        }
    }

    /// Determines whether a new remote connection should be accepted, mirroring
    /// the rules from the C# `LocalNode.AllowNewConnection` implementation.
    pub fn allow_new_connection(
        &self,
        snapshot: &RemoteNodeSnapshot,
        version: &VersionPayload,
    ) -> bool {
        if version.network != self.settings.network {
            return false;
        }

        // Check if this is our own nonce (self-connection)
        if version.nonce == self.nonce {
            return false;
        }

        let remote_ip = Self::normalize_ip(snapshot.remote_address);
        let config = self.config();
        let peers = self.read_peers();

        if config.max_connections > 0 && peers.len() >= config.max_connections {
            return false;
        }

        // SECURITY FIX (H-5): Use O(1) lookup instead of O(n) scan to prevent DoS
        // The previous implementation used filter().count() which was O(n) and could
        // be exploited by attackers to slow down connection acceptance.
        if config.max_connections_per_address > 0 {
            drop(peers); // Release peers lock before acquiring peers_by_ip lock
            let per_address = self
                .peers_by_ip
                .read()
                .get(&remote_ip)
                .map(|addrs| addrs.len())
                .unwrap_or(0);

            if per_address >= config.max_connections_per_address {
                return false;
            }
        } else {
            drop(peers);
        }
        let nodes = self.read_remote_nodes();
        let duplicate = nodes.values().any(|entry| {
            let existing_ip = Self::normalize_ip(entry.snapshot.remote_address);
            existing_ip == remote_ip && entry.version.nonce == version.nonce
        });
        drop(nodes);

        !duplicate
    }

    fn normalize_ip(addr: SocketAddr) -> IpAddr {
        match addr {
            SocketAddr::V6(v6) => v6
                .ip()
                .to_ipv4()
                .map(IpAddr::V4)
                .unwrap_or_else(|| IpAddr::V6(*v6.ip())),
            SocketAddr::V4(v4) => IpAddr::V4(*v4.ip()),
        }
    }

    /// Registers a remote node actor and synchronises its snapshot with the peer registry.
    pub fn register_remote_node(
        &self,
        actor: ActorRef,
        snapshot: RemoteNodeSnapshot,
        version: VersionPayload,
    ) {
        let path_key = actor.path().to_string();

        {
            let mut peers = self.write_peers();
            peers.insert(snapshot.remote_address, snapshot.clone());
        }

        self.clear_pending(&snapshot.remote_address);

        let mut nodes = self.write_remote_nodes();
        nodes.insert(
            path_key,
            RemoteActorEntry {
                actor,
                snapshot,
                version,
            },
        );
    }

    /// Removes the remote node entry and associated peer snapshot.
    pub fn unregister_remote_node(&self, actor: &ActorRef) -> Option<RemoteNodeSnapshot> {
        let path_key = actor.path().to_string();
        let entry = {
            let mut nodes = self.write_remote_nodes();
            nodes.remove(&path_key)
        };

        if let Some(entry) = entry {
            let mut peers = self.write_peers();
            peers.remove(&entry.snapshot.remote_address);
            self.clear_pending(&entry.snapshot.remote_address);
            Some(entry.snapshot)
        } else {
            None
        }
    }

    /// Returns the actor references for all registered remote nodes.
    pub fn remote_actor_refs(&self) -> Vec<ActorRef> {
        self.read_remote_nodes()
            .values()
            .map(|entry| entry.actor.clone())
            .collect()
    }

    fn read_peers(&self) -> RwLockReadGuard<'_, HashMap<SocketAddr, RemoteNodeSnapshot>> {
        self.peers.read()
    }

    fn write_peers(&self) -> RwLockWriteGuard<'_, HashMap<SocketAddr, RemoteNodeSnapshot>> {
        self.peers.write()
    }

    fn read_remote_nodes(&self) -> RwLockReadGuard<'_, HashMap<String, RemoteActorEntry>> {
        self.remote_nodes.read()
    }

    fn write_remote_nodes(&self) -> RwLockWriteGuard<'_, HashMap<String, RemoteActorEntry>> {
        self.remote_nodes.write()
    }

    pub fn track_pending(&self, endpoint: SocketAddr) {
        self.pending_connections.write().insert(endpoint);
    }

    pub fn clear_pending(&self, endpoint: &SocketAddr) {
        self.pending_connections.write().remove(endpoint);
    }

    pub fn is_pending(&self, endpoint: &SocketAddr) -> bool {
        self.pending_connections.read().contains(endpoint)
    }

    // SECURITY: Rate limiting, ban list, and reputation methods

    /// Checks if an inbound connection should be allowed based on rate limits.
    /// Returns true if the connection is within the allowed rate.
    pub fn check_inbound_rate_limit(&self) -> bool {
        self.inbound_rate_limiter.write().acquire()
    }

    /// Returns the current number of available tokens in the rate limiter.
    pub fn inbound_rate_limit_tokens(&self) -> f64 {
        self.inbound_rate_limiter.read().available_tokens()
    }

    /// Checks if the given IP address is banned.
    pub fn is_ip_banned(&self, ip: &IpAddr) -> bool {
        self.ban_list.read().is_banned(ip)
    }

    /// Bans a peer for the specified duration with a reason.
    pub fn ban_peer(&self, ip: IpAddr, duration: Duration, reason: impl Into<String>) {
        let reason_str: String = reason.into();
        warn!(target: "neo", ip = %ip, reason = %reason_str, "banning peer");
        self.ban_list.write().ban(ip, duration, reason_str);
    }

    /// Unbans a peer and returns true if they were banned.
    pub fn unban_peer(&self, ip: &IpAddr) -> bool {
        self.ban_list.write().unban(ip)
    }

    /// Returns the number of active bans.
    pub fn active_ban_count(&self) -> usize {
        self.ban_list.read().active_ban_count()
    }

    /// Cleans up expired bans and returns the count of removed entries.
    pub fn cleanup_expired_bans(&self) -> usize {
        self.ban_list.write().cleanup_expired()
    }

    /// Returns a reference to the reputation tracker.
    pub fn reputation_tracker(&self) -> Arc<PeerReputationTracker> {
        Arc::clone(&self.reputation_tracker)
    }

    /// Validates if a connection should be accepted based on all security checks.
    /// Returns Ok(()) if the connection is allowed, Err(reason) if rejected.
    pub fn validate_inbound_connection(&self, remote: &SocketAddr) -> Result<(), &'static str> {
        let ip = remote.ip();

        // Check rate limit
        if !self.check_inbound_rate_limit() {
            return Err("rate limit exceeded");
        }

        // Check ban list
        if self.is_ip_banned(&ip) {
            return Err("peer is banned");
        }

        // Check existing connection count against limit
        let config = self.config();
        if config.max_connections > 0 {
            let current = self.connected_peers_count();
            if current >= config.max_connections {
                return Err("connection limit reached");
            }
        }

        Ok(())
    }

    /// Returns actor properties matching the C# `LocalNode.Props` helper.
    pub fn props(state: Arc<Self>) -> Props {
        Props::new(move || LocalNodeActor::new(state.clone()))
    }
}

impl Default for LocalNode {
    fn default() -> Self {
        Self::new(
            Arc::new(ProtocolSettings::default()),
            10333,
            "neo-rust".to_string(),
        )
    }
}
