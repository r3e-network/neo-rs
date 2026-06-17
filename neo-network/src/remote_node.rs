//! `RemoteNodeService` — reth-style per-peer state machine.
//!
//! Each TCP connection (accepted or dialed) spawns one
//! `RemoteNodeService` task. The task owns:
//!
//! - a `tokio::net::TcpStream`, wrapped in a
//!   `tokio_util::codec::Framed` with the local wire
//!   [`MessageCodec`] for frame-level decode/encode,
//! - the per-peer handshake state (peer version, verack flag —
//!   C# `RemoteNode.Version` / `RemoteNode._verack`),
//! - a `mpsc::Receiver<RemoteNodeCommand>` for outbound messages
//!   from the local node, and
//! - a `broadcast::Sender<NetworkEvent>` cloned from the local
//!   node's sender for publishing lifecycle events.
//!
//! ## Protocol lifecycle (C# parity)
//!
//! 1. On start the service sends its `version` message
//!    (C# `LocalNode.OnTcpConnected` → `RemoteNode.StartProtocol` →
//!    `OnStartProtocol`, both for inbound and outbound connections).
//! 2. The first inbound message **must** be `version`
//!    (C# `OnMessage` throws `ProtocolViolationException` otherwise).
//!    `OnVersionMessageReceived` captures the peer's user agent,
//!    nonce, full-node flag, and the advertised `TcpServer` listener
//!    port, then applies `LocalNode.AllowNewConnection`: network
//!    mismatch, self-connection (nonce equality), and
//!    duplicate-connection (same address + nonce) all disconnect.
//!    On success a `verack` is sent and — when the advertised
//!    listener port differs from the transport port — the upgraded
//!    `(remote_ip, listener_port)` endpoint is published so
//!    `getpeers` serves the C# `Listener` value.
//! 3. The second inbound message **must** be `verack`; afterwards the
//!    data plane is open. Post-handshake messages other than the
//!    handshake commands are ignored-and-logged: the full protocol
//!    dispatcher (`RemoteNode.ProtocolHandler` message handlers) is
//!    future work, and per C# a repeated `version`/`verack` is a
//!    protocol violation that disconnects.
//! 4. EOF, decode errors, write errors, and inactivity timeouts
//!    (C# `Connection.cs` 10 s initial / 60 s rolling) all tear the
//!    connection down: the peer is removed from the shared
//!    [`PeerRegistry`] and a `PeerDisconnected` event is published —
//!    this task is the *only* publisher of that event for its peer.

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio::time::Instant;
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, trace, warn};

use crate::wire::{Message, MessageCodec};
use neo_io::{MemoryReader, Serializable};
use crate::MessageCommand;
use neo_payloads::p2p_payloads::{
    AddrPayload, GetBlockByIndexPayload, GetBlocksPayload, InvPayload, NetworkAddressWithTime,
    NodeCapability, PingPayload, VersionPayload,
};
use neo_payloads::{Block, ExtensiblePayload, Header, HeadersPayload, Transaction};
use neo_primitives::{InventoryType, UInt256};

use crate::connection_timeouts::ConnectionTimeouts;
use crate::error::NetworkResult;
use crate::event::NetworkEvent;
use crate::local_identity::LocalIdentity;
use crate::peer_id::PeerId;
use crate::peer_registry::PeerRegistry;

/// Inbound inventory decoded from a peer and forwarded to the ledger.
///
/// The per-peer task has no direct blockchain handle (the network layer
/// is decoupled from the ledger, mirroring the C# `NeoSystem` mediator),
/// so a received block/transaction is sent over this channel to the
/// composition root, which forwards it to the blockchain service
/// (`BlockchainCommand::InventoryBlock` / mempool admission).
#[derive(Debug, Clone)]
pub enum InboundInventory {
    /// A full block relayed by a peer (C# `OnInventoryReceived` → `Block`).
    Block(Arc<Block>),
    /// A transaction relayed by a peer (C# `OnInventoryReceived` → `Transaction`).
    Transaction(Arc<Transaction>),
    /// An extensible payload relayed by a peer (C# `OnInventoryReceived` →
    /// `ExtensiblePayload`; carries dBFT consensus + state-root messages).
    Extensible(Arc<ExtensiblePayload>),
}

/// Read-only ledger view used to *serve* blocks to peers that request
/// them (C# `RemoteNode.OnGetBlockByIndexMessageReceived` reads
/// `_system.StoreView`). The network layer is decoupled from the ledger,
/// so the composition root supplies this seam over the persistent store.
pub trait BlockSource: Send + Sync {
    /// Returns the full block at `index`, or `None` when the local
    /// ledger does not (yet) hold it.
    fn block_by_index(&self, index: u32) -> Option<Block>;

    /// Returns the block header at `index` (cheaper than a full block;
    /// used to serve `GetHeaders`). Defaults to deriving it from
    /// [`BlockSource::block_by_index`].
    fn header_by_index(&self, index: u32) -> Option<Header> {
        self.block_by_index(index).map(|block| block.header)
    }

    /// Returns the block hash at `index` (used to serve the legacy `GetBlocks`
    /// inventory response). Defaults to deriving it from
    /// [`BlockSource::block_by_index`].
    fn block_hash_by_index(&self, index: u32) -> Option<UInt256> {
        self.block_by_index(index).map(|block| block.hash())
    }

    /// Returns the full block with `hash` (used to serve `GetData` block
    /// items). Defaults to `None`.
    fn block_by_hash(&self, _hash: &UInt256) -> Option<Block> {
        None
    }

    /// Returns the index of the block with `hash` (used by `GetBlocks` to
    /// resolve the starting point without loading the full block). Defaults to
    /// [`BlockSource::block_by_hash`]`.map(|b| b.index())`.
    fn block_index_by_hash(&self, hash: &UInt256) -> Option<u32> {
        self.block_by_hash(hash).map(|block| block.index())
    }

    /// Returns the transaction with `hash` (used to serve `GetData`
    /// transaction items). Defaults to `None`.
    fn transaction_by_hash(&self, _hash: &UInt256) -> Option<Transaction> {
        None
    }

    /// Returns the extensible payload with `hash` (used to serve `GetData`
    /// extensible items from the relay cache). Defaults to `None`.
    fn extensible_by_hash(&self, _hash: &UInt256) -> Option<ExtensiblePayload> {
        None
    }

    /// Returns `true` if the local node already holds the block with `hash`.
    /// Used to filter `Inv` announcements before pulling them via `GetData`
    /// (C# `RemoteNode.OnInvMessageReceived` known-hash filter). Defaults to
    /// [`BlockSource::block_by_hash`]`.is_some()`.
    fn contains_block(&self, hash: &UInt256) -> bool {
        self.block_by_hash(hash).is_some()
    }

    /// Returns `true` if the local node already holds the transaction with
    /// `hash` (mempool *or* ledger). Used to filter `Inv` announcements before
    /// pulling them via `GetData`. Defaults to
    /// [`BlockSource::transaction_by_hash`]`.is_some()` — implementors with a
    /// mempool should override this to also consult it (an unconfirmed tx is
    /// not in the ledger yet).
    fn contains_transaction(&self, hash: &UInt256) -> bool {
        self.transaction_by_hash(hash).is_some()
    }

    /// Returns the verified mempool transaction hashes, for answering a peer's
    /// `Mempool` request (C# `RemoteNode.OnMemPoolMessageReceived` replies with
    /// `Inv(Transaction, MemoryPool.GetVerifiedTransactions().Hashes)`).
    /// Defaults to empty (a node with no mempool seam serves nothing).
    fn mempool_transaction_hashes(&self) -> Vec<UInt256> {
        Vec::new()
    }
}

/// Framed transport driven by the per-peer read/write loop.
type PeerFramed = Framed<TcpStream, MessageCodec>;

/// Per-peer state machine.
///
/// Mirrors the C# `RemoteNode` lifecycle: open, then either
/// `Handshake` (server side) or `Connecting` (client side), then
/// `Versioned` once our version has been sent, then `Ready` once the
/// peer's `version` *and* `verack` have both been received.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteNodeState {
    /// TCP accepted, awaiting outbound `Version` send.
    Handshake,
    /// Outbound dial completed, awaiting outbound `Version` send.
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
    /// Send a pre-encoded wire frame (a complete `Message` byte
    /// sequence) to the peer.
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
    /// Assemble a handle from its parts (crate-internal; used by the
    /// service constructor and registry unit tests).
    pub(crate) fn from_parts(
        cmd_tx: mpsc::Sender<RemoteNodeCommand>,
        peer_id: PeerId,
        remote_addr: SocketAddr,
    ) -> Self {
        Self {
            cmd_tx,
            peer_id,
            remote_addr,
        }
    }

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

    /// Send a pre-encoded wire frame to the peer.
    pub async fn send_raw(&self, bytes: Vec<u8>) -> NetworkResult<()> {
        self.cmd_tx
            .send(RemoteNodeCommand::SendRaw(bytes))
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
    }

    /// Non-blocking [`send_inventory`]: drops the item (returning an error)
    /// instead of awaiting when the peer's command channel is full, so one slow
    /// peer cannot stall the shared broadcast loop. Best-effort gossip — a
    /// dropped item is re-acquired by the peer via `GetData`/the next sync.
    pub fn try_send_inventory(&self, item: InventoryItem) -> NetworkResult<()> {
        use tokio::sync::mpsc::error::TrySendError;
        self.cmd_tx
            .try_send(RemoteNodeCommand::SendInventory(item))
            .map_err(|err| match err {
                TrySendError::Full(_) => crate::error::NetworkError::RemoteUnavailable {
                    peer_id: format!("{:?}", self.peer_id),
                    detail: "send channel full (slow peer)".to_string(),
                },
                TrySendError::Closed(_) => crate::error::NetworkError::LocalShuttingDown,
            })
    }

    /// Non-blocking [`send_raw`] (see [`try_send_inventory`]).
    pub fn try_send_raw(&self, bytes: Vec<u8>) -> NetworkResult<()> {
        use tokio::sync::mpsc::error::TrySendError;
        self.cmd_tx
            .try_send(RemoteNodeCommand::SendRaw(bytes))
            .map_err(|err| match err {
                TrySendError::Full(_) => crate::error::NetworkError::RemoteUnavailable {
                    peer_id: format!("{:?}", self.peer_id),
                    detail: "send channel full (slow peer)".to_string(),
                },
                TrySendError::Closed(_) => crate::error::NetworkError::LocalShuttingDown,
            })
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
    /// Local node identity used to assemble the outbound version
    /// payload and to detect self-connections.
    identity: Arc<LocalIdentity>,
    /// Shared connected-peer registry (duplicate-connection filter,
    /// self-removal on exit).
    registry: Arc<PeerRegistry>,
    /// State machine value at construction time (the live value is
    /// owned by the running session).
    state: RemoteNodeState,
    /// Per-peer command channel receiver.
    cmd_rx: mpsc::Receiver<RemoteNodeCommand>,
    /// Event broadcast sender.
    event_tx: broadcast::Sender<NetworkEvent>,
    /// Cancellation token shared with the local node; fires on node
    /// shutdown.
    shutdown: CancellationToken,
    /// Inactivity timeouts (C# `Connection.cs` constants by default).
    timeouts: ConnectionTimeouts,
    /// Optional sink for blocks/transactions decoded from this peer,
    /// drained by the composition root into the blockchain service.
    inbound_tx: Option<mpsc::Sender<InboundInventory>>,
    /// Optional read-only ledger view for serving block requests.
    block_source: Option<Arc<dyn BlockSource>>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stream: TcpStream,
        peer_id: PeerId,
        remote_addr: SocketAddr,
        identity: Arc<LocalIdentity>,
        registry: Arc<PeerRegistry>,
        event_tx: broadcast::Sender<NetworkEvent>,
        initial_state: RemoteNodeState,
        shutdown: CancellationToken,
    ) -> (Self, RemoteNodeHandle) {
        let (cmd_tx, cmd_rx) = mpsc::channel(64);
        let handle = RemoteNodeHandle::from_parts(cmd_tx, peer_id, remote_addr);
        let service = Self {
            stream,
            peer_id,
            remote_addr,
            identity,
            registry,
            state: initial_state,
            cmd_rx,
            event_tx,
            shutdown,
            timeouts: ConnectionTimeouts::default(),
            inbound_tx: None,
            block_source: None,
        };
        (service, handle)
    }

    /// Override the connection inactivity timeouts (defaults match
    /// C# `Connection.cs`: 10 s initial, 60 s idle).
    pub fn with_timeouts(mut self, timeouts: ConnectionTimeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Attach the inbound-inventory sink so decoded blocks/transactions
    /// from this peer are forwarded to the ledger.
    pub fn with_inventory_sink(mut self, inbound_tx: mpsc::Sender<InboundInventory>) -> Self {
        self.inbound_tx = Some(inbound_tx);
        self
    }

    /// Attach the read-only ledger view used to serve `GetBlockByIndex`
    /// requests from this peer.
    pub fn with_block_source(mut self, block_source: Arc<dyn BlockSource>) -> Self {
        self.block_source = Some(block_source);
        self
    }

    /// Identifier of the peer this service drives.
    pub fn peer_id(&self) -> PeerId {
        self.peer_id
    }

    /// State machine value at construction time.
    pub fn state(&self) -> RemoteNodeState {
        self.state
    }

    /// Drive the per-peer service until the connection closes (EOF,
    /// decode/write error, inactivity timeout, protocol violation),
    /// a shutdown is requested, or the local node's cancellation
    /// token fires. On exit the peer is removed from the shared
    /// registry and a single `PeerDisconnected` event is published.
    pub async fn run(self) {
        let Self {
            stream,
            peer_id,
            remote_addr,
            identity,
            registry,
            state,
            mut cmd_rx,
            event_tx,
            shutdown,
            timeouts,
            inbound_tx,
            block_source,
        } = self;

        info!(
            target: "neo_network",
            %peer_id,
            %remote_addr,
            ?state,
            "remote node service started"
        );

        let mut framed = Framed::new(stream, MessageCodec::new());
        let mut session = PeerSession {
            peer_id,
            remote_addr,
            identity,
            registry: registry.clone(),
            event_tx: event_tx.clone(),
            state,
            peer_version: None,
            verack_received: false,
            listener_port: 0,
            peer_is_full_node: false,
            peer_last_block_index: 0,
            sync_requested_to: 0,
            sync_last_local_height: 0,
            sync_stall_ticks: 0,
            peer_allows_compression: false,
            pending_outbound: Vec::new(),
            inbound_tx,
            block_source,
        };

        let close_reason = session
            .drive(&mut framed, &mut cmd_rx, &shutdown, timeouts)
            .await;

        // Lifecycle teardown: this task is the single owner of the
        // peer's registry entry and `PeerDisconnected` event, so a
        // peer can never outlive its connection (the recorded
        // inbound-peers-persist-forever blocker).
        registry.remove(peer_id);
        let _ = event_tx.send(NetworkEvent::PeerDisconnected {
            peer_id: peer_id.to_string(),
        });
        info!(
            target: "neo_network",
            %peer_id,
            %remote_addr,
            reason = %close_reason,
            "remote node service exited"
        );
    }
}

/// Why the per-peer session ended. Carried only for logging; every
/// variant tears the connection down the same way.
enum CloseReason {
    /// The remote closed the connection (clean EOF).
    RemoteClosed,
    /// No frame arrived within the inactivity timeout.
    TimedOut,
    /// Local node is shutting down.
    LocalShutdown,
    /// Explicit `Shutdown` command (or all handles dropped).
    ShutdownRequested,
    /// The peer violated the protocol (bad handshake order, invalid
    /// payload, network mismatch, self-connection, duplicate).
    ProtocolViolation(String),
    /// Transport-level failure (read/decode/write error).
    Transport(String),
}

impl fmt::Display for CloseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RemoteClosed => write!(f, "connection closed by peer"),
            Self::TimedOut => write!(f, "connection timed out"),
            Self::LocalShutdown => write!(f, "local node shutting down"),
            Self::ShutdownRequested => write!(f, "shutdown requested"),
            Self::ProtocolViolation(detail) => write!(f, "protocol violation: {detail}"),
            Self::Transport(detail) => write!(f, "transport error: {detail}"),
        }
    }
}

/// Live per-connection protocol state, owned by the running task.
///
/// Split from [`RemoteNodeService`] so the read loop can borrow the
/// framed transport and the command receiver independently of the
/// protocol state.
struct PeerSession {
    peer_id: PeerId,
    remote_addr: SocketAddr,
    identity: Arc<LocalIdentity>,
    registry: Arc<PeerRegistry>,
    event_tx: broadcast::Sender<NetworkEvent>,
    state: RemoteNodeState,
    /// The peer's version payload (C# `RemoteNode.Version`); `None`
    /// until the version message has been received and validated.
    peer_version: Option<VersionPayload>,
    /// Whether the peer's `verack` has been received
    /// (C# `RemoteNode._verack`).
    verack_received: bool,
    /// Listener port advertised via the `TcpServer` capability
    /// (C# `RemoteNode.ListenerTcpPort`; `0` when the peer is not a
    /// server).
    listener_port: u16,
    /// Whether the peer advertised the `FullNode` capability
    /// (C# `RemoteNode.IsFullNode`).
    peer_is_full_node: bool,
    /// The peer's last known block height (C# `RemoteNode.LastBlockIndex`):
    /// seeded from the `FullNode` capability's `StartHeight` and refreshed
    /// by each `ping`/`pong` exchange. Drives the block-sync gate
    /// (`block.Index > LastBlockIndex`) once sync is wired.
    peer_last_block_index: u32,
    /// Highest block index already requested from this peer (the in-flight
    /// high-water mark, C# `TaskSession` assigned tasks). `0` = nothing
    /// requested yet. Lets sync pipeline forward without re-requesting the
    /// same range each tick.
    sync_requested_to: u32,
    /// Persisted height observed at the last sync tick, for stall detection.
    sync_last_local_height: u32,
    /// Consecutive sync ticks with no persisted-height progress while still
    /// trailing the peer; a run of these rewinds the in-flight cursor so a
    /// dropped batch is re-requested (C# `TaskManager` task-timeout reassign).
    sync_stall_ticks: u32,
    /// Whether outbound frames to this peer may be compressed
    /// (C# `VersionPayload.AllowCompression`: no `DisableCompression`
    /// capability present).
    peer_allows_compression: bool,
    /// Outbound messages queued while the handshake is still in
    /// flight, flushed on `verack` (C# `RemoteNode` queues messages
    /// until `_verack` is set).
    pending_outbound: Vec<Message>,
    /// Optional sink for blocks/transactions decoded from this peer.
    inbound_tx: Option<mpsc::Sender<InboundInventory>>,
    /// Optional read-only ledger view for serving block requests.
    block_source: Option<Arc<dyn BlockSource>>,
}

impl PeerSession {
    /// Run the connection: send our version, then loop over inbound
    /// frames, outbound commands, the inactivity deadline, and the
    /// local shutdown token.
    async fn drive(
        &mut self,
        framed: &mut PeerFramed,
        cmd_rx: &mut mpsc::Receiver<RemoteNodeCommand>,
        shutdown: &CancellationToken,
        timeouts: ConnectionTimeouts,
    ) -> CloseReason {
        // C# sends the version message immediately on connect for
        // both inbound and outbound connections
        // (LocalNode.OnTcpConnected → StartProtocol).
        if let Err(reason) = self.send_version(framed).await {
            return reason;
        }
        self.state = RemoteNodeState::Versioned;

        // C# Connection.cs arms a 10 s timer at construction and
        // re-arms a 60 s timer after every receive.
        let mut deadline = Instant::now() + timeouts.initial;
        // C# `RemoteNode.ProtocolHandler` arms a 30 s repeating timer
        // (`TimerInterval`) whose tick sends a `ping`; the first tick is
        // scheduled one interval out so we never ping mid-handshake.
        let ping_interval = Duration::from_secs(30);
        let mut ping_timer =
            tokio::time::interval_at(Instant::now() + ping_interval, ping_interval);
        // Block-sync runs on its own fast cadence (decoupled from the 30 s
        // keepalive ping): each tick pipelines the next batch forward while the
        // ledger trails the peer, instead of one batch per keepalive interval.
        let sync_interval = Duration::from_secs(1);
        let mut sync_timer =
            tokio::time::interval_at(Instant::now() + sync_interval, sync_interval);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    return CloseReason::LocalShutdown;
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return CloseReason::TimedOut;
                }
                _ = ping_timer.tick() => {
                    if let Err(reason) = self.send_ping(framed).await {
                        return reason;
                    }
                }
                _ = sync_timer.tick() => {
                    // C# `TaskManager` timer: keep the block-sync pipeline full
                    // while the ledger trails the peer.
                    if let Err(reason) = self.request_blocks_if_behind(framed).await {
                        return reason;
                    }
                }
                cmd = cmd_rx.recv() => match cmd {
                    Some(RemoteNodeCommand::SendInventory(item)) => {
                        if let Err(reason) = self.on_send_inventory(framed, item).await {
                            return reason;
                        }
                    }
                    Some(RemoteNodeCommand::SendRaw(bytes)) => {
                        if let Err(reason) = self.on_send_raw(framed, bytes).await {
                            return reason;
                        }
                    }
                    Some(RemoteNodeCommand::Shutdown) | None => {
                        return CloseReason::ShutdownRequested;
                    }
                },
                frame = framed.next() => match frame {
                    Some(Ok(message)) => {
                        deadline = Instant::now() + timeouts.idle;
                        if let Err(reason) = self.on_message(framed, message).await {
                            return reason;
                        }
                    }
                    Some(Err(err)) => {
                        return CloseReason::Transport(format!("frame decode failed: {err}"));
                    }
                    None => {
                        return CloseReason::RemoteClosed;
                    }
                },
            }
        }
    }

    /// Send our `version` message (C# `RemoteNode.OnStartProtocol`).
    /// Never compressed: C# passes `Version?.AllowCompression ?? false`
    /// to `Message.ToArray`, and the peer's version is unknown here.
    async fn send_version(&mut self, framed: &mut PeerFramed) -> Result<(), CloseReason> {
        let payload = self.identity.version_payload();
        let message = Message::create(MessageCommand::Version, Some(&payload), false)
            .map_err(|err| CloseReason::Transport(format!("encode version: {err}")))?;
        framed
            .send(message)
            .await
            .map_err(|err| CloseReason::Transport(format!("send version: {err}")))?;
        debug!(
            target: "neo_network",
            peer_id = %self.peer_id,
            "version sent"
        );
        Ok(())
    }

    /// C# `RemoteNode.ProtocolHandler.OnTimer`: once the handshake has
    /// completed, send a periodic `ping` carrying our current block height
    /// so an idle-but-healthy connection is not dropped by the peer's 60 s
    /// idle timer. A no-op while the handshake is still in flight.
    async fn send_ping(&mut self, framed: &mut PeerFramed) -> Result<(), CloseReason> {
        if !self.verack_received {
            return Ok(());
        }
        let payload = PingPayload::create(self.identity.block_height());
        let message = Message::create(
            MessageCommand::Ping,
            Some(&payload),
            self.peer_allows_compression,
        )
        .map_err(|err| CloseReason::Transport(format!("encode ping: {err}")))?;
        framed
            .send(message)
            .await
            .map_err(|err| CloseReason::Transport(format!("send ping: {err}")))?;
        Ok(())
    }

    /// C# `TaskManager` block-sync request: while this peer is ahead of our
    /// ledger, request the next batch of blocks by index
    /// (`GetBlockByIndex`). The peer replies with `block` frames, which the
    /// inbound-inventory sink forwards to the blockchain service; as the
    /// ledger advances and the shared height updates, the next request asks
    /// for the new tip. Redundant requests across peers are deduplicated by
    /// the blockchain service (already-persisted blocks are dropped), so a
    /// per-peer trigger needs no cross-peer range assignment to be correct.
    async fn request_blocks_if_behind(
        &mut self,
        framed: &mut PeerFramed,
    ) -> Result<(), CloseReason> {
        if !self.verack_received {
            return Ok(());
        }
        let local_height = self.identity.block_height();
        let peer_height = self.peer_last_block_index;

        // Caught up: reset the in-flight cursor + stall tracker so a future
        // divergence restarts the pipeline cleanly.
        if peer_height <= local_height {
            self.sync_requested_to = local_height;
            self.sync_last_local_height = local_height;
            self.sync_stall_ticks = 0;
            return Ok(());
        }

        // Stall detection: if the persisted height has not advanced across
        // several ticks while we still trail the peer, the in-flight batch was
        // lost — rewind the cursor to re-request the gap (C# `TaskManager`
        // reassigns a timed-out task to another peer).
        if local_height == self.sync_last_local_height {
            self.sync_stall_ticks = self.sync_stall_ticks.saturating_add(1);
        } else {
            self.sync_stall_ticks = 0;
            self.sync_last_local_height = local_height;
        }
        const STALL_LIMIT: u32 = 5;
        if self.sync_stall_ticks >= STALL_LIMIT {
            self.sync_requested_to = local_height;
            self.sync_stall_ticks = 0;
        }

        // Pipeline forward from the in-flight high-water mark (C# `TaskManager.
        // RequestTasks`, TaskManager.cs:400-409): request the next contiguous run
        // the peer holds, never behind the persisted tip, never past the peer's
        // advertised height, and never more than `MaxHashesCount` (500) ahead of
        // the persisted tip — the back-pressure that keeps look-ahead bounded.
        // `count = Math.Min(endHeight - startHeight, MaxHashesCount)`.
        const MAX_HASHES: u32 = 500;
        let start = (local_height + 1).max(self.sync_requested_to + 1);
        if start > peer_height || start >= local_height.saturating_add(MAX_HASHES) {
            return Ok(());
        }
        let upper = peer_height.min(local_height.saturating_add(MAX_HASHES));
        let count = (upper - start + 1).min(MAX_HASHES);
        let payload = GetBlockByIndexPayload::create(start, count as i16);
        let message = Message::create(
            MessageCommand::GetBlockByIndex,
            Some(&payload),
            self.peer_allows_compression,
        )
        .map_err(|err| CloseReason::Transport(format!("encode getblockbyindex: {err}")))?;
        framed
            .send(message)
            .await
            .map_err(|err| CloseReason::Transport(format!("send getblockbyindex: {err}")))?;
        self.sync_requested_to = start + count - 1;
        debug!(
            target: "neo_network",
            peer_id = %self.peer_id,
            from = start,
            count,
            peer_height,
            "requesting block batch from peer"
        );
        Ok(())
    }

    /// Dispatch one inbound frame, enforcing the C#
    /// `RemoteNode.ProtocolHandler.OnMessage` handshake ordering.
    async fn on_message(
        &mut self,
        framed: &mut PeerFramed,
        message: Message,
    ) -> Result<(), CloseReason> {
        if self.peer_version.is_none() {
            if message.command != MessageCommand::Version {
                return Err(CloseReason::ProtocolViolation(format!(
                    "expected version, received {:?}",
                    message.command
                )));
            }
            return self.on_version_message(framed, &message.payload_raw).await;
        }
        if !self.verack_received {
            if message.command != MessageCommand::Verack {
                return Err(CloseReason::ProtocolViolation(format!(
                    "expected verack, received {:?}",
                    message.command
                )));
            }
            return self.on_verack_message(framed).await;
        }
        match message.command {
            // C# treats a repeated version/verack after the handshake
            // as a ProtocolViolationException.
            MessageCommand::Version | MessageCommand::Verack => {
                Err(CloseReason::ProtocolViolation(format!(
                    "unexpected {:?} after handshake",
                    message.command
                )))
            }
            // C# `RemoteNode.ProtocolHandler.OnPingMessageReceived`: record
            // the peer's advertised height and reply with our own ping
            // payload as a pong. The inbound frame already reset the idle
            // deadline in the drive loop (C# `Connection` 60 s timer reset).
            MessageCommand::Ping => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = PingPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid ping payload: {err}"))
                })?;
                self.peer_last_block_index = payload.last_block_index;
                let pong =
                    PingPayload::create_with_nonce(self.identity.block_height(), payload.nonce);
                let message = Message::create(
                    MessageCommand::Pong,
                    Some(&pong),
                    self.peer_allows_compression,
                )
                .map_err(|err| CloseReason::Transport(format!("encode pong: {err}")))?;
                framed
                    .send(message)
                    .await
                    .map_err(|err| CloseReason::Transport(format!("send pong: {err}")))?;
                Ok(())
            }
            // C# `OnPongMessageReceived`: refresh the peer's reported height.
            MessageCommand::Pong => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = PingPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid pong payload: {err}"))
                })?;
                self.peer_last_block_index = payload.last_block_index;
                Ok(())
            }
            // C# `OnInventoryReceived` for a relayed `Block`: decode and
            // forward to the ledger via the inbound-inventory sink. The
            // blockchain service applies the C# `Blockchain.OnNewBlock`
            // sequencing (persist when it is the next block, park when
            // ahead, drop when already known).
            MessageCommand::Block => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let block = Block::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid block payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx.send(InboundInventory::Block(Arc::new(block))).await;
                }
                Ok(())
            }
            // C# `OnGetBlockByIndexMessageReceived`: serve the requested
            // blocks `[IndexStart, IndexStart + min(Count, 500))` from the
            // local ledger as `block` frames, stopping at the first block we
            // do not hold (matching C#'s `GetBlock == null` break).
            MessageCommand::GetBlockByIndex => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlockByIndexPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!(
                        "invalid getblockbyindex payload: {err}"
                    ))
                })?;
                if let Some(source) = self.block_source.clone() {
                    // C# caps a response at `InvPayload.MaxHashesCount` (500);
                    // `Count == -1` means "as many as available".
                    let count = if payload.count < 0 {
                        500u32
                    } else {
                        (payload.count as u32).min(500)
                    };
                    let end = payload.index_start.saturating_add(count);
                    for index in payload.index_start..end {
                        let Some(block) = source.block_by_index(index) else {
                            break;
                        };
                        let served = Message::create(
                            MessageCommand::Block,
                            Some(&block),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode served block: {err}"))
                        })?;
                        framed.send(served).await.map_err(|err| {
                            CloseReason::Transport(format!("send served block: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetBlocksMessageReceived`: starting just after the block
            // named by `hash_start`, reply with an `Inv` of up to `count`
            // (default/-1 => MaxHashesCount 500) subsequent block hashes from
            // the local chain. The legacy hash-based sync request, kept for
            // compatibility alongside `GetBlockByIndex`.
            MessageCommand::GetBlocks => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlocksPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getblocks payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    if let Some(start_index) = source.block_index_by_hash(&payload.hash_start) {
                        let count = if payload.count < 0 {
                            neo_payloads::inv_payload::MAX_HASHES_COUNT as u32
                        } else {
                            (payload.count as u32)
                                .min(neo_payloads::inv_payload::MAX_HASHES_COUNT as u32)
                        };
                        let mut hashes = Vec::new();
                        for offset in 1..=count {
                            match source.block_hash_by_index(start_index.saturating_add(offset)) {
                                Some(hash) => hashes.push(hash),
                                None => break,
                            }
                        }
                        for group in InvPayload::create_group(InventoryType::Block, hashes) {
                            let inv = Message::create(
                                MessageCommand::Inv,
                                Some(&group),
                                self.peer_allows_compression,
                            )
                            .map_err(|err| {
                                CloseReason::Transport(format!("encode getblocks inv: {err}"))
                            })?;
                            framed.send(inv).await.map_err(|err| {
                                CloseReason::Transport(format!("send getblocks inv: {err}"))
                            })?;
                        }
                    }
                }
                Ok(())
            }
            // C# `OnGetHeadersMessageReceived`: serve up to 2000 headers from
            // `IndexStart` as a single `headers` frame (HeadersPayload).
            MessageCommand::GetHeaders => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlockByIndexPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getheaders payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    // C# `HeadersPayload.MaxHeadersCount` is 2000.
                    let count = if payload.count < 0 {
                        2000u32
                    } else {
                        (payload.count as u32).min(2000)
                    };
                    let mut headers = Vec::new();
                    for index in payload.index_start..payload.index_start.saturating_add(count) {
                        match source.header_by_index(index) {
                            Some(header) => headers.push(header),
                            None => break,
                        }
                    }
                    if !headers.is_empty() {
                        let hp = HeadersPayload::create(headers);
                        let served = Message::create(
                            MessageCommand::Headers,
                            Some(&hp),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| CloseReason::Transport(format!("encode headers: {err}")))?;
                        framed.send(served).await.map_err(|err| {
                            CloseReason::Transport(format!("send headers: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetDataMessageReceived`: for each requested inventory hash,
            // serve the matching block / transaction / extensible frame.
            MessageCommand::GetData => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = InvPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getdata payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    let mut not_found = Vec::new();
                    for hash in &payload.hashes {
                        match payload.inventory_type {
                            InventoryType::Block => {
                                if let Some(block) = source.block_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Block,
                                        Some(&block),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "encode getdata block: {err}"
                                        ))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!("send getdata block: {err}"))
                                    })?;
                                } else {
                                    not_found.push(*hash);
                                }
                            }
                            InventoryType::Transaction => {
                                if let Some(tx) = source.transaction_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Transaction,
                                        Some(&tx),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!("encode getdata tx: {err}"))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!("send getdata tx: {err}"))
                                    })?;
                                } else {
                                    not_found.push(*hash);
                                }
                            }
                            InventoryType::Extensible => {
                                if let Some(payload) = source.extensible_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Extensible,
                                        Some(&payload),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "encode getdata extensible: {err}"
                                        ))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "send getdata extensible: {err}"
                                        ))
                                    })?;
                                }
                            }
                        }
                    }
                    for group in InvPayload::create_group(payload.inventory_type, not_found) {
                        let not_found = Message::create(
                            MessageCommand::NotFound,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode getdata notfound: {err}"))
                        })?;
                        framed.send(not_found).await.map_err(|err| {
                            CloseReason::Transport(format!("send getdata notfound: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnInventoryReceived` for a relayed `Transaction`: decode
            // and forward for mempool admission.
            MessageCommand::Transaction => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let transaction = Transaction::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid transaction payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx
                        .send(InboundInventory::Transaction(Arc::new(transaction)))
                        .await;
                }
                Ok(())
            }
            // C# `OnInventoryReceived` for an `ExtensiblePayload`: decode and
            // forward to the ledger/consensus relay (dBFT + state-root votes).
            MessageCommand::Extensible => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = ExtensiblePayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid extensible payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx
                        .send(InboundInventory::Extensible(Arc::new(payload)))
                        .await;
                }
                Ok(())
            }
            // C# `RemoteNode.OnInvMessageReceived`: a peer announces inventory;
            // pull the items we don't already hold via `GetData`.
            MessageCommand::Inv => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = InvPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid inv payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    let unknown: Vec<UInt256> = payload
                        .hashes
                        .iter()
                        .copied()
                        .filter(|hash| match payload.inventory_type {
                            InventoryType::Block => !source.contains_block(hash),
                            InventoryType::Transaction => !source.contains_transaction(hash),
                            // Neo N3 pulls ExtensiblePayload inventory by hash;
                            // consensus and state-service payloads are both
                            // carried by MessageCommand::Extensible.
                            InventoryType::Extensible => true,
                        })
                        .collect();
                    for group in InvPayload::create_group(payload.inventory_type, unknown) {
                        let getdata = Message::create(
                            MessageCommand::GetData,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| CloseReason::Transport(format!("encode getdata: {err}")))?;
                        framed.send(getdata).await.map_err(|err| {
                            CloseReason::Transport(format!("send getdata: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `RemoteNode.OnMemPoolMessageReceived`: reply with `Inv`
            // announcements of every verified mempool transaction.
            MessageCommand::Mempool => {
                if let Some(source) = self.block_source.clone() {
                    let hashes = source.mempool_transaction_hashes();
                    for group in InvPayload::create_group(InventoryType::Transaction, hashes) {
                        let inv = Message::create(
                            MessageCommand::Inv,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode mempool inv: {err}"))
                        })?;
                        framed.send(inv).await.map_err(|err| {
                            CloseReason::Transport(format!("send mempool inv: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetAddrMessageReceived`: gossip up to `MAX_COUNT_TO_SEND`
            // connected peers' advertised listener endpoints (deduplicated,
            // excluding the requester) as a single `Addr` frame.
            MessageCommand::GetAddr => {
                let addrs = self.registry.listener_addresses(
                    self.peer_id,
                    neo_payloads::addr_payload::MAX_COUNT_TO_SEND,
                );
                if !addrs.is_empty() {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as u32)
                        .unwrap_or(0);
                    let entries: Vec<NetworkAddressWithTime> = addrs
                        .into_iter()
                        .map(|addr| {
                            NetworkAddressWithTime::new(
                                timestamp,
                                addr.ip(),
                                vec![NodeCapability::TcpServer { port: addr.port() }],
                            )
                        })
                        .collect();
                    let payload = AddrPayload::create(entries);
                    let served = Message::create(
                        MessageCommand::Addr,
                        Some(&payload),
                        self.peer_allows_compression,
                    )
                    .map_err(|err| CloseReason::Transport(format!("encode addr: {err}")))?;
                    framed
                        .send(served)
                        .await
                        .map_err(|err| CloseReason::Transport(format!("send addr: {err}")))?;
                }
                Ok(())
            }
            other => {
                // Genuine no-ops for this node profile. C# default arm:
                // Alert/MerkleBlock/NotFound/Reject/FilterAdd/FilterClear/
                // FilterLoad. `Addr` is also ignored here, matching C#
                // `OnAddrMessageReceived` (`if (!sent) return;`): this node
                // never sends `GetAddr`, so unsolicited `Addr` is dropped.
                trace!(
                    target: "neo_network",
                    peer_id = %self.peer_id,
                    command = ?other,
                    payload_len = message.payload_raw.len(),
                    "no-op post-handshake message for this node profile"
                );
                Ok(())
            }
        }
    }

    /// C# `RemoteNode.ProtocolHandler.OnVersionMessageReceived` +
    /// `LocalNode.AllowNewConnection`.
    async fn on_version_message(
        &mut self,
        framed: &mut PeerFramed,
        payload_raw: &[u8],
    ) -> Result<(), CloseReason> {
        let mut reader = MemoryReader::new(payload_raw);
        let payload = VersionPayload::deserialize(&mut reader).map_err(|err| {
            CloseReason::ProtocolViolation(format!("invalid version payload: {err}"))
        })?;

        // Capability capture (OnVersionMessageReceived loop).
        for capability in &payload.capabilities {
            match capability {
                NodeCapability::FullNode { start_height } => {
                    self.peer_is_full_node = true;
                    // C# `RemoteNode.LastBlockIndex = StartHeight` on the
                    // FullNode capability (RemoteNode.ProtocolHandler.cs:403).
                    self.peer_last_block_index = *start_height;
                }
                NodeCapability::TcpServer { port } => {
                    self.listener_port = *port;
                }
                _ => {}
            }
        }
        // C# `VersionPayload.AllowCompression` is derived from the
        // capability list on deserialize.
        self.peer_allows_compression = !payload
            .capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression));

        // C# LocalNode.AllowNewConnection, in order.
        if payload.network != self.identity.network() {
            return Err(CloseReason::ProtocolViolation(format!(
                "network mismatch: peer {} != local {}",
                payload.network,
                self.identity.network()
            )));
        }
        if payload.nonce == self.identity.nonce() {
            return Err(CloseReason::ProtocolViolation(
                "self-connection rejected (version nonce equals local nonce)".to_string(),
            ));
        }
        if !self
            .registry
            .record_version_nonce(self.peer_id, payload.nonce)
        {
            return Err(CloseReason::ProtocolViolation(format!(
                "duplicate connection from {} with version nonce {}",
                self.remote_addr.ip(),
                payload.nonce
            )));
        }

        // Address upgrade: C# replaces the transport endpoint with
        // `node.Listener` when the advertised listener port differs
        // (`ConnectedPeers.TryUpdate(actor, node.Listener, node.Remote)`),
        // which is what `getpeers` ultimately reports. The handle-side
        // tracker folds a repeated `PeerConnected` for a known peer id
        // as an address update, not a new peer.
        if self.listener_port != 0 && self.listener_port != self.remote_addr.port() {
            let upgraded = SocketAddr::new(self.remote_addr.ip(), self.listener_port);
            let _ = self.event_tx.send(NetworkEvent::PeerConnected {
                peer_id: self.peer_id.to_string(),
                address: Some(upgraded),
            });
        }

        // Record the advertised listener endpoint so this peer can be gossiped
        // in `GetAddr` responses (C# `RemoteNode.Listener`).
        if self.listener_port != 0 {
            self.registry.record_listener_addr(
                self.peer_id,
                SocketAddr::new(self.remote_addr.ip(), self.listener_port),
            );
        }

        info!(
            target: "neo_network",
            peer_id = %self.peer_id,
            remote_addr = %self.remote_addr,
            user_agent = %payload.user_agent,
            nonce = payload.nonce,
            listener_port = self.listener_port,
            full_node = self.peer_is_full_node,
            "version received"
        );
        self.peer_version = Some(payload);

        // Respond with verack (end of OnVersionMessageReceived).
        let verack = Message::from_payload_bytes(MessageCommand::Verack, Vec::new(), false)
            .map_err(|err| CloseReason::Transport(format!("encode verack: {err}")))?;
        framed
            .send(verack)
            .await
            .map_err(|err| CloseReason::Transport(format!("send verack: {err}")))?;
        Ok(())
    }

    /// C# `RemoteNode.ProtocolHandler.OnVerackMessageReceived`: mark
    /// the data plane open and flush messages queued mid-handshake.
    async fn on_verack_message(&mut self, framed: &mut PeerFramed) -> Result<(), CloseReason> {
        self.verack_received = true;
        self.state = RemoteNodeState::Ready;
        debug!(
            target: "neo_network",
            peer_id = %self.peer_id,
            "handshake complete"
        );
        let pending = std::mem::take(&mut self.pending_outbound);
        for message in pending {
            framed
                .send(message)
                .await
                .map_err(|err| CloseReason::Transport(format!("flush queued message: {err}")))?;
        }
        // C# `TaskManager` kicks off block sync as soon as a peer is ready:
        // if this peer is ahead of our ledger, request the first batch now
        // rather than waiting for the periodic timer.
        self.request_blocks_if_behind(framed).await?;
        Ok(())
    }

    /// C# `RemoteNode.OnSend`: inventory is only relayed to full
    /// nodes (so anything arriving before the peer's version is
    /// dropped — `IsFullNode` is still `false` then, exactly like
    /// C#), and queued until `verack` once the version is known.
    async fn on_send_inventory(
        &mut self,
        framed: &mut PeerFramed,
        item: InventoryItem,
    ) -> Result<(), CloseReason> {
        if !self.peer_is_full_node {
            debug!(
                target: "neo_network",
                peer_id = %self.peer_id,
                "dropping inventory: peer has not advertised the FullNode capability"
            );
            return Ok(());
        }
        let message = match &item {
            InventoryItem::Block(block) => Message::create(
                MessageCommand::Block,
                Some(block),
                self.peer_allows_compression,
            ),
            InventoryItem::Transaction(tx) => Message::create(
                MessageCommand::Transaction,
                Some(tx),
                self.peer_allows_compression,
            ),
        }
        .map_err(|err| CloseReason::Transport(format!("encode inventory: {err}")))?;
        self.send_or_queue(framed, message).await
    }

    /// Handler for [`RemoteNodeCommand::SendRaw`]: the bytes must be
    /// a complete wire frame, which is re-framed through the codec so
    /// a malformed buffer can never corrupt the stream framing. A
    /// malformed buffer is a local caller bug, so it is logged and
    /// dropped rather than blamed on the peer.
    async fn on_send_raw(
        &mut self,
        framed: &mut PeerFramed,
        bytes: Vec<u8>,
    ) -> Result<(), CloseReason> {
        if bytes.is_empty() {
            warn!(
                target: "neo_network",
                peer_id = %self.peer_id,
                "send_raw called with empty payload"
            );
            return Ok(());
        }
        match Message::from_bytes(&bytes) {
            Ok(message) => self.send_or_queue(framed, message).await,
            Err(err) => {
                warn!(
                    target: "neo_network",
                    peer_id = %self.peer_id,
                    %err,
                    "send_raw dropped: bytes are not a valid wire frame"
                );
                Ok(())
            }
        }
    }

    /// Write a message now when the handshake is complete, otherwise
    /// queue it for the `verack` flush (C# `EnqueueMessage` +
    /// `CheckMessageQueue` gating on `_verack`).
    async fn send_or_queue(
        &mut self,
        framed: &mut PeerFramed,
        message: Message,
    ) -> Result<(), CloseReason> {
        if self.state == RemoteNodeState::Ready {
            framed
                .send(message)
                .await
                .map_err(|err| CloseReason::Transport(format!("send message: {err}")))?;
        } else {
            self.pending_outbound.push(message);
        }
        Ok(())
    }
}

#[cfg(test)]
mod broadcast_tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn try_send_raw_drops_on_full_channel_without_blocking() {
        let (tx, _rx) = mpsc::channel::<RemoteNodeCommand>(2);
        let addr = "10.0.0.2:1002".parse().expect("addr");
        let handle = RemoteNodeHandle::from_parts(tx, PeerId::new(), addr);
        assert!(handle.try_send_raw(vec![1]).is_ok());
        assert!(handle.try_send_raw(vec![2]).is_ok());
        // The channel is full and `_rx` is never polled: try_send must return
        // Err immediately rather than parking the shared broadcast loop.
        let res = tokio::time::timeout(std::time::Duration::from_millis(200), async {
            handle.try_send_raw(vec![3])
        })
        .await
        .expect("try_send must not block on a full channel");
        assert!(res.is_err(), "a full peer channel must drop, not block");
    }
}
