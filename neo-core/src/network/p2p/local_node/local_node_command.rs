//! Message types accepted by `LocalNodeActor`.

use super::*;

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
