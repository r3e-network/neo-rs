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
    UnconnectedCount {
        reply: oneshot::Sender<usize>,
    },
    GetUnconnectedPeers {
        reply: oneshot::Sender<Vec<SocketAddr>>,
    },
    AddUnconnectedPeers {
        endpoints: Vec<SocketAddr>,
    },
    InboundTcpAccepted {
        stream: TcpStream,
        remote: SocketAddr,
        local: SocketAddr,
    },
}
