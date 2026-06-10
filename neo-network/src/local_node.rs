//! `LocalNodeService` — reth-style TCP accept loop.
//!
//! The reth-style replacement for the legacy `LocalNodeActor`. The
//! service owns:
//!
//! - a `tokio::net::TcpListener` for inbound connections,
//! - a `HashMap<PeerId, RemoteNodeHandle>` of the per-peer tasks it
//!   has spawned, and
//! - a `tokio::sync::broadcast::Sender<NetworkEvent>` for publishing
//!   peer-connected / peer-disconnected events.
//!
//! The command loop is a single `while let Some(cmd) = self.cmd_rx.recv().await`
//! that dispatches each [`NetworkCommand`] variant to a private
//! `async fn` handler. This replaces the `impl Actor for LocalNodeActor`
//! pattern one-for-one.
//!
//! ## State machine
//!
//! ```text
//!                      NetworkCommand
//!                            │
//!                            ▼
//! ┌───────────────────────────────────────────────────────────┐
//! │             LocalNodeService.run() command loop            │
//! │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────────┐  │
//! │  │  Start   │  │ Connect  │  │Broadcast │  │  Shutdown  │  │
//! │  │  (bind)  │  │  (dial)  │  │ (relay)  │  │  (drain)   │  │
//! │  └──────────┘  └──────────┘  └──────────┘  └────────────┘  │
//! │                            │                                │
//! │                            ▼                                │
//! │                  TcpListener accept loop                   │
//! │                            │                                │
//! │                            ▼                                │
//! │              tokio::spawn(RemoteNodeService::run)           │
//! └───────────────────────────────────────────────────────────┘
//! ```
//!
//! The `LocalNodeService` does not own the actual per-message
//! protocol state — that lives in each per-peer
//! [`crate::remote_node::RemoteNodeService`]. The local node is
//! responsible for the *connection lifecycle*: bind, accept, dial,
//! disconnect.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use neo_config::ProtocolSettings;
use neo_payloads::{Block, Transaction};
use neo_runtime::{NetworkEvent as RuntimeNetworkEvent, NetworkService, Service, ServiceError};

use crate::command::NetworkCommand;
use crate::error::{NetworkError, NetworkResult};
use crate::event::NetworkEvent;
use crate::handle::{NetworkHandle, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY};
use crate::peer_id::PeerId;
use crate::remote_node::{RemoteNodeHandle, RemoteNodeService, RemoteNodeState};

/// Time we wait for the outbound TCP dial to complete before
/// declaring the peer unreachable.
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Reth-style local node service.
///
/// Constructed via [`LocalNodeService::new`], which returns the
/// `(service, handle)` pair. The service is moved into a
/// `tokio::spawn`'d task that calls [`LocalNodeService::run`].
pub struct LocalNodeService {
    /// Protocol settings (magic, max connections, …).
    settings: Arc<ProtocolSettings>,
    /// Command channel receiver.
    cmd_rx: mpsc::Receiver<NetworkCommand>,
    /// Event broadcast sender. The handle holds a clone for
    /// `subscribe_events`.
    event_tx: broadcast::Sender<NetworkEvent>,
    /// Currently-connected per-peer services. Populated by the
    /// accept loop and `ConnectPeer`.
    remote_nodes: HashMap<PeerId, RemoteNodeHandle>,
    /// Cancellation token used to break the accept loop on
    /// shutdown.
    shutdown: CancellationToken,
    /// `true` once `Start` has been called and the listener is bound.
    started: bool,
    /// Bound address (recorded so the `Debug` impl and the
    /// `Start`-idempotency check can both see it without
    /// re-consuming the listener).
    bind_addr: Option<SocketAddr>,
}

impl fmt::Debug for LocalNodeService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalNodeService")
            .field("started", &self.started)
            .field("bind_addr", &self.bind_addr)
            .field("connected_peers", &self.remote_nodes.len())
            .field("cmd_capacity", &self.cmd_rx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl LocalNodeService {
    /// Build a fresh `(service, handle)` pair.
    pub fn new(settings: Arc<ProtocolSettings>) -> (Self, NetworkHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(DEFAULT_COMMAND_CAPACITY);
        let (event_tx, _event_rx) = broadcast::channel(DEFAULT_EVENT_CAPACITY);
        let handle = NetworkHandle::from_parts(cmd_tx, event_tx.clone());
        let service = Self {
            settings,
            cmd_rx,
            event_tx,
            remote_nodes: HashMap::new(),
            shutdown: CancellationToken::new(),
            started: false,
            bind_addr: None,
        };
        (service, handle)
    }

    /// Drive the service loop until the command channel is closed.
    ///
    /// Every command is dispatched to a private `async fn` handler
    /// on the service struct; the loop itself is just
    /// `while let Some(cmd) = self.cmd_rx.recv().await`.
    pub async fn run(mut self) {
        info!(target: "neo_network", "local node service run loop started");
        while let Some(cmd) = self.cmd_rx.recv().await {
            let is_shutdown = matches!(cmd, NetworkCommand::Shutdown);
            self.dispatch(cmd).await;
            if is_shutdown {
                break;
            }
        }
        self.on_shutdown().await;
        info!(target: "neo_network", "local node service run loop exited");
    }

    /// Dispatch a single command to its handler. Public for testing.
    pub async fn dispatch(&mut self, cmd: NetworkCommand) {
        match cmd {
            NetworkCommand::Start { bind_addr, reply } => {
                let result = self.handle_start(bind_addr).await;
                let _ = reply.send(result);
            }
            NetworkCommand::ConnectPeer { addr, reply } => {
                let result = self.handle_connect_peer(addr).await;
                let _ = reply.send(result);
            }
            NetworkCommand::DisconnectPeer { peer_id, reply } => {
                let result = self.handle_disconnect_peer(peer_id).await;
                let _ = reply.send(result);
            }
            NetworkCommand::BroadcastBlock { block } => {
                self.handle_broadcast_block(&block).await;
            }
            NetworkCommand::BroadcastTransaction { transaction } => {
                self.handle_broadcast_transaction(&transaction).await;
            }
            NetworkCommand::RelayInventory { hash } => {
                self.handle_relay_inventory(hash).await;
            }
            NetworkCommand::Shutdown => {
                self.on_shutdown().await;
            }
        }
    }

    // -----------------------------------------------------------------
    // Handlers
    // -----------------------------------------------------------------

    async fn handle_start(&mut self, bind_addr: SocketAddr) -> NetworkResult<SocketAddr> {
        if self.started {
            return Err(NetworkError::Protocol(
                "local node already started".to_string(),
            ));
        }
        let listener = TcpListener::bind(bind_addr).await?;
        // Resolve the *actual* bound address: when the caller requested
        // port `0` the kernel picks an ephemeral port, and both the
        // reply (handle-side `getversion` port reporting) and the
        // recorded `bind_addr` must reflect the real listener endpoint.
        let local_addr = listener.local_addr()?;
        info!(target: "neo_network", %local_addr, "local node tcp listener bound");

        // Spawn the accept loop on a fresh task. The task captures
        // its own clones of the shared state so the command loop
        // can keep running.
        let event_tx = self.event_tx.clone();
        let settings = self.settings.clone();
        let remote_nodes = Arc::new(parking_lot::Mutex::new(HashMap::<PeerId, RemoteNodeHandle>::new()));
        let shutdown = self.shutdown.clone();
        tokio::spawn(accept_loop(
            listener,
            settings,
            event_tx,
            remote_nodes,
            shutdown,
        ));

        self.started = true;
        self.bind_addr = Some(local_addr);
        Ok(local_addr)
    }

    async fn handle_connect_peer(&mut self, addr: SocketAddr) -> NetworkResult<PeerId> {
        if !self.started {
            return Err(NetworkError::NotStarted);
        }
        let stream = tokio::time::timeout(DIAL_TIMEOUT, TcpStream::connect(addr))
            .await
            .map_err(|_| {
                NetworkError::Protocol(format!("dial timeout after {DIAL_TIMEOUT:?}"))
            })??;
        let peer_id = PeerId::new();
        let (service, handle) = RemoteNodeService::new(
            stream,
            peer_id,
            addr,
            self.settings.clone(),
            self.event_tx.clone(),
            RemoteNodeState::Connecting,
        );
        tokio::spawn(service.run());
        self.remote_nodes.insert(peer_id, handle);
        // The dialed endpoint *is* the peer's listener, so the event
        // carries the same address/port pair C# reports for outbound
        // remotes (`Remote.Address` / `ListenerTcpPort`).
        self.event_tx
            .send(RuntimeNetworkEvent::PeerConnected {
                peer_id: peer_id.to_string(),
                address: Some(addr),
            })
            .ok();
        Ok(peer_id)
    }

    async fn handle_disconnect_peer(&mut self, peer_id: PeerId) -> NetworkResult<()> {
        if let Some(handle) = self.remote_nodes.remove(&peer_id) {
            let _ = handle.shutdown().await;
            self.event_tx
                .send(RuntimeNetworkEvent::PeerDisconnected {
                    peer_id: peer_id.to_string(),
                })
                .ok();
        }
        Ok(())
    }

    async fn handle_broadcast_block(&self, block: &Block) {
        for (peer_id, handle) in &self.remote_nodes {
            if let Err(err) = handle
                .send_inventory(crate::remote_node::InventoryItem::Block(block.clone()))
                .await
            {
                warn!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "failed to send block inventory to peer"
                );
            }
        }
    }

    async fn handle_broadcast_transaction(&self, tx: &Transaction) {
        for (peer_id, handle) in &self.remote_nodes {
            if let Err(err) = handle
                .send_inventory(crate::remote_node::InventoryItem::Transaction(tx.clone()))
                .await
            {
                warn!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "failed to send transaction inventory to peer"
                );
            }
        }
    }

    async fn handle_relay_inventory(&self, hash: neo_primitives::UInt256) {
        trace!(target: "neo_network", ?hash, "relay inventory");
    }

    async fn on_shutdown(&mut self) {
        for (_peer_id, handle) in self.remote_nodes.drain() {
            let _ = handle.shutdown().await;
        }
        self.shutdown.cancel();
        self.started = false;
        self.bind_addr = None;
    }
}

// -----------------------------------------------------------------------------
// Trait impls
// -----------------------------------------------------------------------------

impl Service for LocalNodeService {
    fn name(&self) -> &str {
        "LocalNodeService"
    }
}

#[async_trait]
impl NetworkService for LocalNodeService {
    async fn broadcast_block(&self, block: &Block) -> Result<(), ServiceError> {
        self.handle_broadcast_block(block).await;
        Ok(())
    }

    async fn broadcast_transaction(&self, tx: &Transaction) -> Result<(), ServiceError> {
        self.handle_broadcast_transaction(tx).await;
        Ok(())
    }

    async fn peer_count(&self) -> usize {
        self.remote_nodes.len()
    }

    fn subscribe_events(&self) -> broadcast::Receiver<RuntimeNetworkEvent> {
        self.event_tx.subscribe()
    }
}

// -----------------------------------------------------------------------------
// Accept loop
// -----------------------------------------------------------------------------

async fn accept_loop(
    listener: TcpListener,
    settings: Arc<ProtocolSettings>,
    event_tx: broadcast::Sender<NetworkEvent>,
    remote_nodes: Arc<parking_lot::Mutex<HashMap<PeerId, RemoteNodeHandle>>>,
    shutdown: CancellationToken,
) {
    info!(target: "neo_network", "accept loop started");
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => {
                debug!(target: "neo_network", "accept loop cancelled");
                break;
            }
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, remote_addr)) => {
                        let peer_id = PeerId::new();
                        info!(
                            target: "neo_network",
                            %peer_id,
                            %remote_addr,
                            "inbound connection accepted"
                        );
                        let (service, handle) = RemoteNodeService::new(
                            stream,
                            peer_id,
                            remote_addr,
                            settings.clone(),
                            event_tx.clone(),
                            RemoteNodeState::Handshake,
                        );
                        // Retain the per-peer handle: dropping it would
                        // close the per-peer command channel and make the
                        // freshly spawned service exit (publishing a
                        // spurious `PeerDisconnected`) before the peer
                        // exchanged a single byte. Dropping the map on
                        // accept-loop shutdown tears the per-peer tasks
                        // down with it.
                        remote_nodes.lock().insert(peer_id, handle);
                        // C# reports a peer's LISTENER port (from its
                        // version payload's TcpServer capability), never
                        // the remote's ephemeral source port, and encodes
                        // an unknown listener as port 0 (RemoteNode.cs:54
                        // initializes ListenerTcpPort = 0). The version
                        // handshake is not ported yet, so publish the
                        // C#-faithful unknown form: (remote_ip, 0).
                        let reported = std::net::SocketAddr::new(remote_addr.ip(), 0);
                        let _ = event_tx.send(RuntimeNetworkEvent::PeerConnected {
                            peer_id: peer_id.to_string(),
                            address: Some(reported),
                        });
                        tokio::spawn(service.run());
                    }
                    Err(err) => {
                        error!(
                            target: "neo_network",
                            %err,
                            "tcp accept failed"
                        );
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
        }
    }
    info!(target: "neo_network", "accept loop exited");
}

#[allow(dead_code)]
fn _force_reply_link(_r: oneshot::Sender<NetworkResult<()>>) {}
