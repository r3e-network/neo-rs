// Copyright (C) 2015-2025 The Neo Project.
//
// local_node.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    capabilities::NodeCapability,
    channels_config::ChannelsConfig,
    connection::PeerConnection,
    peer::{PeerCommand, PeerState, PeerTimer, MAX_COUNT_FROM_SEED_LIST},
    remote_node::{RemoteNode, RemoteNodeCommand},
};
use crate::neo_io::{BinaryWriter, Serializable};
use crate::network::p2p::payloads::{
    addr_payload::MAX_COUNT_TO_SEND, block::Block, extensible_payload::ExtensiblePayload,
    inventory_type::InventoryType, network_address_with_time::NetworkAddressWithTime,
    transaction::Transaction, VersionPayload,
};
use crate::network::p2p::{NetworkMessage, ProtocolMessage};
use crate::{neo_system::NeoSystemContext, protocol_settings::ProtocolSettings};
use akka::{Actor, ActorContext, ActorRef, ActorResult, Props, Terminated};
use async_trait::async_trait;
use rand::{seq::IteratorRandom, thread_rng};
use std::any::{type_name_of_val, Any};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio::{
    net::{lookup_host, TcpListener, TcpStream},
    sync::{oneshot, Mutex},
};
use tracing::{debug, trace, warn};

/// The protocol version supported by this node implementation (matches C# LocalNode.ProtocolVersion).
pub const PROTOCOL_VERSION: u32 = 0;

/// Immutable snapshot of a remote peer used for API exposure (matches C# RemoteNodeModel source data).
#[derive(Debug, Clone)]
pub struct RemoteNodeSnapshot {
    /// Remote socket endpoint as seen by the transport layer.
    pub remote_address: SocketAddr,
    /// Remote TCP port reported by the peer.
    pub remote_port: u16,
    /// Remote listener TCP port (advertised to the network).
    pub listen_tcp_port: u16,
    /// Last block height reported by the peer.
    pub last_block_index: u32,
    /// Protocol version of the peer.
    pub version: u32,
    /// Service bitmask advertised by the peer.
    pub services: u64,
    /// Unix timestamp (seconds) when the snapshot was captured.
    pub timestamp: u64,
}

impl RemoteNodeSnapshot {
    /// Updates the last block height and refreshes the timestamp.
    fn touch(&mut self, last_block_index: u32, timestamp: u64) {
        self.last_block_index = last_block_index;
        self.timestamp = timestamp;
    }
}

/// Represents the local node in the P2P network (mirrors C# Neo.Network.P2P.LocalNode).
#[derive(Debug)]
pub struct LocalNode {
    /// Runtime protocol settings shared with the wider system.
    settings: Arc<ProtocolSettings>,
    /// Random nonce identifying this node instance.
    pub nonce: u32,
    /// User agent advertised during version handshake.
    pub user_agent: String,
    /// Listening port for inbound connections.
    port: RwLock<u16>,
    /// Supported node capabilities.
    capabilities: Arc<RwLock<Vec<NodeCapability>>>,
    /// Connected peers keyed by their remote socket address.
    peers: Arc<RwLock<HashMap<SocketAddr, RemoteNodeSnapshot>>>,
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
}

#[derive(Debug, Clone)]
struct RemoteActorEntry {
    actor: ActorRef,
    snapshot: RemoteNodeSnapshot,
    version: VersionPayload,
}

impl LocalNode {
    pub const PROTOCOL_VERSION: u32 = 0;

    /// Creates a new local node matching the behaviour of the C# constructor.
    pub fn new(settings: Arc<ProtocolSettings>, port: u16, user_agent: String) -> Self {
        Self {
            settings,
            nonce: rand::random(),
            user_agent,
            port: RwLock::new(port),
            capabilities: Arc::new(RwLock::new(vec![
                NodeCapability::tcp_server(port),
                NodeCapability::full_node(0),
            ])),
            peers: Arc::new(RwLock::new(HashMap::new())),
            remote_nodes: Arc::new(RwLock::new(HashMap::new())),
            broadcasts: Arc::new(RwLock::new(Vec::new())),
            seed_list: Arc::new(RwLock::new(Vec::new())),
            pending_connections: Arc::new(RwLock::new(HashSet::new())),
            system_context: RwLock::new(None),
        }
    }

    /// Adds a capability to the node.
    pub fn add_capability(&self, capability: NodeCapability) {
        let mut guard = self
            .capabilities
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.push(capability);
    }

    /// Returns the number of connected peers.
    pub fn connected_peers_count(&self) -> usize {
        self.read_peers().len()
    }

    /// Adds or updates a connected peer snapshot.
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
        let mut peers = self.write_peers();
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
    }

    /// Updates the last known block height for the specified peer.
    pub fn update_peer_height(&self, remote_address: &SocketAddr, last_block_index: u32) {
        let timestamp = current_unix_timestamp();
        if let Some(snapshot) = self.write_peers().get_mut(remote_address) {
            snapshot.touch(last_block_index, timestamp);
        }
    }

    /// Removes a peer from the local registry.
    pub fn remove_peer(&self, address: &SocketAddr) -> bool {
        self.write_peers().remove(address).is_some()
    }

    /// Returns the list of connected peer endpoints.
    pub fn get_peers(&self) -> Vec<SocketAddr> {
        self.read_peers().keys().copied().collect()
    }

    /// Returns detailed snapshots of connected peers, mirroring the data exposed by C# RemoteNode.
    pub fn remote_nodes(&self) -> Vec<RemoteNodeSnapshot> {
        self.read_peers().values().cloned().collect()
    }

    fn remote_entries(&self) -> Vec<RemoteActorEntry> {
        self.remote_nodes
            .read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default()
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
        self.broadcasts
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Replaces the seed list used when requesting new peers.
    pub fn set_seed_list<S>(&self, seeds: S)
    where
        S: Into<Vec<String>>,
    {
        let mut guard = self
            .seed_list
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = seeds.into();
    }

    /// Returns the configured seed list.
    pub fn seed_list(&self) -> Vec<String> {
        self.seed_list.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Returns the TCP listening port set for this node.
    pub fn port(&self) -> u16 {
        self.port.read().map(|port| *port).unwrap_or(0)
    }

    /// Updates the TCP listening port.
    pub fn set_port(&self, port: u16) {
        if let Ok(mut guard) = self.port.write() {
            *guard = port;
        }
    }

    /// Associates the Neo system context with this local node.
    pub fn set_system_context(&self, context: Arc<NeoSystemContext>) {
        if let Ok(mut guard) = self.system_context.write() {
            *guard = Some(context);
        }
    }

    /// Returns the previously attached system context if available.
    pub fn system_context(&self) -> Option<Arc<NeoSystemContext>> {
        self.system_context
            .read()
            .map(|guard| guard.as_ref().cloned())
            .unwrap_or(None)
    }

    /// Provides a handle to the shared protocol settings.
    pub fn settings(&self) -> Arc<ProtocolSettings> {
        Arc::clone(&self.settings)
    }

    /// Generates the version payload broadcast during handshake.
    pub fn version_payload(&self) -> VersionPayload {
        let mut capabilities = self
            .capabilities
            .read()
            .map(|g| g.clone())
            .unwrap_or_default();

        let current_height = self
            .system_context()
            .map(|context| context.current_block_index())
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
            self.settings.network,
            self.nonce,
            self.user_agent.clone(),
            capabilities,
        )
    }

    /// Returns a list of network addresses used to respond to `GetAddr` messages.
    pub fn address_book(&self) -> Vec<NetworkAddressWithTime> {
        let guard = match self.remote_nodes.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };

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
        if let Some(endpoint) = config.tcp {
            self.set_port(endpoint.port());
        }

        let mut capabilities = self
            .capabilities
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

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
        }

        // Toggle compression capability.
        capabilities.retain(|cap| !matches!(cap, NodeCapability::DisableCompression));
        if !config.enable_compression {
            capabilities.push(NodeCapability::disable_compression());
        }
    }

    fn record_broadcast(&self, event: BroadcastEvent) {
        if let Ok(mut guard) = self.broadcasts.write() {
            guard.push(event);
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

        if version.nonce == self.nonce {
            return false;
        }

        let remote_ip = Self::normalize_ip(snapshot.remote_address);
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
        match self.peers.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn write_peers(&self) -> RwLockWriteGuard<'_, HashMap<SocketAddr, RemoteNodeSnapshot>> {
        match self.peers.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn read_remote_nodes(&self) -> RwLockReadGuard<'_, HashMap<String, RemoteActorEntry>> {
        match self.remote_nodes.read() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn write_remote_nodes(&self) -> RwLockWriteGuard<'_, HashMap<String, RemoteActorEntry>> {
        match self.remote_nodes.write() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn track_pending(&self, endpoint: SocketAddr) {
        let mut guard = self
            .pending_connections
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(endpoint);
    }

    pub fn clear_pending(&self, endpoint: &SocketAddr) {
        if let Ok(mut guard) = self.pending_connections.write() {
            guard.remove(endpoint);
        }
    }

    pub fn is_pending(&self, endpoint: &SocketAddr) -> bool {
        self.pending_connections
            .read()
            .map(|guard| guard.contains(endpoint))
            .unwrap_or(false)
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

/// Captures different broadcast intents executed by the local node actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastEvent {
    Relay(Vec<u8>),
    Direct(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum RelayInventory {
    Block(Block),
    Transaction(Transaction),
    Extensible(ExtensiblePayload),
}

impl RelayInventory {
    pub fn inventory_type(&self) -> InventoryType {
        match self {
            RelayInventory::Block(_) => InventoryType::Block,
            RelayInventory::Transaction(_) => InventoryType::Transaction,
            RelayInventory::Extensible(_) => InventoryType::Extensible,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        match self {
            RelayInventory::Block(block) => {
                Serializable::serialize(block, &mut writer)
                    .expect("failed to serialize block inventory");
            }
            RelayInventory::Transaction(tx) => {
                Serializable::serialize(tx, &mut writer)
                    .expect("failed to serialize transaction inventory");
            }
            RelayInventory::Extensible(payload) => {
                Serializable::serialize(payload, &mut writer)
                    .expect("failed to serialize extensible inventory");
            }
        }
        writer.into_bytes()
    }
}

fn current_unix_timestamp() -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs(),
        Err(error) => {
            // System time is before UNIX_EPOCH; fall back to zero to preserve monotonicity.
            let duration: Duration = error.duration();
            duration.as_secs()
        }
    }
}

fn parse_seed_entry(entry: &str) -> Option<(String, u16)> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (host, port_str) = match trimmed.rsplit_once(':') {
        Some(split) => split,
        None => return None,
    };

    if host.is_empty() {
        return None;
    }

    let port: u16 = port_str.parse().ok()?;
    Some((host.to_string(), port))
}

impl LocalNode {
    /// Returns actor properties matching the C# `LocalNode.Props` helper.
    pub fn props(state: Arc<Self>) -> Props {
        Props::new(move || LocalNodeActor::new(state.clone()))
    }
}

/// Actor responsible for orchestrating peer management, mirroring C# `LocalNode` behaviour.
pub struct LocalNodeActor {
    state: Arc<LocalNode>,
    peer: PeerState,
    listener: Option<JoinHandle<()>>,
}

impl LocalNodeActor {
    /// Creates a new actor wrapping the provided shared state.
    pub fn new(state: Arc<LocalNode>) -> Self {
        let peer = PeerState::new(state.port());
        Self {
            state,
            peer,
            listener: None,
        }
    }

    async fn handle_peer_command(
        &mut self,
        command: PeerCommand,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match command {
            PeerCommand::Configure { config } => {
                self.peer.configure(config, ctx);
                self.state.apply_channels_config(self.peer.config());
                self.start_listener(ctx);
                self.handle_peer_timer(ctx).await
            }
            PeerCommand::AddPeers { endpoints } => {
                self.peer.add_unconnected_peers(endpoints);
                Ok(())
            }
            PeerCommand::Connect {
                endpoint,
                is_trusted,
            } => {
                if self.peer.begin_connect(endpoint, is_trusted) {
                    if self.state.is_pending(&endpoint) {
                        return Ok(());
                    }
                    self.state.track_pending(endpoint);
                    self.initiate_connect(ctx, endpoint, is_trusted).await?;
                }
                Ok(())
            }
            PeerCommand::ConnectionEstablished {
                actor,
                snapshot,
                is_trusted,
                inbound,
                version,
                reply,
            } => {
                let allowed = self.state.allow_new_connection(&snapshot, &version);
                if allowed {
                    self.peer
                        .register_connection(actor.clone(), &snapshot, is_trusted, ctx);
                    self.state.register_remote_node(
                        actor.clone(),
                        snapshot.clone(),
                        version.clone(),
                    );
                    self.state.add_peer(
                        snapshot.remote_address,
                        Some(snapshot.listen_tcp_port),
                        version.version,
                        snapshot.services,
                        snapshot.last_block_index,
                    );
                    let _ = reply.send(true);
                } else {
                    debug!(
                        target: "neo",
                        remote = %snapshot.remote_address,
                        "connection rejected based on local node policy"
                    );
                    if !inbound {
                        self.state.clear_pending(&snapshot.remote_address);
                    }
                    let _ = reply.send(false);
                }
                Ok(())
            }
            PeerCommand::ConnectionFailed { endpoint } => {
                let was_pending = self.state.is_pending(&endpoint);
                self.peer.connection_failed(endpoint);
                self.state.clear_pending(&endpoint);
                if was_pending {
                    self.requeue_endpoint(endpoint);
                }
                Ok(())
            }
            PeerCommand::ConnectionTerminated { actor } => self.handle_terminated(actor, ctx).await,
            PeerCommand::TimerElapsed => self.handle_peer_timer(ctx).await,
            PeerCommand::QueryConnectingPeers { reply } => {
                let _ = reply.send(self.peer.connecting_endpoints());
                Ok(())
            }
        }
    }

    async fn handle_local_command(
        &mut self,
        command: LocalNodeCommand,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match command {
            LocalNodeCommand::AddPeer {
                remote_address,
                listener_tcp_port,
                version,
                services,
                last_block_index,
            } => {
                self.state.add_peer(
                    remote_address,
                    listener_tcp_port,
                    version,
                    services,
                    last_block_index,
                );
            }
            LocalNodeCommand::UpdatePeerHeight {
                remote_address,
                last_block_index,
            } => {
                self.state
                    .update_peer_height(&remote_address, last_block_index);
            }
            LocalNodeCommand::RemovePeer {
                remote_address,
                reply,
            } => {
                let removed = self.state.remove_peer(&remote_address);
                let _ = reply.send(removed);
            }
            LocalNodeCommand::GetPeers { reply } => {
                let peers = self.state.get_peers();
                let _ = reply.send(peers);
            }
            LocalNodeCommand::GetRemoteNodes { reply } => {
                let nodes = self.state.remote_nodes();
                let _ = reply.send(nodes);
            }
            LocalNodeCommand::PeerCount { reply } => {
                let count = self.state.connected_peers_count();
                let _ = reply.send(count);
            }
            LocalNodeCommand::GetInstance { reply } => {
                let _ = reply.send(self.state.clone());
            }
            LocalNodeCommand::RelayDirectly {
                inventory,
                block_index,
            } => {
                self.state.record_relay(&inventory);
                self.send_inventory_to_peers(&inventory, block_index, true);
            }
            LocalNodeCommand::SendDirectly {
                inventory,
                block_index,
            } => {
                self.state.record_send(&inventory);
                self.send_inventory_to_peers(&inventory, block_index, false);
            }
            LocalNodeCommand::RegisterRemoteNode {
                actor,
                snapshot,
                version,
            } => {
                self.state.register_remote_node(actor, snapshot, version);
            }
            LocalNodeCommand::UnregisterRemoteNode { actor } => {
                self.state.unregister_remote_node(&actor);
            }
            LocalNodeCommand::GetRemoteActors { reply } => {
                let actors = self.state.remote_actor_refs();
                let _ = reply.send(actors);
            }
            LocalNodeCommand::InboundTcpAccepted {
                stream,
                remote,
                local,
            } => {
                self.spawn_remote(ctx, stream, remote, local, false, true)
                    .await?;
            }
        }
        Ok(())
    }

    async fn handle_peer_timer(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let deficit = self.peer.connection_deficit();
        if deficit == 0 {
            return Ok(());
        }

        if self.peer.connecting_capacity() == 0 {
            return Ok(());
        }

        if !self.peer.has_unconnected_peers() {
            self.need_more_peers(ctx, deficit).await?;
        }

        let targets = self.peer.take_connect_targets(deficit);
        for endpoint in targets {
            if self.peer.begin_connect(endpoint, false) {
                if self.state.is_pending(&endpoint) {
                    continue;
                }
                self.state.track_pending(endpoint);
                self.initiate_connect(ctx, endpoint, false).await?;
            } else {
                self.requeue_endpoint(endpoint);
            }
        }

        Ok(())
    }

    async fn handle_terminated(&mut self, actor: ActorRef, ctx: &mut ActorContext) -> ActorResult {
        if let Err(error) = ctx.unwatch(&actor) {
            trace!(target: "neo", error = %error, "failed to unwatch remote node");
        }

        if let Some((_, remote_endpoint)) = self.peer.unregister_connection(&actor) {
            let was_pending = self.state.is_pending(&remote_endpoint);
            self.state.remove_peer(&remote_endpoint);
            self.state.clear_pending(&remote_endpoint);
            if was_pending {
                self.requeue_endpoint(remote_endpoint);
            }
        }

        self.state.unregister_remote_node(&actor);
        Ok(())
    }

    async fn initiate_connect(
        &mut self,
        ctx: &mut ActorContext,
        endpoint: SocketAddr,
        is_trusted: bool,
    ) -> ActorResult {
        match TcpStream::connect(endpoint).await {
            Ok(stream) => {
                if let Err(err) = stream.set_nodelay(true) {
                    warn!(target: "neo", endpoint = %endpoint, error = %err, "failed to enable TCP_NODELAY");
                }

                let local_endpoint = stream
                    .local_addr()
                    .unwrap_or_else(|_| "0.0.0.0:0".parse().expect("valid socket address"));

                self.spawn_remote(ctx, stream, endpoint, local_endpoint, is_trusted, false)
                    .await
            }
            Err(error) => {
                debug!(target: "neo", endpoint = %endpoint, error = %error, "connection attempt failed");
                self.peer.connection_failed(endpoint);
                self.state.clear_pending(&endpoint);
                self.requeue_endpoint(endpoint);
                Ok(())
            }
        }
    }

    fn send_inventory_to_peers(
        &self,
        inventory: &RelayInventory,
        block_index: Option<u32>,
        restrict_block_height: bool,
    ) {
        match inventory {
            RelayInventory::Block(block) => {
                let target_index = block_index.unwrap_or(block.index());
                for entry in self.state.remote_entries() {
                    if restrict_block_height && entry.snapshot.last_block_index >= target_index {
                        continue;
                    }

                    let message = NetworkMessage::new(ProtocolMessage::Block(block.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay block to peer"
                        );
                    }
                }
            }
            RelayInventory::Transaction(tx) => {
                for entry in self.state.remote_entries() {
                    let message = NetworkMessage::new(ProtocolMessage::Transaction(tx.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay transaction to peer"
                        );
                    }
                }
            }
            RelayInventory::Extensible(payload) => {
                for entry in self.state.remote_entries() {
                    let message = NetworkMessage::new(ProtocolMessage::Extensible(payload.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay extensible payload to peer"
                        );
                    }
                }
            }
        }
    }

    async fn spawn_remote(
        &mut self,
        ctx: &mut ActorContext,
        stream: TcpStream,
        remote: SocketAddr,
        local: SocketAddr,
        is_trusted: bool,
        inbound: bool,
    ) -> ActorResult {
        let connection = Arc::new(Mutex::new(PeerConnection::new(stream, remote, inbound)));
        let actor_name = format!("remote-{:016x}", rand::random::<u64>());

        let version_payload = self.state.version_payload();
        let settings = self.state.settings();
        let config = self.peer.config().clone();
        let Some(system_context) = self.state.system_context() else {
            warn!(target: "neo", "system context missing when spawning remote node");
            return Ok(());
        };

        let props = RemoteNode::props(
            Arc::clone(&system_context),
            Arc::clone(&self.state),
            Arc::clone(&connection),
            remote,
            local,
            version_payload,
            settings,
            config,
            is_trusted,
            inbound,
        );

        match ctx.actor_of(props, actor_name) {
            Ok(actor) => {
                if let Err(err) = actor.tell(RemoteNodeCommand::StartProtocol) {
                    warn!(target: "neo", endpoint = %remote, error = %err, "failed to start protocol");
                    if !inbound {
                        self.peer.connection_failed(remote);
                        self.state.clear_pending(&remote);
                        self.requeue_endpoint(remote);
                    }
                }
                Ok(())
            }
            Err(err) => {
                warn!(target: "neo", endpoint = %remote, error = %err, "failed to spawn remote node actor");
                if !inbound {
                    self.peer.connection_failed(remote);
                    self.state.clear_pending(&remote);
                    self.requeue_endpoint(remote);
                }
                Ok(())
            }
        }
    }

    async fn need_more_peers(&mut self, _ctx: &mut ActorContext, count: usize) -> ActorResult {
        let requested = count.max(MAX_COUNT_FROM_SEED_LIST);

        if self.peer.connected_count() > 0 {
            trace!(target: "neo", requested, "requesting additional peers from network");
            return Ok(());
        }

        let seeds = self.resolve_seed_endpoints().await;
        if seeds.is_empty() {
            warn!(target: "neo", "no seeds available to satisfy peer request");
            return Ok(());
        }

        let mut rng = thread_rng();
        let selection: Vec<_> = seeds
            .iter()
            .copied()
            .choose_multiple(&mut rng, requested.min(seeds.len()));

        if selection.is_empty() {
            return Ok(());
        }

        self.peer.add_unconnected_peers(selection);
        Ok(())
    }

    async fn resolve_seed_endpoints(&self) -> Vec<SocketAddr> {
        let mut endpoints = Vec::new();
        for entry in self.state.seed_list() {
            if let Some((host, port)) = parse_seed_entry(&entry) {
                match lookup_host((host.as_str(), port)).await {
                    Ok(iter) => {
                        for addr in iter {
                            endpoints.push(addr);
                        }
                    }
                    Err(error) => {
                        warn!(target: "neo", seed = %entry, error = %error, "failed to resolve seed");
                    }
                }
            }
        }
        endpoints
    }

    fn requeue_endpoint(&mut self, endpoint: SocketAddr) {
        self.peer.add_unconnected_peers([endpoint]);
    }

    fn start_listener(&mut self, ctx: &ActorContext) {
        if let Some(handle) = self.listener.take() {
            handle.abort();
        }

        let Some(endpoint) = self.peer.config().tcp else {
            return;
        };

        let actor_ref = ctx.self_ref();
        self.listener = Some(tokio::spawn(async move {
            match TcpListener::bind(endpoint).await {
                Ok(listener) => loop {
                    match listener.accept().await {
                        Ok((stream, remote)) => {
                            let local = stream.local_addr().unwrap_or(endpoint);
                            if let Err(err) = stream.set_nodelay(true) {
                                warn!(target: "neo", endpoint = %remote, error = %err, "failed to enable TCP_NODELAY for inbound connection");
                            }
                            if let Err(err) = actor_ref.tell(LocalNodeCommand::InboundTcpAccepted {
                                stream,
                                remote,
                                local,
                            }) {
                                warn!(target: "neo", error = %err, "failed to enqueue inbound connection");
                            }
                        }
                        Err(error) => {
                            warn!(target: "neo", error = %error, "failed to accept inbound connection");
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                },
                Err(error) => {
                    warn!(target: "neo", endpoint = %endpoint, error = %error, "failed to bind TCP listener");
                }
            }
        }));
    }
}

#[async_trait]
impl Actor for LocalNodeActor {
    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<PeerCommand>() {
            Ok(command) => self.handle_peer_command(*command, ctx).await,
            Err(payload) => match payload.downcast::<LocalNodeCommand>() {
                Ok(command) => self.handle_local_command(*command, ctx).await,
                Err(payload) => match payload.downcast::<PeerTimer>() {
                    Ok(_) => self.handle_peer_timer(ctx).await,
                    Err(payload) => match payload.downcast::<Terminated>() {
                        Ok(terminated) => self.handle_terminated(terminated.actor, ctx).await,
                        Err(payload) => {
                            warn!(
                                target: "neo",
                                message_type = %type_name_of_val(payload.as_ref()),
                                "unknown message routed to local node actor"
                            );
                            Ok(())
                        }
                    },
                },
            },
        }
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        self.peer.cancel_timer();
        if let Some(handle) = self.listener.take() {
            handle.abort();
        }
        Ok(())
    }
}

/// Message types accepted by [`LocalNodeActor`].
#[derive(Debug)]
pub enum LocalNodeCommand {
    AddPeer {
        remote_address: SocketAddr,
        listener_tcp_port: Option<u16>,
        version: u32,
        services: u64,
        last_block_index: u32,
    },
    UpdatePeerHeight {
        remote_address: SocketAddr,
        last_block_index: u32,
    },
    RemovePeer {
        remote_address: SocketAddr,
        reply: oneshot::Sender<bool>,
    },
    GetPeers {
        reply: oneshot::Sender<Vec<SocketAddr>>,
    },
    GetRemoteNodes {
        reply: oneshot::Sender<Vec<RemoteNodeSnapshot>>,
    },
    PeerCount {
        reply: oneshot::Sender<usize>,
    },
    GetInstance {
        reply: oneshot::Sender<Arc<LocalNode>>,
    },
    RelayDirectly {
        inventory: RelayInventory,
        block_index: Option<u32>,
    },
    SendDirectly {
        inventory: RelayInventory,
        block_index: Option<u32>,
    },
    RegisterRemoteNode {
        actor: ActorRef,
        snapshot: RemoteNodeSnapshot,
        version: VersionPayload,
    },
    UnregisterRemoteNode {
        actor: ActorRef,
    },
    GetRemoteActors {
        reply: oneshot::Sender<Vec<ActorRef>>,
    },
    InboundTcpAccepted {
        stream: TcpStream,
        remote: SocketAddr,
        local: SocketAddr,
    },
}
