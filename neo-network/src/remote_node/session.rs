use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::{
    BlockSource, InboundInventory, InventoryItem, PeerFramed, RemoteNodeCommand, RemoteNodeState,
};
use crate::MessageCommand;
use crate::connection_timeouts::ConnectionTimeouts;
use crate::download::{BlockDownloadBatch, BlockRequest, BlockRequestScheduler};
use crate::error::NetworkError;
use crate::event::NetworkEvent;
use crate::local_identity::LocalIdentity;
use crate::peer_id::PeerId;
use crate::peer_registry::PeerRegistry;
use crate::service::block_sync_mode::BlockSyncMode;
use crate::wire::Message;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::p2p_payloads::{
    GetBlockByIndexPayload, NodeCapability, PingPayload, VersionPayload,
};

#[path = "session/messages.rs"]
mod messages;

/// Why the per-peer session ended. Carried only for logging; every
/// variant tears the connection down the same way.
pub(super) enum CloseReason {
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
pub(super) struct PeerSession {
    pub(super) peer_id: PeerId,
    pub(super) remote_addr: SocketAddr,
    pub(super) identity: Arc<LocalIdentity>,
    pub(super) registry: Arc<PeerRegistry>,
    pub(super) event_tx: broadcast::Sender<NetworkEvent>,
    pub(super) state: RemoteNodeState,
    /// The peer's version payload (C# `RemoteNode.Version`); `None`
    /// until the version message has been received and validated.
    pub(super) peer_version: Option<VersionPayload>,
    /// Whether the peer's `verack` has been received
    /// (C# `RemoteNode._verack`).
    pub(super) verack_received: bool,
    /// Listener port advertised via the `TcpServer` capability
    /// (C# `RemoteNode.ListenerTcpPort`; `0` when the peer is not a
    /// server).
    pub(super) listener_port: u16,
    /// Whether the peer advertised the `FullNode` capability
    /// (C# `RemoteNode.IsFullNode`).
    pub(super) peer_is_full_node: bool,
    /// The peer's last known block height (C# `RemoteNode.LastBlockIndex`):
    /// seeded from the `FullNode` capability's `StartHeight` and refreshed
    /// by each `ping`/`pong` exchange. Drives the block-sync gate
    /// (`block.Index > LastBlockIndex`) once sync is wired.
    pub(super) peer_last_block_index: u32,
    /// Per-peer block-sync request planner.
    pub(super) sync_scheduler: BlockRequestScheduler,
    /// Owner of outbound block range requests.
    pub(super) block_sync_mode: BlockSyncMode,
    /// Whether outbound frames to this peer may be compressed
    /// (C# `VersionPayload.AllowCompression`: no `DisableCompression`
    /// capability present).
    pub(super) peer_allows_compression: bool,
    /// Outbound messages queued while the handshake is still in
    /// flight, flushed on `verack` (C# `RemoteNode` queues messages
    /// until `_verack` is set).
    pub(super) pending_outbound: Vec<Message>,
    /// Whether we have sent a `GetAddr` to this peer and are awaiting its
    /// `Addr` reply (C# `RemoteNode._sentCommands[GetAddr]`). Only a
    /// solicited `Addr` is ingested; an unsolicited one is dropped
    /// (C# `OnAddrMessageReceived`: `if (!sent) return;`). Reset to
    /// `false` once the reply is consumed so each `Addr` matches one
    /// `GetAddr`.
    pub(super) get_addr_sent: bool,
    /// Optional sink for blocks/transactions decoded from this peer.
    pub(super) inbound_tx: Option<mpsc::Sender<InboundInventory>>,
    /// Optional read-only ledger view for serving block requests.
    pub(super) block_source: Option<Arc<dyn BlockSource>>,
    /// Explicit block range fetch currently awaiting `block` responses.
    pub(super) pending_block_fetch: Option<PendingBlockFetch>,
}

pub(super) struct PendingBlockFetch {
    request: BlockRequest,
    next_index: u32,
    blocks: Vec<neo_payloads::Block>,
    reply: oneshot::Sender<crate::NetworkResult<BlockDownloadBatch>>,
}

impl PeerSession {
    /// Run the connection: send our version, then loop over inbound
    /// frames, outbound commands, the inactivity deadline, and the
    /// local shutdown token.
    pub(super) async fn drive(
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
        let sync_interval = Duration::from_millis(100);
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
                // Message reads are placed before timer ticks so that
                // incoming frames are always processed with priority,
                // preventing starvation when timers fire concurrently
                // (the sync timer runs on a fast 100 ms cadence).
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
                _ = ping_timer.tick() => {
                    if let Err(reason) = self.send_ping(framed).await {
                        return reason;
                    }
                }
                _ = sync_timer.tick() => {
                    // C# `TaskManager` timer: keep the block-sync pipeline full
                    // while the ledger trails the peer.
                    if self.block_sync_mode.uses_legacy_per_peer_requests()
                        && let Err(reason) = self.request_blocks_if_behind(framed).await
                    {
                        return reason;
                    }
                }
                cmd = cmd_rx.recv() => match cmd {
                    Some(RemoteNodeCommand::SendInventory(item)) => {
                        if let Err(reason) = self.on_send_inventory(framed, item).await {
                            return reason;
                        }
                    }
                    Some(RemoteNodeCommand::RequestBlocksByIndex { request, reply }) => {
                        match self.send_get_block_by_index(framed, request).await {
                            Ok(()) => {
                                let _ = reply.send(Ok(()));
                            }
                            Err(reason) => {
                                let _ = reply.send(Err(self.network_error_for_close_reason(&reason)));
                                return reason;
                            }
                        }
                    }
                    Some(RemoteNodeCommand::FetchBlocksByIndex { request, reply }) => {
                        if self.pending_block_fetch.is_some() {
                            let _ = reply.send(Err(NetworkError::Protocol(
                                "block range fetch already in flight for this peer".to_string(),
                            )));
                            continue;
                        }
                        match self.send_get_block_by_index(framed, request).await {
                            Ok(()) => {
                                self.pending_block_fetch = Some(PendingBlockFetch {
                                    request,
                                    next_index: request.start,
                                    blocks: Vec::with_capacity(request.count as usize),
                                    reply,
                                });
                            }
                            Err(reason) => {
                                let _ = reply.send(Err(self.network_error_for_close_reason(&reason)));
                                return reason;
                            }
                        }
                    }
                    Some(RemoteNodeCommand::SendRaw(bytes)) => {
                        if let Err(reason) = self.on_send_raw(framed, bytes).await {
                            return reason;
                        }
                    }
                    Some(RemoteNodeCommand::SendGetAddr) => {
                        if let Err(reason) = self.send_get_addr(framed).await {
                            return reason;
                        }
                    }
                    Some(RemoteNodeCommand::Shutdown) | None => {
                        return CloseReason::ShutdownRequested;
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

    /// C# `LocalNode.NeedMorePeers` → `BroadcastMessage(GetAddr)`: solicit
    /// the peer's known addresses so the mesh can grow beyond the seed list.
    /// A no-op until the handshake completes (`GetAddr` is a post-handshake
    /// message; queuing it mid-handshake would race the version exchange).
    /// Records that we sent `GetAddr` so the peer's `Addr` reply is accepted
    /// (C# `_sentCommands[GetAddr]`).
    async fn send_get_addr(&mut self, framed: &mut PeerFramed) -> Result<(), CloseReason> {
        if !self.verack_received {
            return Ok(());
        }
        let message = Message::from_payload_bytes(MessageCommand::GetAddr, Vec::new(), false)
            .map_err(|err| CloseReason::Transport(format!("encode getaddr: {err}")))?;
        framed
            .send(message)
            .await
            .map_err(|err| CloseReason::Transport(format!("send getaddr: {err}")))?;
        self.get_addr_sent = true;
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
            self.sync_scheduler.record_tick(local_height, peer_height);
            let _ = self.sync_scheduler.next_request(local_height, peer_height);
            return Ok(());
        }

        self.sync_scheduler.record_tick(local_height, peer_height);

        // Pipeline forward from the in-flight high-water mark (C# `TaskManager.
        // RequestTasks`, TaskManager.cs:400-409): request the next contiguous
        // run the peer holds while preserving the per-message protocol cap.
        // The ahead window keeps two batches queued per peer, so aggregate
        // throughput can reach N_peers x 1000 blocks of in-flight work without
        // sending an invalid over-sized request.
        let Some(request) = self.sync_scheduler.next_request(local_height, peer_height) else {
            return Ok(());
        };
        self.send_get_block_by_index(framed, request).await?;
        debug!(
            target: "neo_network",
            peer_id = %self.peer_id,
            from = request.start,
            count = request.count,
            peer_height,
            "requesting block batch from peer"
        );
        Ok(())
    }

    fn validate_get_block_by_index_request(request: BlockRequest) -> Result<i16, CloseReason> {
        if request.count == 0 {
            return Err(CloseReason::ProtocolViolation(
                "GetBlockByIndex request count must be greater than zero".to_string(),
            ));
        }
        if request.count > BlockRequestScheduler::MAX_BLOCKS_PER_REQUEST {
            return Err(CloseReason::ProtocolViolation(format!(
                "GetBlockByIndex request count {} exceeds protocol cap {}",
                request.count,
                BlockRequestScheduler::MAX_BLOCKS_PER_REQUEST
            )));
        }
        i16::try_from(request.count).map_err(|_| {
            CloseReason::ProtocolViolation(format!(
                "GetBlockByIndex request count {} exceeds i16 payload range",
                request.count
            ))
        })
    }

    async fn send_get_block_by_index(
        &mut self,
        framed: &mut PeerFramed,
        request: BlockRequest,
    ) -> Result<(), CloseReason> {
        let count = Self::validate_get_block_by_index_request(request)?;
        let payload = GetBlockByIndexPayload::create(request.start, count);
        let message = Message::create(
            MessageCommand::GetBlockByIndex,
            Some(&payload),
            self.peer_allows_compression,
        )
        .map_err(|err| CloseReason::Transport(format!("encode getblockbyindex: {err}")))?;
        self.send_or_queue(framed, message).await
    }

    fn network_error_for_close_reason(&self, reason: &CloseReason) -> NetworkError {
        match reason {
            CloseReason::LocalShutdown | CloseReason::ShutdownRequested => {
                NetworkError::LocalShuttingDown
            }
            CloseReason::RemoteClosed | CloseReason::TimedOut | CloseReason::Transport(_) => {
                NetworkError::RemoteUnavailable {
                    peer_id: self.peer_id.to_string(),
                    detail: reason.to_string(),
                }
            }
            CloseReason::ProtocolViolation(detail) => NetworkError::Protocol(detail.clone()),
        }
    }

    pub(super) fn accept_pending_block_fetch(
        &mut self,
        block: neo_payloads::Block,
    ) -> Option<neo_payloads::Block> {
        let Some(fetch) = self.pending_block_fetch.as_mut() else {
            return Some(block);
        };
        if block.index() != fetch.next_index {
            return Some(block);
        }

        fetch.next_index = fetch.next_index.saturating_add(1);
        fetch.blocks.push(block);
        if fetch.blocks.len() == fetch.request.count as usize {
            let fetch = self
                .pending_block_fetch
                .take()
                .expect("pending fetch exists");
            let batch =
                BlockDownloadBatch::new(Some(self.peer_id), fetch.request.start, fetch.blocks);
            let _ = fetch.reply.send(Ok(batch));
        }
        None
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
                    self.registry
                        .record_block_height(self.peer_id, *start_height);
                    // Update the global peer-reported live tip so the daemon's
                    // indexer-gate can detect catch-up vs near-tip operation.
                    neo_runtime::sync_metrics::set_peer_live_tip(*start_height as u64);
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
        if self.block_sync_mode.uses_legacy_per_peer_requests() {
            self.request_blocks_if_behind(framed).await?;
        }
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
