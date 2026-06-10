//! Cheap-to-clone network service handle.
//!
//! The public, request/response API for the network service. Other
//! subsystems (RPC server, consensus driver, node startup) store a
//! `NetworkHandle` in their state and call its methods instead of
//! sending `NetworkCommand` variants directly.

use std::collections::BTreeMap;
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

/// A single connected-peer entry folded handle-side from the network
/// service's peer lifecycle events.
///
/// This is the per-peer record behind the RPC server's `getpeers`
/// `connected` array (C# `LocalNode.GetRemoteNodes()`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectedPeer {
    /// Opaque peer identifier carried by the service's
    /// `PeerConnected` / `PeerDisconnected` broadcast events
    /// (the `Display` form of [`crate::peer_id::PeerId`]).
    pub peer_id: String,
    /// Remote socket address of the peer, when known at the handle
    /// seam.
    ///
    /// The lifecycle events carry only the opaque peer id, so the
    /// address is attached out-of-band: [`NetworkHandle::connect_peer`]
    /// records the dialed address on a successful outbound dial (the
    /// dialed endpoint *is* the peer's listener, so address + port
    /// match the `Remote.Address` / `ListenerTcpPort` pair C#'s
    /// `LocalNode.GetRemoteNodes` reports). Peers whose address was
    /// never recorded — e.g. inbound connections accepted by the
    /// service's TCP listener — fold with `None` here.
    pub address: Option<SocketAddr>,
}

/// Point-in-time identity / liveness view of the local node, served
/// entirely from handle-side state (no service round-trip).
///
/// This is the seam the RPC server's `getversion` / `getpeers` /
/// `getconnectioncount` handlers read. The fields mirror the subset of
/// the C# `LocalNode` surface that the RPC layer consumes:
///
/// - [`LocalNodeInfo::nonce`]: the random identity nonce generated when
///   the handle family was constructed (one nonce per
///   [`NetworkHandle::channel`] / [`NetworkHandle::from_parts`] call,
///   shared by every clone of that handle).
/// - [`LocalNodeInfo::user_agent`]: the node software identifier.
/// - [`LocalNodeInfo::port`]: the TCP listen port recorded when
///   [`NetworkHandle::start`] succeeded (`0` when the listener has not
///   been started through this handle family).
/// - [`LocalNodeInfo::connected_peers`]: the connected peer set folded
///   from the service's `PeerConnected` / `PeerDisconnected` broadcast
///   events (C# `LocalNode.GetRemoteNodes`).
#[derive(Clone, Debug)]
pub struct LocalNodeInfo {
    /// Random identity nonce of this node instance.
    pub nonce: u32,
    /// User-agent string identifying the node software.
    pub user_agent: String,
    listen_port: u16,
    connected_peers: Vec<ConnectedPeer>,
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
        self.connected_peers.len()
    }

    /// The currently connected peers, folded from the network
    /// service's `PeerConnected` / `PeerDisconnected` broadcast events,
    /// in deterministic (peer-id) order.
    ///
    /// Entries carry the remote address only when it is known at the
    /// handle seam — see [`ConnectedPeer::address`]. The same
    /// lag-drift caveat as [`LocalNodeInfo::connected_peers_count`]
    /// applies.
    pub fn connected_peers(&self) -> &[ConnectedPeer] {
        &self.connected_peers
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
    /// Connected peer set keyed by the event-stream peer id, valued
    /// with the last recorded remote address (see
    /// [`ConnectedPeer::address`]). A `BTreeMap` keeps the fold — and
    /// therefore the `getpeers` view — deterministically ordered.
    connected: BTreeMap<String, Option<SocketAddr>>,
}

impl PeerTracker {
    /// Drain any pending peer lifecycle events into the connected map.
    fn fold_pending_events(&mut self) {
        loop {
            match self.events.try_recv() {
                Ok(NetworkEvent::PeerConnected { peer_id }) => {
                    // `or_insert` keeps an address already attached via
                    // `record_peer_address` when the lifecycle event
                    // drains after the dial reply resolved.
                    self.connected.entry(peer_id).or_insert(None);
                }
                Ok(NetworkEvent::PeerDisconnected { peer_id }) => {
                    self.connected.remove(&peer_id);
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
    }

    /// Point-in-time copy of the connected peer set, in key order.
    fn snapshot(&self) -> Vec<ConnectedPeer> {
        self.connected
            .iter()
            .map(|(peer_id, address)| ConnectedPeer {
                peer_id: peer_id.clone(),
                address: *address,
            })
            .collect()
    }
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
                connected: BTreeMap::new(),
            }),
        }
    }

    /// Drain any pending peer lifecycle events and return the current
    /// connected peer set.
    fn refresh_connected_peers(&self) -> Vec<ConnectedPeer> {
        let mut tracker = self.peers.lock();
        tracker.fold_pending_events();
        tracker.snapshot()
    }

    /// Attach `addr` to the tracker entry for `peer_id`.
    ///
    /// Folds pending events first so the entry created by the peer's
    /// own `PeerConnected` event (published by the service before the
    /// dial reply resolves) is present, then records the address. The
    /// insert also covers the rare case where that event was lost to
    /// broadcast lag: the dial reply proved the peer connected.
    fn record_peer_address(&self, peer_id: String, addr: SocketAddr) {
        let mut tracker = self.peers.lock();
        tracker.fold_pending_events();
        tracker.connected.insert(peer_id, Some(addr));
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
    /// nonce, user agent, listen port, and connected peer set. Served
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

    /// Record the remote socket address of a connected peer in the
    /// handle-side peer tracker, keyed by the peer's event-stream id.
    ///
    /// [`NetworkHandle::connect_peer`] calls this automatically for
    /// successful outbound dials. Service-side integrations that learn
    /// a peer's address out-of-band (the lifecycle events carry only
    /// opaque peer ids) can record it here so the `getpeers`-style
    /// view ([`LocalNodeInfo::connected_peers`]) reports it.
    pub fn record_peer_address(&self, peer_id: impl Into<String>, addr: SocketAddr) {
        self.local.record_peer_address(peer_id.into(), addr);
    }

    /// Subscribe to network events. Each call returns an independent
    /// receiver; dropping the receiver unregisters the subscription.
    pub fn subscribe(&self) -> broadcast::Receiver<NetworkEvent> {
        self.event_tx.subscribe()
    }

    /// Publishing half of the event broadcast channel.
    ///
    /// The service loop normally keeps the sender returned by
    /// [`NetworkHandle::channel`]; this accessor serves callers that
    /// were handed only a pre-built handle (e.g. composition roots and
    /// tests) and need to publish lifecycle events into the same
    /// channel the handle-side peer tracker folds.
    pub fn event_sender(&self) -> broadcast::Sender<NetworkEvent> {
        self.event_tx.clone()
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
    ///
    /// On success the dialed address is recorded against the new peer
    /// in the handle-side peer tracker, so
    /// [`LocalNodeInfo::connected_peers`] reports it (see
    /// [`ConnectedPeer::address`]).
    pub async fn connect_peer(&self, addr: SocketAddr) -> NetworkResult<PeerId> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(NetworkCommand::ConnectPeer {
                addr,
                reply: reply_tx,
            })
            .await
            .map_err(|_| NetworkError::LocalShuttingDown)?;
        let result = reply_rx.await.map_err(|_| NetworkError::LocalShuttingDown)?;
        if let Ok(peer_id) = &result {
            // The service publishes the peer's `PeerConnected` event
            // before resolving the dial reply, so the tracker entry is
            // already pending; attach the dialed address to it.
            self.local.record_peer_address(peer_id.to_string(), addr);
        }
        result
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_handle() -> (
        NetworkHandle,
        mpsc::Receiver<NetworkCommand>,
        broadcast::Sender<NetworkEvent>,
    ) {
        NetworkHandle::channel(8, 32)
    }

    fn addr(s: &str) -> SocketAddr {
        s.parse().expect("socket address")
    }

    #[test]
    fn folds_peer_connected_and_disconnected_events() {
        let (handle, _cmd_rx, events) = test_handle();
        assert_eq!(handle.local_node_info().connected_peers_count(), 0);
        assert!(handle.local_node_info().connected_peers().is_empty());

        events
            .send(NetworkEvent::PeerConnected {
                peer_id: "peer:1".to_string(),
            })
            .expect("publish");
        events
            .send(NetworkEvent::PeerConnected {
                peer_id: "peer:2".to_string(),
            })
            .expect("publish");

        let info = handle.local_node_info();
        assert_eq!(info.connected_peers_count(), 2);
        // Event-only peers fold without an address: the lifecycle
        // events carry only the opaque peer id.
        assert!(info
            .connected_peers()
            .iter()
            .all(|peer| peer.address.is_none()));

        events
            .send(NetworkEvent::PeerDisconnected {
                peer_id: "peer:1".to_string(),
            })
            .expect("publish");

        let info = handle.local_node_info();
        assert_eq!(info.connected_peers_count(), 1);
        assert_eq!(info.connected_peers()[0].peer_id, "peer:2");
    }

    #[test]
    fn duplicate_peer_connected_events_fold_once() {
        let (handle, _cmd_rx, events) = test_handle();
        for _ in 0..3 {
            events
                .send(NetworkEvent::PeerConnected {
                    peer_id: "peer:1".to_string(),
                })
                .expect("publish");
        }
        assert_eq!(handle.local_node_info().connected_peers_count(), 1);
    }

    #[test]
    fn record_peer_address_attaches_address_until_disconnect() {
        let (handle, _cmd_rx, events) = test_handle();
        let remote = addr("10.0.0.9:20333");

        // Realistic order: the service publishes the lifecycle event
        // first, the dial path records the address afterwards.
        events
            .send(NetworkEvent::PeerConnected {
                peer_id: "peer:7".to_string(),
            })
            .expect("publish");
        handle.record_peer_address("peer:7", remote);

        let info = handle.local_node_info();
        assert_eq!(info.connected_peers_count(), 1);
        assert_eq!(
            info.connected_peers()[0],
            ConnectedPeer {
                peer_id: "peer:7".to_string(),
                address: Some(remote),
            }
        );

        events
            .send(NetworkEvent::PeerDisconnected {
                peer_id: "peer:7".to_string(),
            })
            .expect("publish");
        assert_eq!(handle.local_node_info().connected_peers_count(), 0);
    }

    #[test]
    fn peer_connected_event_keeps_previously_recorded_address() {
        let (handle, _cmd_rx, events) = test_handle();
        let remote = addr("192.168.1.4:10333");

        // Reverse order: the address lands before the lifecycle event
        // drains; the `or_insert` fold must not erase it.
        handle.record_peer_address("peer:3", remote);
        events
            .send(NetworkEvent::PeerConnected {
                peer_id: "peer:3".to_string(),
            })
            .expect("publish");

        let info = handle.local_node_info();
        assert_eq!(info.connected_peers().len(), 1);
        assert_eq!(info.connected_peers()[0].address, Some(remote));
    }

    #[test]
    fn connected_peers_snapshot_is_ordered_by_peer_id() {
        let (handle, _cmd_rx, events) = test_handle();
        for id in ["peer:9", "peer:1", "peer:5"] {
            events
                .send(NetworkEvent::PeerConnected {
                    peer_id: id.to_string(),
                })
                .expect("publish");
        }
        let info = handle.local_node_info();
        let ids: Vec<&str> = info
            .connected_peers()
            .iter()
            .map(|peer| peer.peer_id.as_str())
            .collect();
        assert_eq!(ids, vec!["peer:1", "peer:5", "peer:9"]);
    }

    #[tokio::test]
    async fn connect_peer_records_dialed_address() {
        let (handle, mut cmd_rx, events) = test_handle();

        // Stand-in for the service's `handle_connect_peer`: publish
        // the lifecycle event, then resolve the dial reply — the same
        // order `LocalNodeService` uses.
        tokio::spawn(async move {
            while let Some(cmd) = cmd_rx.recv().await {
                if let NetworkCommand::ConnectPeer { addr: _, reply } = cmd {
                    let peer_id = PeerId::from_raw(42);
                    let _ = events.send(NetworkEvent::PeerConnected {
                        peer_id: peer_id.to_string(),
                    });
                    let _ = reply.send(Ok(peer_id));
                }
            }
        });

        let remote = addr("127.0.0.1:20333");
        let peer_id = handle.connect_peer(remote).await.expect("connect");
        assert_eq!(peer_id, PeerId::from_raw(42));

        let info = handle.local_node_info();
        assert_eq!(info.connected_peers_count(), 1);
        assert_eq!(info.connected_peers()[0].peer_id, peer_id.to_string());
        assert_eq!(info.connected_peers()[0].address, Some(remote));
    }
}
