//! `RemoteNodeService` — reth-style per-peer state machine.
//!
//! Each accepted TCP connection spawns one `RemoteNodeService` task.
//! The task owns:
//!
//! - a `tokio::net::TcpStream` (the actual connection),
//! - the per-peer handshake state (`RemoteNodeState`),
//! - a `mpsc::Receiver<RemoteNodeCommand>` for outbound messages
//!   from the local node, and
//! - a `broadcast::Sender<NetworkEvent>` cloned from the local
//!   node's sender for publishing lifecycle events.
//!
//! This module is the **foundation** of the reth-style port: the
//! state machine, the handshake protocol, the inventory queue, the
//! bloom filter, and the outbound command queue from the legacy
//! `RemoteNodeActor` are all still to be ported. What this Stage C
//! commit delivers is the *shape*: the right struct, the right
//! channels, the right `run()` loop, and a few of the simpler
//! message handlers so the service compiles and integrates with
//! `LocalNodeService`.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use neo_config::ProtocolSettings;
use neo_payloads::{Block, Transaction};
use neo_runtime::NetworkEvent as RuntimeNetworkEvent;

use crate::error::NetworkResult;
use crate::event::NetworkEvent;
use crate::peer_id::PeerId;

/// Per-peer state machine.
///
/// Mirrors the legacy `RemoteNodeActor`'s lifecycle: open, then
/// either `Handshake` (server side) or `Connecting` (client side),
/// then `Versioned` once the version payload has been exchanged,
/// then `Ready` once the connection is fully established.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteNodeState {
    /// TCP accepted, awaiting outbound `Version` send.
    Handshake,
    /// Outbound dial in progress, awaiting outbound `Version` send.
    Connecting,
    /// `Version` sent, awaiting peer's `Version` + `Verack`.
    Versioned,
    /// Fully established; data plane is open.
    Ready,
    /// Service is shutting down.
    Closing,
}

/// Inventory item that can be broadcast over an existing peer
/// connection.
#[derive(Clone, Debug)]
pub enum InventoryItem {
    /// Block inventory.
    Block(Block),
    /// Transaction inventory.
    Transaction(Transaction),
}

/// Per-peer command enum sent down the
/// `mpsc::Sender<RemoteNodeCommand>` half of the per-peer channel.
#[derive(Debug)]
pub enum RemoteNodeCommand {
    /// Send an inventory item to the peer.
    SendInventory(InventoryItem),
    /// Send a raw framed message (placeholder until the port of the
    /// full protocol message catalog is complete).
    SendRaw(Vec<u8>),
    /// Request graceful shutdown of the service task.
    Shutdown,
}

/// Cheap-to-clone handle to a running [`RemoteNodeService`] task.
#[derive(Clone)]
pub struct RemoteNodeHandle {
    /// Per-peer command channel sender.
    cmd_tx: mpsc::Sender<RemoteNodeCommand>,
    /// Peer id (cached on the handle so the caller doesn't have to
    /// thread it through every call).
    peer_id: PeerId,
    /// Remote address (cached for the same reason).
    remote_addr: SocketAddr,
}

impl fmt::Debug for RemoteNodeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteNodeHandle")
            .field("peer_id", &self.peer_id)
            .field("remote_addr", &self.remote_addr)
            .field("cmd_capacity", &self.cmd_tx.capacity())
            .finish()
    }
}

impl RemoteNodeHandle {
    /// Identifier of the peer this handle drives.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Remote socket address of the peer.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// Send an inventory item to the peer.
    pub async fn send_inventory(&self, item: InventoryItem) -> NetworkResult<()> {
        self.cmd_tx
            .send(RemoteNodeCommand::SendInventory(item))
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
    }

    /// Send a raw framed message to the peer.
    pub async fn send_raw(&self, bytes: Vec<u8>) -> NetworkResult<()> {
        self.cmd_tx
            .send(RemoteNodeCommand::SendRaw(bytes))
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
    }

    /// Request graceful shutdown of the service task.
    pub async fn shutdown(&self) -> NetworkResult<()> {
        self.cmd_tx
            .send(RemoteNodeCommand::Shutdown)
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
    }
}

/// Reth-style per-peer service.
///
/// Constructed via [`RemoteNodeService::new`], which returns the
/// `(service, handle)` pair. The service is moved into a
/// `tokio::spawn`'d task that calls [`RemoteNodeService::run`].
pub struct RemoteNodeService {
    /// Underlying TCP connection.
    stream: TcpStream,
    /// Peer identifier.
    peer_id: PeerId,
    /// Remote socket address.
    remote_addr: SocketAddr,
    /// Protocol settings (magic, …).
    settings: Arc<ProtocolSettings>,
    /// Current state machine value.
    state: RemoteNodeState,
    /// Per-peer command channel receiver.
    cmd_rx: mpsc::Receiver<RemoteNodeCommand>,
    /// Event broadcast sender.
    event_tx: broadcast::Sender<NetworkEvent>,
    /// Cancellation token used to break the inner `select!` on
    /// shutdown.
    #[allow(dead_code)]
    shutdown: CancellationToken,
}

impl fmt::Debug for RemoteNodeService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RemoteNodeService")
            .field("peer_id", &self.peer_id)
            .field("remote_addr", &self.remote_addr)
            .field("state", &self.state)
            .finish()
    }
}

impl RemoteNodeService {
    /// Build a fresh `(service, handle)` pair.
    pub fn new(
        stream: TcpStream,
        peer_id: PeerId,
        remote_addr: SocketAddr,
        settings: Arc<ProtocolSettings>,
        event_tx: broadcast::Sender<NetworkEvent>,
        initial_state: RemoteNodeState,
    ) -> (Self, RemoteNodeHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let handle = RemoteNodeHandle {
            cmd_tx,
            peer_id,
            remote_addr,
        };
        let service = Self {
            stream,
            peer_id,
            remote_addr,
            settings,
            state: initial_state,
            cmd_rx,
            event_tx,
            shutdown: CancellationToken::new(),
        };
        (service, handle)
    }

    /// Identifier of the peer this service drives.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// Current state machine value.
    pub fn state(&self) -> RemoteNodeState {
        self.state
    }

    /// Drive the per-peer service loop until the command channel
    /// is closed or the cancellation token fires.
    ///
    /// The full port of the legacy actor's per-message handlers is
    /// deferred to a follow-up commit; this Stage C foundation
    /// implements only the *outer* `select!` and the shutdown
    /// signalling. The `state` field is exposed via
    /// [`Self::state`] for tests to inspect.
    pub async fn run(mut self) {
        info!(
            target: "neo_network",
            peer_id = %self.peer_id,
            remote_addr = %self.remote_addr,
            state = ?self.state,
            "remote node service started"
        );
        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => {
                    debug!(
                        target: "neo_network",
                        peer_id = %self.peer_id,
                        "remote node service cancelled"
                    );
                    break;
                }
                cmd = self.cmd_rx.recv() => {
                    match cmd {
                        Some(RemoteNodeCommand::SendInventory(item)) => {
                            self.on_send_inventory(item).await;
                        }
                        Some(RemoteNodeCommand::SendRaw(bytes)) => {
                            self.on_send_raw(bytes).await;
                        }
                        Some(RemoteNodeCommand::Shutdown) | None => break,
                    }
                }
            }
        }
        self.state = RemoteNodeState::Closing;
        let _ = self.event_tx.send(RuntimeNetworkEvent::PeerDisconnected {
            peer_id: self.peer_id.to_string(),
        });
        info!(
            target: "neo_network",
            peer_id = %self.peer_id,
            "remote node service exited"
        );
    }

    /// Per-peer handler for [`RemoteNodeCommand::SendInventory`].
    /// Currently logs and discards; the full port that frames the
    /// payload via the `neo-wire` codec is deferred.
    async fn on_send_inventory(&self, item: InventoryItem) {
        match item {
            InventoryItem::Block(block) => {
                debug!(
                    target: "neo_network",
                    peer_id = %self.peer_id,
                    block_hash = ?block.hash(),
                    "would broadcast block"
                );
            }
            InventoryItem::Transaction(tx) => {
                debug!(
                    target: "neo_network",
                    peer_id = %self.peer_id,
                    tx_hash = ?tx.hash(),
                    "would broadcast transaction"
                );
            }
        }
    }

    /// Per-peer handler for [`RemoteNodeCommand::SendRaw`]. Logs
    /// the byte count; the actual `tokio::io::AsyncWriteExt::write_all`
    /// is deferred to the port of the legacy actor's `send_queue`.
    async fn on_send_raw(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            warn!(
                target: "neo_network",
                peer_id = %self.peer_id,
                "send_raw called with empty payload"
            );
            return;
        }
        debug!(
            target: "neo_network",
            peer_id = %self.peer_id,
            bytes = bytes.len(),
            "would write raw framed message"
        );
        // Stage C foundation: do not actually write to `self.stream`
        // yet. The legacy actor's outbound command queue + framed
        // writer will be ported as part of the RemoteNodeService
        // body work in a follow-up commit.
    }
}
