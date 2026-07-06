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

use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::codec::Framed;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::download::{BlockDownloadBatch, BlockRequest, BlockRequestScheduler};
use crate::wire::MessageCodec;
use neo_payloads::{Block, ExtensiblePayload, Header, Transaction};
use neo_primitives::UInt256;

use crate::connection_timeouts::ConnectionTimeouts;
use crate::error::NetworkResult;
use crate::event::NetworkEvent;
use crate::local_identity::LocalIdentity;
use crate::peer_id::PeerId;
use crate::peer_registry::PeerRegistry;

#[path = "../remote_node/session.rs"]
mod session;

use session::PeerSession;

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
    /// Request a contiguous block range from this peer via `GetBlockByIndex`.
    RequestBlocksByIndex {
        /// Planned range to request.
        request: BlockRequest,
        /// Completion signal after the frame has been sent, queued for the
        /// post-handshake flush, or rejected locally.
        reply: oneshot::Sender<NetworkResult<()>>,
    },
    /// Request and collect a contiguous block range from this peer.
    FetchBlocksByIndex {
        /// Planned range to request.
        request: BlockRequest,
        /// Completion signal carrying the collected block batch.
        reply: oneshot::Sender<NetworkResult<BlockDownloadBatch>>,
    },
    /// Send a pre-encoded wire frame (a complete `Message` byte
    /// sequence) to the peer.
    SendRaw(Vec<u8>),
    /// Send a `GetAddr` message to the peer to solicit its known peers
    /// (C# `LocalNode.NeedMorePeers` → `BroadcastMessage(GetAddr)`). The
    /// session records that it sent `GetAddr` so the peer's `Addr` reply
    /// is accepted (C# `_sentCommands[GetAddr]`).
    SendGetAddr,
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

    /// Ask this peer for a contiguous block range using `GetBlockByIndex`.
    ///
    /// This is the request-side seam used by the future transport-backed
    /// [`crate::BlockRangeFetcher`]. Response correlation remains owned by the
    /// caller/fetcher layer; this method only validates and sends the wire
    /// request through the per-peer session. If the handshake is still
    /// finishing, the session queues the frame and flushes it at `verack`, the
    /// same policy used for outbound inventory.
    pub async fn request_blocks_by_index(&self, request: BlockRequest) -> NetworkResult<()> {
        Self::validate_block_index_request(request)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(RemoteNodeCommand::RequestBlocksByIndex {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)?;
        reply_rx
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)?
    }

    /// Ask this peer for a contiguous block range and wait for the matching
    /// `block` frames.
    ///
    /// This is the peer-level implementation primitive for
    /// [`crate::BlockRangeFetcher`]. The per-peer session accepts only one
    /// explicit fetch at a time so range responses cannot be interleaved
    /// ambiguously; callers that need concurrency should spread assignments
    /// across peers through `BlockDownloadCoordinator`.
    pub async fn fetch_blocks_by_index(
        &self,
        request: BlockRequest,
    ) -> NetworkResult<BlockDownloadBatch> {
        Self::validate_block_index_request(request)?;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(RemoteNodeCommand::FetchBlocksByIndex {
                request,
                reply: reply_tx,
            })
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)?;
        reply_rx
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)?
    }

    fn validate_block_index_request(request: BlockRequest) -> NetworkResult<()> {
        if request.count == 0 {
            return Err(crate::error::NetworkError::Protocol(
                "GetBlockByIndex request count must be greater than zero".to_string(),
            ));
        }
        if request.count > BlockRequestScheduler::MAX_BLOCKS_PER_REQUEST {
            return Err(crate::error::NetworkError::Protocol(format!(
                "GetBlockByIndex request count {} exceeds protocol cap {}",
                request.count,
                BlockRequestScheduler::MAX_BLOCKS_PER_REQUEST
            )));
        }
        i16::try_from(request.count).map(|_| ()).map_err(|_| {
            crate::error::NetworkError::Protocol(format!(
                "GetBlockByIndex request count {} exceeds i16 payload range",
                request.count
            ))
        })
    }

    /// Send a pre-encoded wire frame to the peer.
    pub async fn send_raw(&self, bytes: Vec<u8>) -> NetworkResult<()> {
        self.cmd_tx
            .send(RemoteNodeCommand::SendRaw(bytes))
            .await
            .map_err(|_| crate::error::NetworkError::LocalShuttingDown)
    }

    /// Non-blocking [`Self::send_inventory`]: drops the item (returning an error)
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

    /// Non-blocking [`Self::send_raw`] (see [`Self::try_send_inventory`]).
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

    /// Non-blocking request to send a `GetAddr` to the peer. Best-effort: a
    /// dropped `GetAddr` (channel full) is retried on the next discovery tick.
    pub fn try_send_get_addr(&self) -> NetworkResult<()> {
        use tokio::sync::mpsc::error::TrySendError;
        self.cmd_tx
            .try_send(RemoteNodeCommand::SendGetAddr)
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
            sync_scheduler: BlockRequestScheduler::default(),
            peer_allows_compression: false,
            pending_outbound: Vec::new(),
            get_addr_sent: false,
            inbound_tx,
            block_source,
            pending_block_fetch: None,
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

#[cfg(test)]
#[path = "../tests/service/remote_node.rs"]
mod tests;
