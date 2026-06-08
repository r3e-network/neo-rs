//! Cheap-to-clone network service handle.
//!
//! The public, request/response API for the network service. Other
//! subsystems (RPC server, consensus driver, node startup) store a
//! `NetworkHandle` in their state and call its methods instead of
//! sending `NetworkCommand` variants directly.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use neo_payloads::{Block, Transaction};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::command::NetworkCommand;
use crate::error::{NetworkError, NetworkResult};
use crate::event::NetworkEvent;
use crate::peer_id::PeerId;

/// Cheap-to-clone handle to a running [`crate::local_node::LocalNodeService`].
///
/// The handle is `Clone`, `Send`, and `Sync`. The two channels are
/// the only state it owns: an `mpsc::Sender<NetworkCommand>` for
/// user-facing requests and a `broadcast::Sender<NetworkEvent>` for
/// receiving lifecycle / payload events.
#[derive(Clone)]
pub struct NetworkHandle {
    /// Command channel sender.
    cmd_tx: mpsc::Sender<NetworkCommand>,
    /// Event broadcast sender.
    event_tx: broadcast::Sender<NetworkEvent>,
}

impl fmt::Debug for NetworkHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkHandle")
            .field("cmd_capacity", &self.cmd_tx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl NetworkHandle {
    /// Build a `(handle, command-receiver, event-sender)` triple.
    ///
    /// The caller is expected to spawn the local node service loop
    /// on the returned `mpsc::Receiver<NetworkCommand>`, and to use
    /// the returned `broadcast::Sender<NetworkEvent>` (or hand it to
    /// the loop) to publish events.
    pub fn channel(
        cmd_capacity: usize,
        event_capacity: usize,
    ) -> (
        Self,
        mpsc::Receiver<NetworkCommand>,
        broadcast::Sender<NetworkEvent>,
    ) {
        let (cmd_tx, cmd_rx) = mpsc::channel(cmd_capacity);
        let (event_tx, _event_rx) = broadcast::channel(event_capacity);
        let handle = Self::from_parts(cmd_tx, event_tx.clone());
        (handle, cmd_rx, event_tx)
    }

    /// Construct a handle from its two channel halves. The
    /// counterpart to `channel` for callers that already own the
    /// sender / broadcast-sender pair.
    pub fn from_parts(
        cmd_tx: mpsc::Sender<NetworkCommand>,
        event_tx: broadcast::Sender<NetworkEvent>,
    ) -> Self {
        Self { cmd_tx, event_tx }
    }

    /// Subscribe to network events. Each call returns an independent
    /// receiver; dropping the receiver unregisters the subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<NetworkEvent> {
        self.event_tx.subscribe()
    }

    /// Current number of event subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.event_tx.receiver_count()
    }

    /// Start the TCP listener on the given address. Resolves once
    /// the listener is bound and the accept loop is running, or with
    /// an [`NetworkError::Io`] on failure.
    pub async fn start(&self, bind_addr: SocketAddr) -> NetworkResult<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(NetworkCommand::Start {
                bind_addr,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        reply_rx.await.map_err(|_| NetworkError::LocalShuttingDown)?
    }

    /// Connect to a remote peer. Resolves with the new peer's id.
    pub async fn connect_peer(&self, addr: SocketAddr) -> NetworkResult<PeerId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(NetworkCommand::ConnectPeer {
                addr,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        reply_rx.await.map_err(|_| NetworkError::LocalShuttingDown)?
    }

    /// Disconnect a peer by id.
    pub async fn disconnect_peer(&self, peer_id: PeerId) -> NetworkResult<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(NetworkCommand::DisconnectPeer {
                peer_id,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        reply_rx.await.map_err(|_| NetworkError::LocalShuttingDown)?
    }

    /// Broadcast a block to all connected peers.
    pub async fn broadcast_block(&self, block: Block) -> NetworkResult<()> {
        self.cmd_tx
            .send(NetworkCommand::BroadcastBlock { block })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Broadcast a transaction to all connected peers.
    pub async fn broadcast_transaction(&self, transaction: Transaction) -> NetworkResult<()> {
        self.cmd_tx
            .send(NetworkCommand::BroadcastTransaction { transaction })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Relay an inventory item to all connected peers.
    pub async fn relay_inventory(&self, hash: neo_primitives::UInt256) -> NetworkResult<()> {
        self.cmd_tx
            .send(NetworkCommand::RelayInventory { hash })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Request graceful shutdown of the service.
    pub async fn shutdown(&self) -> NetworkResult<()> {
        self.cmd_tx
            .send(NetworkCommand::Shutdown)
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)
    }

    /// Internal: drop a fresh clone of the command sender, leaving
    /// the service's run loop to exit once all other senders are
    /// dropped too.
    pub async fn drop_self(&self) {
        let tx = self.cmd_tx.clone();
        drop(tx);
    }
}

/// Default capacity for the command channel. Matches the value used
/// in `neo_blockchain` and `neo_runtime`.
pub const DEFAULT_COMMAND_CAPACITY: usize = 1024;

/// Default capacity for the event broadcast channel. Sized to absorb
/// a burst of inventory events without lagging the producer.
pub const DEFAULT_EVENT_CAPACITY: usize = 1024;

/// `Arc`-wrapped network handle, the form most consumers will store
/// in their state.
pub type SharedNetworkHandle = Arc<NetworkHandle>;
