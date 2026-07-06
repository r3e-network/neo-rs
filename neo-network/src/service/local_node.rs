//! `LocalNodeService` — reth-style TCP accept loop.
//!
//! The reth-style replacement for the legacy `LocalNodeActor`. The
//! service owns:
//!
//! - a `tokio::net::TcpListener` for inbound connections,
//! - a shared [`PeerRegistry`] of the per-peer tasks it has spawned
//!   (the same registry instance is used by the dial path, the accept
//!   loop, and every per-peer service — there is exactly one
//!   connected-peer map),
//! - a `tokio::sync::broadcast::Sender<NetworkEvent>` for publishing
//!   peer-connected / peer-disconnected events, and
//! - the [`LocalIdentity`] (network magic, nonce, user agent, listen
//!   port) advertised in every outbound version payload.
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
//! admission control (C# `Peer.OnTcpConnected` caps), disconnect.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

use crate::ChannelsConfig;
use neo_config::ProtocolSettings;
use neo_payloads::{Block, Transaction};
use neo_runtime::{NetworkEvent as RuntimeNetworkEvent, NetworkService, Service, ServiceError};

use crate::command::NetworkCommand;
use crate::error::{NetworkError, NetworkResult};
use crate::event::NetworkEvent;
use crate::handle::{DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY, NetworkHandle};
use crate::local_identity::LocalIdentity;
use crate::peer_id::PeerId;
use crate::peer_registry::PeerRegistry;
use crate::remote_node::{BlockSource, InboundInventory, RemoteNodeService, RemoteNodeState};
use crate::service::block_sync_mode::BlockSyncMode;
use crate::spawn::spawn_guarded;

/// Time we wait for the outbound TCP dial to complete before
/// declaring the peer unreachable.
const DIAL_TIMEOUT: Duration = Duration::from_secs(15);

/// Cadence of the peer-discovery maintenance tick (C# `Peer.OnTimer` fires
/// every 5 s to top the connection count back up toward the desired minimum).
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(5);

/// Reth-style local node service.
///
/// Constructed via [`LocalNodeService::new`] (C# `ChannelsConfig`
/// defaults) or [`LocalNodeService::with_config`], both returning the
/// `(service, handle)` pair. The service is moved into a
/// `tokio::spawn`'d task that calls [`LocalNodeService::run`].
pub struct LocalNodeService {
    /// Protocol settings (network magic, …).
    settings: Arc<ProtocolSettings>,
    /// Channel configuration (connection caps, compression flag).
    config: ChannelsConfig,
    /// Identity advertised in outbound version payloads.
    identity: Arc<LocalIdentity>,
    /// Command channel receiver.
    cmd_rx: mpsc::Receiver<NetworkCommand>,
    /// Event broadcast sender. The handle holds a clone for
    /// `subscribe_events`.
    event_tx: broadcast::Sender<NetworkEvent>,
    /// Unified connected-peer registry shared with the accept loop
    /// and every per-peer service.
    registry: Arc<PeerRegistry>,
    /// Cancellation token used to break the accept loop and the
    /// per-peer read loops on shutdown.
    shutdown: CancellationToken,
    /// `true` once `Start` has been called and the listener is bound.
    started: bool,
    /// Bound address (recorded so the `Debug` impl and the
    /// `Start`-idempotency check can both see it without
    /// re-consuming the listener).
    bind_addr: Option<SocketAddr>,
    /// `JoinHandle` of the spawned accept loop, so we can await it
    /// during shutdown to catch panics.
    accept_handle: Option<JoinHandle<()>>,
    /// Optional sink handed to every per-peer service so blocks and
    /// transactions decoded from peers reach the ledger.
    inbound_tx: Option<mpsc::Sender<InboundInventory>>,
    /// Optional read-only ledger view handed to every per-peer service
    /// so it can serve peers' block requests.
    block_source: Option<Arc<dyn BlockSource>>,
    /// Owner of outbound block range requests.
    block_sync_mode: BlockSyncMode,
    /// Cadence of the peer-discovery maintenance tick. Defaults to
    /// [`DISCOVERY_INTERVAL`] (C# `Peer.OnTimer` = 5 s); overridable so
    /// integration tests can drive discovery on a fast cadence.
    discovery_interval: Duration,
}

impl fmt::Debug for LocalNodeService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalNodeService")
            .field("started", &self.started)
            .field("bind_addr", &self.bind_addr)
            .field("connected_peers", &self.registry.len())
            .field("cmd_capacity", &self.cmd_rx.capacity())
            .field("event_receivers", &self.event_tx.receiver_count())
            .finish()
    }
}

impl LocalNodeService {
    /// Build a fresh `(service, handle)` pair with the default
    /// [`ChannelsConfig`] (C# defaults: 40 max connections, 3 per
    /// address, compression enabled).
    pub fn new(settings: Arc<ProtocolSettings>) -> (Self, NetworkHandle) {
        Self::with_config(settings, ChannelsConfig::default())
    }

    /// Build a fresh `(service, handle)` pair with an explicit
    /// channel configuration.
    pub fn with_config(
        settings: Arc<ProtocolSettings>,
        config: ChannelsConfig,
    ) -> (Self, NetworkHandle) {
        let registry = Arc::new(PeerRegistry::from_config(&config));
        Self::with_config_and_registry(settings, config, registry)
    }

    /// Build a fresh `(service, handle)` pair with an externally supplied peer
    /// registry.
    ///
    /// Composition roots use this when downloader components need the same
    /// connected-peer map as the live P2P service. The registry limits must
    /// match `config`; callers should normally create it with
    /// [`PeerRegistry::from_config`].
    pub fn with_config_and_registry(
        settings: Arc<ProtocolSettings>,
        config: ChannelsConfig,
        registry: Arc<PeerRegistry>,
    ) -> (Self, NetworkHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(DEFAULT_COMMAND_CAPACITY);
        let (event_tx, _event_rx) = broadcast::channel(DEFAULT_EVENT_CAPACITY);
        let handle = NetworkHandle::from_parts(cmd_tx, event_tx.clone());
        // Source the identity nonce / user agent from the handle
        // family so the values served by `getversion` are the values
        // sent in version payloads (C#: both read `LocalNode.Nonce` /
        // `LocalNode.UserAgent`).
        let info = handle.local_node_info();
        let identity = Arc::new(LocalIdentity::new(
            settings.network,
            info.nonce,
            info.user_agent.clone(),
            config.enable_compression,
        ));
        let service = Self {
            settings,
            config,
            identity,
            cmd_rx,
            event_tx,
            registry,
            shutdown: CancellationToken::new(),
            started: false,
            bind_addr: None,
            accept_handle: None,
            inbound_tx: None,
            block_source: None,
            block_sync_mode: BlockSyncMode::default(),
            discovery_interval: DISCOVERY_INTERVAL,
        };
        (service, handle)
    }

    /// Override the peer-discovery tick cadence (default
    /// [`DISCOVERY_INTERVAL`]). Intended for integration tests that need
    /// discovery to run faster than the 5 s production interval.
    #[doc(hidden)]
    pub fn with_discovery_interval(mut self, interval: Duration) -> Self {
        self.discovery_interval = interval;
        self
    }

    /// Attach an inbound-inventory sink: blocks and transactions decoded
    /// from every connected peer are forwarded over `inbound_tx` to the
    /// composition root, which relays them to the blockchain service.
    pub fn with_inventory_sink(mut self, inbound_tx: mpsc::Sender<InboundInventory>) -> Self {
        self.inbound_tx = Some(inbound_tx);
        self
    }

    /// Attach a read-only ledger view so every per-peer service can serve
    /// peers' `GetBlockByIndex` requests from the local chain.
    pub fn with_block_source(mut self, block_source: Arc<dyn BlockSource>) -> Self {
        self.block_source = Some(block_source);
        self
    }

    /// Select which component owns outbound block-sync range requests.
    ///
    /// Defaults to [`BlockSyncMode::LegacyPerPeer`]. Production composition can
    /// switch to [`BlockSyncMode::ExternalCoordinator`] when a shared
    /// `BlockDownloadCoordinator` task owns cross-peer scheduling.
    pub fn with_block_sync_mode(mut self, mode: BlockSyncMode) -> Self {
        self.block_sync_mode = mode;
        self
    }

    /// Channel configuration in effect for this service.
    pub fn config(&self) -> &ChannelsConfig {
        &self.config
    }

    /// Protocol settings in effect for this service.
    pub fn settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    /// Drive the service loop until the command channel is closed.
    ///
    /// The loop `select!`s over the command stream and a periodic
    /// peer-discovery tick (C# `Peer.OnTimer`). Every command is dispatched
    /// to a private `async fn` handler on the service struct; the discovery
    /// tick runs the private peer-maintenance helper.
    pub async fn run(mut self) {
        info!(target: "neo_network", "local node service run loop started");
        // C# `Peer.OnTimer` runs on a fixed schedule independent of inbound
        // commands; drive the peer-discovery maintenance from a matching
        // interval interleaved with the command stream.
        let mut discovery_timer = tokio::time::interval(self.discovery_interval);
        discovery_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The first tick fires immediately; skip it so we do not run discovery
        // before the listener is even started.
        discovery_timer.tick().await;
        loop {
            tokio::select! {
                cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = cmd else {
                        break;
                    };
                    let is_shutdown = matches!(cmd, NetworkCommand::Shutdown);
                    self.dispatch(cmd).await;
                    if is_shutdown {
                        break;
                    }
                }
                _ = discovery_timer.tick() => {
                    self.maintain_peers().await;
                }
            }
        }
        self.on_shutdown().await;
        info!(target: "neo_network", "local node service run loop exited");
    }

    /// Peer-discovery maintenance tick (C# `Peer.OnTimer` +
    /// `LocalNode.NeedMorePeers`). When the connected count is below the
    /// desired minimum:
    ///
    /// 1. Dial candidate endpoints already learned via `Addr` gossip and
    ///    queued in the shared address book (C# `OnTimer` samples
    ///    `UnconnectedPeers` and calls `ConnectToPeer`).
    /// 2. If the address book is empty but peers are connected, broadcast
    ///    `GetAddr` to every peer to learn more (C# `NeedMorePeers`'s
    ///    `if (!ConnectedPeers.IsEmpty) BroadcastMessage(GetAddr)` branch).
    ///
    /// The zero-peers reseed branch (C# `NeedMorePeers` else-branch that
    /// re-adds the seed list) is driven by the node's seed dialer, so it is
    /// intentionally not duplicated here.
    async fn maintain_peers(&mut self) {
        if !self.started {
            return;
        }
        let connected = self.registry.len();
        let desired = self.config.min_desired_connections;
        if connected >= desired {
            return;
        }
        let need = desired - connected;

        // Prefer dialing peers we already know about (C# `OnTimer` connects
        // sampled `UnconnectedPeers` before asking for more).
        let candidates = self.registry.take_unconnected(need);
        if !candidates.is_empty() {
            for addr in candidates {
                if let Err(err) = self.handle_connect_peer(addr).await {
                    debug!(
                        target: "neo_network",
                        %addr,
                        %err,
                        "discovery dial to unconnected candidate failed"
                    );
                }
            }
            return;
        }

        // No candidates queued: ask connected peers for more (C#
        // `NeedMorePeers` → `BroadcastMessage(GetAddr)`). With no peers at all
        // there is nobody to ask; the seed dialer handles that case.
        let handles = self.registry.handles();
        if handles.is_empty() {
            return;
        }
        debug!(
            target: "neo_network",
            connected,
            desired,
            "peer count below desired minimum; broadcasting getaddr"
        );
        for (peer_id, handle) in handles {
            if let Err(err) = handle.try_send_get_addr() {
                trace!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "dropping getaddr to peer (channel full or closed)"
                );
            }
        }
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
            NetworkCommand::BroadcastExtensible { payload } => {
                self.handle_broadcast_extensible(&payload).await;
            }
            NetworkCommand::BroadcastInv {
                inventory_type,
                hashes,
            } => {
                self.handle_broadcast_inv(inventory_type, hashes).await;
            }
            NetworkCommand::RelayInventory { hash } => {
                self.handle_relay_inventory(hash).await;
            }
            NetworkCommand::SetBlockHeight { height } => {
                // Shared with every per-peer task via the `Arc<LocalIdentity>`;
                // advertised in version/ping and read to gate block-sync.
                self.identity.set_block_height(height);
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

        // Record the listener port on the shared identity so version
        // payloads sent from now on advertise the `TcpServer`
        // capability (C# `Peer.ListenerTcpPort`).
        self.identity.set_listen_port(local_addr.port());

        // Spawn the accept loop on a fresh task. The task captures
        // its own clones of the shared state so the command loop
        // can keep running. The `JoinHandle` is stored so we can
        // await it during shutdown to catch panics.
        self.accept_handle = Some(spawn_guarded(
            "accept_loop",
            accept_loop(
                listener,
                self.identity.clone(),
                self.registry.clone(),
                self.event_tx.clone(),
                self.shutdown.clone(),
                self.inbound_tx.clone(),
                self.block_source.clone(),
                self.block_sync_mode,
            ),
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
            self.identity.clone(),
            self.registry.clone(),
            self.event_tx.clone(),
            RemoteNodeState::Connecting,
            self.shutdown.clone(),
        );
        let service = service.with_block_sync_mode(self.block_sync_mode);
        let service = match &self.inbound_tx {
            Some(tx) => service.with_inventory_sink(tx.clone()),
            None => service,
        };
        let service = match &self.block_source {
            Some(source) => service.with_block_source(Arc::clone(source)),
            None => service,
        };
        // Admission control applies to dialed peers too: C# routes
        // outbound `Tcp.Connected` events through the same
        // `Peer.OnTcpConnected` cap checks as inbound ones.
        if !self.registry.try_admit(peer_id, addr, handle) {
            // Dropping the service closes the freshly dialed stream
            // (the C# path replies Tcp.Abort).
            return Err(NetworkError::Protocol(format!(
                "connection to {addr} rejected: connection limit reached"
            )));
        }
        // Publish before spawning so the `PeerConnected` event is
        // ordered before anything the per-peer task publishes (e.g.
        // the post-handshake address upgrade). The dialed endpoint
        // *is* the peer's listener, so the event carries the same
        // address/port pair C# reports for outbound remotes
        // (`Remote.Address` / `ListenerTcpPort`).
        self.event_tx
            .send(RuntimeNetworkEvent::PeerConnected {
                peer_id: peer_id.to_string(),
                address: Some(addr),
            })
            .ok();
        spawn_guarded("remote_node", service.run());
        Ok(peer_id)
    }

    async fn handle_disconnect_peer(&mut self, peer_id: PeerId) -> NetworkResult<()> {
        // The per-peer task owns the teardown: it removes itself from
        // the registry and publishes the single `PeerDisconnected`
        // event when its run loop exits.
        if let Some(handle) = self.registry.handle(peer_id) {
            let _ = handle.shutdown().await;
        }
        Ok(())
    }

    async fn handle_broadcast_block(&self, block: &Block) {
        for (peer_id, handle) in self.registry.handles() {
            if let Err(err) =
                handle.try_send_inventory(crate::remote_node::InventoryItem::Block(block.clone()))
            {
                warn!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "dropping block inventory to peer (channel full or closed)"
                );
            }
        }
    }

    /// Relay an extensible payload (dBFT consensus message) to every peer.
    /// The frame is encoded once and sent verbatim via `send_raw`; consensus
    /// payloads are small (< the compression minimum), so it is sent
    /// uncompressed for a deterministic frame.
    async fn handle_broadcast_extensible(&self, payload: &neo_payloads::ExtensiblePayload) {
        let frame = match crate::wire::Message::create(
            crate::MessageCommand::Extensible,
            Some(payload),
            false,
        )
        .and_then(|message| message.to_bytes())
        {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(target: "neo_network", %err, "failed to encode extensible payload");
                return;
            }
        };
        for (peer_id, handle) in self.registry.handles() {
            if let Err(err) = handle.try_send_raw(frame.clone()) {
                warn!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "dropping extensible payload to peer (channel full or closed)"
                );
            }
        }
    }

    /// C# `LocalNode.RelayDirectly`: announce inventory hashes to every peer
    /// via `Inv` (chunked at `InvPayload.MaxHashesCount`); peers pull the full
    /// items they lack via `GetData`.
    async fn handle_broadcast_inv(
        &self,
        inventory_type: crate::InventoryType,
        hashes: Vec<neo_primitives::UInt256>,
    ) {
        if hashes.is_empty() {
            return;
        }
        for group in neo_payloads::inv_payload::InvPayload::create_group(inventory_type, hashes) {
            let frame =
                match crate::wire::Message::create(crate::MessageCommand::Inv, Some(&group), false)
                    .and_then(|message| message.to_bytes())
                {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        warn!(target: "neo_network", %err, "failed to encode inv announcement");
                        return;
                    }
                };
            for (peer_id, handle) in self.registry.handles() {
                if let Err(err) = handle.try_send_raw(frame.clone()) {
                    warn!(
                        target: "neo_network",
                        %peer_id,
                        %err,
                        "dropping inv announcement to peer (channel full or closed)"
                    );
                }
            }
        }
    }

    async fn handle_broadcast_transaction(&self, tx: &Transaction) {
        for (peer_id, handle) in self.registry.handles() {
            if let Err(err) = handle
                .try_send_inventory(crate::remote_node::InventoryItem::Transaction(tx.clone()))
            {
                warn!(
                    target: "neo_network",
                    %peer_id,
                    %err,
                    "dropping transaction inventory to peer (channel full or closed)"
                );
            }
        }
    }

    async fn handle_relay_inventory(&self, hash: neo_primitives::UInt256) {
        trace!(target: "neo_network", ?hash, "relay inventory");
    }

    async fn on_shutdown(&mut self) {
        for (_peer_id, handle) in self.registry.handles() {
            let _ = handle.shutdown().await;
        }
        self.shutdown.cancel();
        // Await the accept loop so we catch any panic it suffered
        // (the cancellation token above unblocks its `select!`).
        if let Some(handle) = self.accept_handle.take() {
            let _ = handle.await;
        }
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
        self.registry.len()
    }

    fn subscribe_events(&self) -> broadcast::Receiver<NetworkEvent> {
        self.event_tx.subscribe()
    }
}

// -----------------------------------------------------------------------------
// Accept loop
// -----------------------------------------------------------------------------

async fn accept_loop(
    listener: TcpListener,
    identity: Arc<LocalIdentity>,
    registry: Arc<PeerRegistry>,
    event_tx: broadcast::Sender<NetworkEvent>,
    shutdown: CancellationToken,
    inbound_tx: Option<mpsc::Sender<InboundInventory>>,
    block_source: Option<Arc<dyn BlockSource>>,
    block_sync_mode: BlockSyncMode,
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
                        let (service, handle) = RemoteNodeService::new(
                            stream,
                            peer_id,
                            remote_addr,
                            identity.clone(),
                            registry.clone(),
                            event_tx.clone(),
                            RemoteNodeState::Handshake,
                            shutdown.clone(),
                        );
                        let service = service.with_block_sync_mode(block_sync_mode);
                        let service = match &inbound_tx {
                            Some(tx) => service.with_inventory_sink(tx.clone()),
                            None => service,
                        };
                        let service = match &block_source {
                            Some(source) => service.with_block_source(Arc::clone(source)),
                            None => service,
                        };
                        // C# `Peer.OnTcpConnected` aborts the TCP
                        // connection *before* creating the RemoteNode
                        // actor when either connection cap is hit, so
                        // a rejected peer never appears in the
                        // connected set and never produces lifecycle
                        // events. Dropping the un-spawned service
                        // closes the stream.
                        if !registry.try_admit(peer_id, remote_addr, handle) {
                            info!(
                                target: "neo_network",
                                %remote_addr,
                                "inbound connection rejected: connection limit reached"
                            );
                            continue;
                        }
                        info!(
                            target: "neo_network",
                            %peer_id,
                            %remote_addr,
                            "inbound connection accepted"
                        );
                        // C# reports a peer's LISTENER port (from its
                        // version payload's TcpServer capability), never
                        // the remote's ephemeral source port, and encodes
                        // an unknown listener as port 0 (RemoteNode.cs:54
                        // initializes ListenerTcpPort = 0). Publish the
                        // C#-faithful unknown form `(remote_ip, 0)` now;
                        // the per-peer service publishes the upgraded
                        // endpoint once the peer's version advertises a
                        // listener port.
                        let reported = SocketAddr::new(remote_addr.ip(), 0);
                        let _ = event_tx.send(RuntimeNetworkEvent::PeerConnected {
                            peer_id: peer_id.to_string(),
                            address: Some(reported),
                        });
                        spawn_guarded("remote_node", service.run());
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
