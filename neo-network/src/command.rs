//! Network-level command enum.
//!
//! The single command stream that feeds the
//! [`crate::local_node::LocalNodeService`] command loop. The
//! variants cover the *user-facing* surface (start, connect, broadcast,
//! disconnect, shutdown); per-peer commands live in
//! [`crate::remote_node::RemoteNodeCommand`] and are routed by the
//! local node service to the right per-peer task.

use std::net::SocketAddr;

use neo_payloads::{Block, Transaction};
use neo_primitives::UInt256;
use tokio::sync::oneshot;

use crate::error::NetworkResult;
use crate::peer_id::PeerId;
use crate::remote_node::RemoteNodeHandle;

/// Top-level command accepted by [`crate::local_node::LocalNodeService`].
///
/// Each variant is a single, self-contained request; the service
/// loop dispatches each one to a private `async fn` handler.
#[derive(Debug)]
pub enum NetworkCommand {
    /// Start the TCP listener on the given address. The reply
    /// resolves once the listener is bound and the accept loop has
    /// been spawned, or with [`NetworkError::Io`] if the bind failed.
    Start {
        /// Address to bind the TCP listener to.
        bind_addr: SocketAddr,
        /// Reply channel.
        reply: oneshot::Sender<NetworkResult<()>>,
    },

    /// Connect to a remote peer. The reply resolves with the new
    /// peer's id once the outbound connection has been established
    /// and a [`RemoteNodeService`] has been spawned to drive it.
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

    /// Relay an inventory item to all connected peers.
    RelayInventory {
        /// Inventory hash.
        hash: UInt256,
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

/// Helper to send a `NetworkCommand` over an `mpsc::Sender` and
/// translate the `SendError` into a [`crate::error::NetworkError`].
pub(crate) async fn send(
    tx: &tokio::sync::mpsc::Sender<NetworkCommand>,
    cmd: NetworkCommand,
) -> NetworkResult<()> {
    tx.send(cmd)
        .await
        .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
}

/// Helper to send a `NetworkCommand` with a `oneshot` reply and
/// await the reply. The reply channel is constructed by the caller
/// (via `build(reply_tx)`) so the same pattern works for every
/// request/response command.
pub(crate) async fn ask<T>(
    tx: &tokio::sync::mpsc::Sender<NetworkCommand>,
    build: impl FnOnce(oneshot::Sender<NetworkResult<T>>) -> NetworkCommand,
) -> NetworkResult<T> {
    let (reply_tx, reply_rx) = oneshot::channel();
    send(tx, build(reply_tx)).await?;
    reply_rx
        .await
        .map_err(|_| crate::error::NetworkError::LocalShuttingDown)?
}

/// Suppress the unused-import warning that `RemoteNodeHandle` would
/// otherwise trigger when it is only used by future variants of
/// `NetworkCommand`.
#[allow(dead_code)]
fn _force_remote_handle_link(_h: &RemoteNodeHandle) {}
