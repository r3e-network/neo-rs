//! Network-level command enum.
//!
//! The single command stream that feeds the
//! [`crate::LocalNodeService`] command loop. The
//! variants cover the *user-facing* surface (start, connect, broadcast,
//! disconnect, shutdown); per-peer commands live in
//! [`crate::remote_node::RemoteNodeCommand`] and are routed by the
//! local node service to the right per-peer task.

use std::net::SocketAddr;

use neo_payloads::{Block, ExtensiblePayload, Transaction};
use neo_primitives::{InventoryType, UInt256};
use tokio::sync::oneshot;

use crate::error::NetworkResult;
use crate::peer_id::PeerId;

/// Top-level command accepted by [`crate::LocalNodeService`].
///
/// Each variant is a single, self-contained request; the service
/// loop dispatches each one to a private `async fn` handler.
#[derive(Debug)]
pub enum NetworkCommand {
    /// Start the TCP listener on the given address. The reply
    /// resolves with the *actual* bound address (which differs from
    /// `bind_addr` when port `0` was requested) once the listener is
    /// bound and the accept loop has been spawned, or with
    /// `NetworkError::Io` if the bind failed.
    Start {
        /// Address to bind the TCP listener to.
        bind_addr: SocketAddr,
        /// Reply channel carrying the resolved listener address.
        reply: oneshot::Sender<NetworkResult<SocketAddr>>,
    },

    /// Connect to a remote peer. The reply resolves with the new
    /// peer's id once the outbound connection has been established
    /// and a `RemoteNodeService` has been spawned to drive it.
    ConnectPeer {
        /// Remote peer address to dial.
        addr: SocketAddr,
        /// Reply channel.
        reply: oneshot::Sender<NetworkResult<PeerId>>,
    },

    /// Disconnect a peer by id. The reply resolves once the
    /// per-peer service task has been signalled to shut down.
    DisconnectPeer {
        /// Identifier of the peer to disconnect.
        peer_id: PeerId,
        /// Reply channel.
        reply: oneshot::Sender<NetworkResult<()>>,
    },

    /// Broadcast a freshly persisted block to all connected peers.
    BroadcastBlock {
        /// The block to broadcast.
        block: Block,
    },

    /// Broadcast a transaction to all connected peers.
    BroadcastTransaction {
        /// The transaction to broadcast.
        transaction: Transaction,
    },

    /// Broadcast an extensible payload (dBFT consensus / state-root vote) to
    /// all connected peers (C# `LocalNode.RelayDirectly` for the consensus
    /// `ExtensiblePayload` inventory).
    BroadcastExtensible {
        /// The extensible payload to relay.
        payload: ExtensiblePayload,
    },

    /// Relay an inventory item to all connected peers.
    RelayInventory {
        /// Inventory hash.
        hash: UInt256,
    },

    /// Announce inventory (block/transaction hashes) to all connected peers
    /// via an `Inv` message (C# `LocalNode.RelayDirectly`: peers pull the full
    /// items they lack via `GetData`). Used to re-broadcast freshly-accepted
    /// transactions and blocks.
    BroadcastInv {
        /// The kind of inventory being announced.
        inventory_type: InventoryType,
        /// The announced hashes.
        hashes: Vec<UInt256>,
    },

    /// Update the locally advertised block height (C# ledger
    /// `CurrentIndex`). Advertised in version and ping payloads so peers can
    /// select this node for their own sync work. Fire-and-forget; driven by the
    /// ledger's block-imported events.
    SetBlockHeight {
        /// The new local block height.
        height: u32,
    },

    /// Request graceful shutdown of the entire service. The local
    /// node service will:
    ///
    /// 1. Signal every [`crate::remote_node::RemoteNodeService`] task
    ///    to shut down.
    /// 2. Drop its TCP listener.
    /// 3. Drop its command receiver, causing `run()` to return.
    Shutdown,
}
