//! Cheap-to-clone network service handle.
//!
//! The public, request/response API for the network service. Other
//! subsystems (RPC server, consensus driver, node startup) store a
//! `NetworkHandle` in their state and call its methods instead of
//! sending `NetworkCommand` variants directly.

use std::collections::HashSet;
use std::fmt;
use std::hash::{BuildHasher, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;

use neo_payloads::{Block, Transaction};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::command::NetworkCommand;
use crate::error::{NetworkError, NetworkResult};
use crate::event::NetworkEvent;
use crate::peer_id::PeerId;

/// Point-in-time identity / liveness view of the local node, served
/// entirely from handle-side state (no service round-trip).
///
/// This is the seam the RPC server's `getversion` / `getconnectioncount`
/// handlers read. The fields mirror the subset of the C# `LocalNode`
/// surface that the RPC layer consumes:
///
/// - [`LocalNodeInfo::nonce`]: the random identity nonce generated when
///   the handle family was constructed (one nonce per
///   [`NetworkHandle::channel`] / [`NetworkHandle::from_parts`] call,
///   shared by every clone of that handle).
/// - [`LocalNodeInfo::user_agent`]: the node software identifier.
/// - [`LocalNodeInfo::port`]: the TCP listen port recorded when
///   [`NetworkHandle::start`] succeeded (`0` when the listener has not
///   been started through this handle family).
/// - [`LocalNodeInfo::connected_peers_count`]: the connected-peer count
///   folded from the service's `PeerConnected` / `PeerDisconnected`
///   broadcast events.
#[derive(Clone, Debug)]
pub struct LocalNodeInfo {
    /// Random identity nonce of this node instance.
    pub nonce: u32,
    /// User-agent string identifying the node software.
    pub user_agent: String,
    listen_port: u16,
    connected_peers: usize,
}

impl LocalNodeInfo {
    /// TCP listen port the network service was started on through this
    /// handle family, or `0` when no listener has been started.
    pub fn port(&self) -> u16 {
        self.listen_port
    }

    /// Number of currently connected peers, folded from the network
    /// service's `PeerConnected` / `PeerDisconnected` broadcast events.
    ///
    /// The count is exact while the event broadcast channel keeps up;
    /// if the channel lags (more unread events than its capacity), the
    /// dropped events cannot be replayed and the count may drift until
    /// the affected peers produce further connect/disconnect events.
    pub fn connected_peers_count(&self) -> usize {
        self.connected_peers
    }
}

/// Handle-side shared state backing [`LocalNodeInfo`]. One instance is
/// created per handle *family* (in [`NetworkHandle::from_parts`]) and
/// shared by every clone via `Arc`.
struct LocalNodeState {
    nonce: u32,
    user_agent: String,
    listen_addr: parking_lot::RwLock<Option<SocketAddr>>,
    peers: parking_lot::Mutex<PeerTracker>,
}

/// Folds the service's peer lifecycle events into a connected-peer set.
struct PeerTracker {
    events: broadcast::Receiver<NetworkEvent>,
    connected: HashSet<String>,
}

impl LocalNodeState {
    fn new(events: broadcast::Receiver<NetworkEvent>) -> Self {
        // Derive the identity nonce from `RandomState`, which is seeded
        // from OS entropy per instance — no extra dependency needed.
        let nonce = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish() as u32;
        Self {
            nonce,
            user_agent: format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION")),
            listen_addr: parking_lot::RwLock::new(None),
            peers: parking_lot::Mutex::new(PeerTracker {
                events,
                connected: HashSet::new(),
            }),
        }
    }

    /// Drain any pending peer lifecycle events and return the current
    /// connected-peer count.
    fn refresh_connected_peers(&self) -> usize {
        let mut tracker = self.peers.lock();
        loop {
            match tracker.events.try_recv() {
                Ok(NetworkEvent::PeerConnected { peer_id }) => {
                    tracker.connected.insert(peer_id);
                }
                Ok(NetworkEvent::PeerDisconnected { peer_id }) => {
                    tracker.connected.remove(&peer_id);
                }
                Ok(_) => {}
                // Lagged: the broadcast channel dropped events we never
                // saw; keep draining what remains. See
                // `LocalNodeInfo::connected_peers_count` for the
                // documented drift behaviour.
                Err(broadcast::error::TryRecvError::Lagged(_)) => {}
                Err(broadcast::error::TryRecvError::Empty)
                | Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }
        tracker.connected.len()
    }
}

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
    /// Handle-side local-node identity / peer tracking shared by every
    /// clone of this handle family. See [`LocalNodeInfo`].
    local: Arc<LocalNodeState>,
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
        // Subscribe to the event stream *before* the service loop can
        // publish anything, so the handle-side peer tracker observes
        // every peer lifecycle event from the start.
        let local = Arc::new(LocalNodeState::new(event_tx.subscribe()));
        Self {
            cmd_tx,
            event_tx,
            local,
        }
    }

    /// Identity / liveness snapshot of the local node: the random node
    /// nonce, user agent, listen port, and connected-peer count. Served
    /// from handle-side state without a service round-trip; see
    /// [`LocalNodeInfo`] for field semantics.
    pub fn local_node_info(&self) -> LocalNodeInfo {
        let connected_peers = self.local.refresh_connected_peers();
        LocalNodeInfo {
            nonce: self.local.nonce,
            user_agent: self.local.user_agent.clone(),
            listen_port: self
                .local
                .listen_addr
                .read()
                .map(|addr| addr.port())
                .unwrap_or(0),
            connected_peers,
        }
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
        let result = reply_rx
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        if result.is_ok() {
            // Record the listen address so `local_node_info` can report
            // the TCP port without a service round-trip.
            *self.local.listen_addr.write() = Some(bind_addr);
        }
        result
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
