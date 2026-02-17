//
// types.rs - Type definitions for local node
//

use super::*;

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
    pub(super) fn touch(&mut self, last_block_index: u32, timestamp: u64) {
        self.last_block_index = last_block_index;
        self.timestamp = timestamp;
    }
}

/// Captures different broadcast intents executed by the local node actor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BroadcastEvent {
    /// Relay broadcast to all connected peers.
    Relay(Vec<u8>),
    /// Direct broadcast to specific peers.
    Direct(Vec<u8>),
}

/// Inventory items that can be relayed across the P2P network.
#[derive(Debug, Clone)]
pub enum RelayInventory {
    /// A complete block to relay.
    Block(Block),
    /// A transaction to relay.
    Transaction(Transaction),
    /// An extensible payload (consensus, oracle, etc.).
    Extensible(ExtensiblePayload),
}

impl RelayInventory {
    /// Returns the inventory type for this relay item.
    pub fn inventory_type(&self) -> InventoryType {
        match self {
            RelayInventory::Block(_) => InventoryType::Block,
            RelayInventory::Transaction(_) => InventoryType::Transaction,
            RelayInventory::Extensible(_) => InventoryType::Extensible,
        }
    }

    /// Serializes the inventory item to bytes for network transmission.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        let result = match self {
            RelayInventory::Block(block) => Serializable::serialize(block, &mut writer),
            RelayInventory::Transaction(tx) => Serializable::serialize(tx, &mut writer),
            RelayInventory::Extensible(payload) => Serializable::serialize(payload, &mut writer),
        };
        if let Err(e) = result {
            tracing::error!("Failed to serialize inventory: {:?}", e);
            return Vec::new();
        }
        writer.into_bytes()
    }
}

/// Message types accepted by `LocalNodeActor`.
#[derive(Debug)]
pub enum LocalNodeCommand {
    /// Add a new peer to the connected peers list.
    AddPeer {
        /// Remote socket address.
        remote_address: SocketAddr,
        /// Optional listener TCP port.
        listener_tcp_port: Option<u16>,
        /// Protocol version.
        version: u32,
        /// Service bitmask.
        services: u64,
        /// Last known block index.
        last_block_index: u32,
    },
    /// Update the block height for an existing peer.
    UpdatePeerHeight {
        /// Remote socket address.
        remote_address: SocketAddr,
        /// New last block index.
        last_block_index: u32,
    },
    /// Remove a peer from the connected peers list.
    RemovePeer {
        /// Remote socket address.
        remote_address: SocketAddr,
        /// Reply channel for removal result.
        reply: oneshot::Sender<bool>,
    },
    /// Get list of connected peer addresses.
    GetPeers {
        /// Reply channel for peer addresses.
        reply: oneshot::Sender<Vec<SocketAddr>>,
    },
    /// Get snapshots of all remote nodes.
    GetRemoteNodes {
        /// Reply channel for remote node snapshots.
        reply: oneshot::Sender<Vec<RemoteNodeSnapshot>>,
    },
    /// Get the count of connected peers.
    PeerCount {
        /// Reply channel for peer count.
        reply: oneshot::Sender<usize>,
    },
    /// Get the LocalNode instance.
    GetInstance {
        /// Reply channel for LocalNode Arc.
        reply: oneshot::Sender<Arc<LocalNode>>,
    },
    /// Relay inventory to all connected peers.
    RelayDirectly {
        /// Inventory item to relay.
        inventory: RelayInventory,
        /// Optional block index context.
        block_index: Option<u32>,
    },
    /// Send inventory directly to specific peers.
    SendDirectly {
        /// Inventory item to send.
        inventory: RelayInventory,
        /// Optional block index context.
        block_index: Option<u32>,
    },
    /// Register a new remote node actor.
    RegisterRemoteNode {
        /// Actor reference for the remote node.
        actor: ActorRef,
        /// Snapshot of the remote node state.
        snapshot: RemoteNodeSnapshot,
        /// Version payload from handshake.
        version: VersionPayload,
    },
    /// Unregister a remote node actor.
    UnregisterRemoteNode {
        /// Actor reference to unregister.
        actor: ActorRef,
    },
    /// Get all remote node actor references.
    GetRemoteActors {
        /// Reply channel for actor references.
        reply: oneshot::Sender<Vec<ActorRef>>,
    },
    /// Get count of unconnected peers.
    UnconnectedCount {
        /// Reply channel for count.
        reply: oneshot::Sender<usize>,
    },
    /// Get list of unconnected peer addresses.
    GetUnconnectedPeers {
        /// Reply channel for addresses.
        reply: oneshot::Sender<Vec<SocketAddr>>,
    },
    /// Add endpoints to the unconnected peers list.
    AddUnconnectedPeers {
        /// Endpoints to add.
        endpoints: Vec<SocketAddr>,
    },
    /// Handle an accepted inbound TCP connection.
    InboundTcpAccepted {
        /// TCP stream for the connection.
        stream: TcpStream,
        /// Remote socket address.
        remote: SocketAddr,
        /// Local socket address.
        local: SocketAddr,
    },
    /// Outbound TCP connection established asynchronously.
    OutboundTcpConnected {
        /// TCP stream for the established connection.
        stream: TcpStream,
        /// Remote endpoint we dialed.
        endpoint: SocketAddr,
        /// Local endpoint bound for the connection.
        local: SocketAddr,
        /// Whether this outbound target is trusted.
        is_trusted: bool,
    },
    /// Outbound TCP connection attempt failed asynchronously.
    OutboundTcpFailed {
        /// Remote endpoint we attempted to dial.
        endpoint: SocketAddr,
        /// Whether this outbound target is trusted.
        is_trusted: bool,
        /// Human-readable error detail.
        error: String,
        /// Whether the failure was a timeout.
        timed_out: bool,
        /// Whether the failure was permission denied.
        permission_denied: bool,
    },
}
