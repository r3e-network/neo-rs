//! `RemoteNode` peer-session actor body (dest-only migration from `neo-core`).
//!
//! This is the faithful re-expression of `neo-core::network::p2p::RemoteNode`
//! (and its `routing` / `handshake` / `lifecycle` / `inventory` /
//! `peer_messages` / `timers` submodules) on top of the task-105
//! [`RemoteSessionPorts`](crate::network::p2p::ports::RemoteSessionPorts) bundle. Every
//! collaborator the original reached through `NeoSystemContext` / `ActorRef` is
//! now either:
//!
//! * a port trait object inside [`RemoteSessionPorts`] (`ledger_query`,
//!   `mempool`, `ingress`, `chain_sink`, `task_manager`, `dispatch`), or
//! * one of the two local adapter seams declared in [`super`]:
//!   [`OutboundSink`] (the session's own wire write queue) and
//!   [`SessionLifecycleSink`] (the `LocalNode`/peer-registry coordination).
//!
//! The peer is addressed by an opaque [`PeerHandle`], never an `ActorRef`.
//!
//! # Task-89 alignment points
//!
//! * The [`Actor`] / [`ActorContext`] / [`ScheduleHandle`] runtime used here is
//!   the local placeholder from [`super`]; task 89 swaps in
//!   `neo-system::runtime` without touching the body.
//! * `on_get_headers` cannot enumerate stored headers because
//!   [`LedgerQueryPort`](crate::network::p2p::ports::LedgerQueryPort) exposes no
//!   "headers-from-index" projection. The handler therefore records the request
//!   and defers the reply to the adapter/chain seam (documented inline).
//! * Bloom-filtered (`MerkleBlock`) serving is deferred: `neo-p2p` has no
//!   merkle-block payload, so a loaded filter is tracked but blocks are served
//!   in full.

use super::{
    Actor, ActorContext, ActorResult, BloomFilterState, DropCounter, OutboundSink, PeerSnapshot,
    PendingKnownHashes, ScheduleHandle, SessionInbound, SessionLifecycleSink, SessionOutbound,
    normalize_request,
};
use crate::network::p2p::payloads::{
    AddrPayload, GetBlockByIndexPayload, GetBlocksPayload, InvPayload, PingPayload, VersionPayload,
};
use crate::network::p2p::ports::{PeerHandle, RemoteSessionPorts};
use crate::network::{ChannelsConfig, ContainsTransactionType, InventoryType};
use async_trait::async_trait;
use neo_io_crate::{HashSetCache, Serializable};
use neo_primitives::{MAX_TRANSACTION_SIZE, UInt160, UInt256};
use neo_protocol::payloads::{Block, Extensible, Header, Transaction};
use std::any::Any;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Protocol keepalive/housekeeping timer interval (mirrors `RemoteNode`).
const TIMER_INTERVAL: Duration = Duration::from_secs(30);
/// Interval between active `Ping` probes once the handshake is complete.
const PING_INTERVAL: Duration = Duration::from_secs(60);
/// How far ahead of the persisted frontier an unsolicited block is accepted.
const BLOCK_INDEX_LOOKAHEAD: u32 = 500;
/// Maximum hashes carried in one inventory payload.
const MAX_HASHES_COUNT: usize = crate::network::p2p::payloads::inv_payload::MAX_HASHES_COUNT;
/// Maximum peer addresses served in one `Addr` reply.
const MAX_COUNT_TO_SEND: usize = crate::network::p2p::payloads::addr_payload::MAX_COUNT_TO_SEND;

/// Commands accepted by the [`RemoteNode`] session actor.
///
/// Replaces `neo-core`'s `RemoteNodeCommand`. Timer-driven variants carry no
/// payload because the placeholder scheduler delivers a boxed command back to
/// this actor's own [`PeerHandle`].
#[derive(Debug, Clone)]
pub enum RemoteNodeCommand {
    /// Kick off the protocol handshake (send our `Version`).
    StartProtocol,
    /// A decoded inbound protocol frame from the wire adapter.
    Inbound(SessionInbound),
    /// The wire adapter produced a protocol/framing error.
    ConnectionError(String),
    /// Request an orderly disconnect with a human-readable reason.
    Disconnect { reason: String },
    /// The handshake did not complete within the configured timeout.
    HandshakeTimeout,
    /// Periodic housekeeping tick (ping, timeout pruning, known-hash expiry).
    TimerTick,
    /// Relay locally-produced inventory to this peer.
    RelayInventory(InvPayload),
    /// Enqueue a fully-formed outbound protocol message.
    SendInventory(SessionOutbound),
}

/// Decision produced by the handshake gate before default routing runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeGateDecision {
    /// Accept and process a `Version` frame.
    AcceptVersion,
    /// Accept and process a `Verack` frame.
    AcceptVerack,
    /// Handshake already complete: accept normal protocol traffic.
    AcceptProtocol,
    /// Reject the frame; carries the protocol-violation reason.
    Reject(&'static str),
}

/// Pure handshake gate mirroring `neo-core`'s pre-routing version/verack checks.
///
/// Enforces the ordering invariants: a `Version` is only valid before one has
/// been received, a `Verack` only after a `Version`, and every other command
/// only after the handshake completed.
pub fn handshake_gate_decision(
    version_received: bool,
    handshake_complete: bool,
    command: crate::network::p2p::MessageCommand,
) -> HandshakeGateDecision {
    use crate::network::p2p::MessageCommand;
    match command {
        MessageCommand::Version => {
            if version_received {
                HandshakeGateDecision::Reject("duplicate version")
            } else {
                HandshakeGateDecision::AcceptVersion
            }
        }
        MessageCommand::Verack => {
            if !version_received {
                HandshakeGateDecision::Reject("verack before version")
            } else if handshake_complete {
                HandshakeGateDecision::Reject("duplicate verack")
            } else {
                HandshakeGateDecision::AcceptVerack
            }
        }
        _ => {
            if handshake_complete {
                HandshakeGateDecision::AcceptProtocol
            } else {
                HandshakeGateDecision::Reject("message before handshake")
            }
        }
    }
}

/// The per-peer session actor.
///
/// Holds the immutable port bundle plus the mutable protocol state the original
/// `RemoteNode` carried (handshake progress, dedup caches, bloom filter, timers).
pub struct RemoteNode {
    handle: PeerHandle,
    ports: RemoteSessionPorts,
    outbound: Arc<dyn OutboundSink>,
    lifecycle: Arc<dyn SessionLifecycleSink>,
    config: ChannelsConfig,

    local_version: VersionPayload,
    remote_version: Option<VersionPayload>,
    handshake_complete: bool,
    version_sent: bool,
    verack_received: bool,

    known_hashes: HashSetCache<UInt256>,
    sent_hashes: HashSetCache<UInt256>,
    pending_known_hashes: PendingKnownHashes,
    bloom_filter: BloomFilterState,

    handshake_timer: Option<ScheduleHandle>,
    protocol_timer: Option<ScheduleHandle>,

    last_block_index: u32,
    last_sent: Instant,
    is_full_node: bool,
    is_trusted: bool,
    inbound: bool,
    endpoint: SocketAddr,
    fast_sync: bool,
    drop_counter: DropCounter,
}

impl RemoteNode {
    /// Constructs a session actor for `handle`.
    ///
    /// * `ports` — the task-105 bundle (never a system context).
    /// * `outbound` / `lifecycle` — the adapter-provided wire and registry seams.
    /// * `local_version` — our advertised `Version` payload.
    /// * `inbound` — whether this session was accepted (vs. dialed out).
    /// * `is_trusted` — whether the peer bypasses connection-limit accounting.
    /// * `fast_sync` — constructor flag replacing the original system read.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        handle: PeerHandle,
        ports: RemoteSessionPorts,
        outbound: Arc<dyn OutboundSink>,
        lifecycle: Arc<dyn SessionLifecycleSink>,
        config: ChannelsConfig,
        local_version: VersionPayload,
        endpoint: SocketAddr,
        inbound: bool,
        is_trusted: bool,
        fast_sync: bool,
    ) -> Self {
        let max_known = config.max_known_hashes.max(1);
        Self {
            handle,
            ports,
            outbound,
            lifecycle,
            config,
            local_version,
            remote_version: None,
            handshake_complete: false,
            version_sent: false,
            verack_received: false,
            known_hashes: HashSetCache::new(max_known),
            sent_hashes: HashSetCache::new(max_known),
            pending_known_hashes: PendingKnownHashes::new(max_known),
            bloom_filter: BloomFilterState::default(),
            handshake_timer: None,
            protocol_timer: None,
            last_block_index: 0,
            last_sent: Instant::now(),
            is_full_node: false,
            is_trusted,
            inbound,
            endpoint,
            fast_sync,
            drop_counter: DropCounter::new(),
        }
    }

    /// The opaque handle identifying this session.
    pub fn handle(&self) -> PeerHandle {
        self.handle
    }

    /// Whether the version/verack handshake has completed.
    pub fn is_handshake_complete(&self) -> bool {
        self.handshake_complete
    }

    /// Whether the peer advertised the full-node capability.
    pub fn is_full_node(&self) -> bool {
        self.is_full_node
    }

    /// Whether this session runs in fast-sync mode.
    pub fn fast_sync(&self) -> bool {
        self.fast_sync
    }

    /// Enqueues one outbound message, recording the send time on success.
    fn send(&mut self, message: SessionOutbound) {
        match self.outbound.enqueue(message) {
            Ok(()) => self.last_sent = Instant::now(),
            Err(err) => tracing::debug!(peer = %self.handle, "outbound enqueue failed: {err}"),
        }
    }

    /// Requests an orderly disconnect and marks the session stopped.
    fn disconnect(&mut self, ctx: &mut ActorContext, reason: &str) {
        tracing::debug!(peer = %self.handle, "disconnecting: {reason}");
        let _ = self.lifecycle.connection_terminated(self.handle);
        let _ = ctx.stop_self();
    }

    /// Sends our `Version` and arms the handshake timeout.
    fn start_protocol(&mut self, ctx: &mut ActorContext) {
        if !self.version_sent {
            self.send(SessionOutbound::Version(self.local_version.clone()));
            self.version_sent = true;
        }
        if self.handshake_timer.is_none() {
            self.handshake_timer = Some(ctx.schedule_once(
                self.config.handshake_timeout,
                Box::new(RemoteNodeCommand::HandshakeTimeout),
            ));
        }
    }

    // -----------------------------------------------------------------------
    // Inbound routing.
    // -----------------------------------------------------------------------

    /// Routes one inbound frame through the dispatch veto, the handshake gate,
    /// and finally the per-command handler.
    fn route_inbound(&mut self, message: SessionInbound, ctx: &mut ActorContext) {
        let command = message.command();

        // External-handler veto (fail-open-to-default: `true` continues).
        if !self
            .ports
            .dispatch
            .dispatch_inbound(self.handle, command, message.raw_payload())
        {
            return;
        }

        let version_received = self.remote_version.is_some();
        match handshake_gate_decision(version_received, self.handshake_complete, command) {
            HandshakeGateDecision::AcceptVersion => {
                if let SessionInbound::Version(version) = message {
                    self.on_version(version, ctx);
                }
            }
            HandshakeGateDecision::AcceptVerack => self.on_verack(ctx),
            HandshakeGateDecision::AcceptProtocol => self.route_protocol(message, ctx),
            HandshakeGateDecision::Reject(reason) => {
                self.disconnect(ctx, reason);
            }
        }
    }

    /// Dispatches post-handshake protocol frames to their handlers.
    fn route_protocol(&mut self, message: SessionInbound, ctx: &mut ActorContext) {
        match message {
            SessionInbound::Ping(payload) => self.on_ping(payload),
            SessionInbound::Pong(payload) => self.on_pong(payload),
            SessionInbound::Inv(payload) => self.on_inv(payload),
            SessionInbound::GetData(payload) => self.on_get_data(payload),
            SessionInbound::GetBlocks(payload) => self.on_get_blocks(payload),
            SessionInbound::GetBlockByIndex(payload) => self.on_get_block_by_index(payload),
            SessionInbound::GetHeaders(payload) => self.on_get_headers(payload),
            SessionInbound::Headers(headers) => self.on_headers(headers),
            SessionInbound::Transaction(tx) => self.on_transaction(tx),
            SessionInbound::Block(block) => self.on_block(block),
            SessionInbound::Extensible(payload) => self.on_extensible(payload),
            SessionInbound::Mempool => self.on_mempool(),
            SessionInbound::GetAddr => self.on_get_addr(),
            SessionInbound::Addr(payload) => self.on_addr(payload),
            SessionInbound::NotFound(payload) => self.on_not_found(payload),
            SessionInbound::FilterLoad(payload) => self.bloom_filter.load(payload),
            SessionInbound::FilterAdd(payload) => self.bloom_filter.add(payload),
            SessionInbound::FilterClear => self.bloom_filter.clear(),
            SessionInbound::Reject(body) => {
                tracing::debug!(peer = %self.handle, len = body.len(), "peer sent reject");
            }
            SessionInbound::Alert(body) => {
                tracing::debug!(peer = %self.handle, len = body.len(), "peer sent alert");
            }
            // Version/Verack are gated before reaching this arm.
            SessionInbound::Version(_) | SessionInbound::Verack => {}
        }
        let _ = ctx;
    }

    // -----------------------------------------------------------------------
    // Handshake handlers.
    // -----------------------------------------------------------------------

    fn on_version(&mut self, version: VersionPayload, ctx: &mut ActorContext) {
        if version.network != self.local_version.network {
            self.disconnect(ctx, "network magic mismatch");
            return;
        }

        for capability in &version.capabilities {
            if let crate::network::p2p::payloads::NodeCapability::FullNode { start_height } = capability {
                self.is_full_node = true;
                self.last_block_index = *start_height;
            }
        }

        let listen_port = version
            .capabilities
            .iter()
            .find_map(|cap| match cap {
                crate::network::p2p::payloads::NodeCapability::TcpServer { port } => Some(*port),
                _ => None,
            })
            .unwrap_or(self.endpoint.port());

        let snapshot = PeerSnapshot {
            remote_address: self.endpoint,
            remote_port: self.endpoint.port(),
            listen_tcp_port: listen_port,
            last_block_index: self.last_block_index,
            version: version.version,
            services: 0,
            timestamp: version.timestamp as u64,
        };

        let accepted = self.lifecycle.connection_established(
            self.handle,
            snapshot,
            &version,
            self.is_trusted,
            self.inbound,
        );
        if !accepted {
            self.disconnect(ctx, "connection rejected by registry");
            return;
        }

        // Reply with our version if we have not sent it yet (inbound sessions).
        if !self.version_sent {
            self.send(SessionOutbound::Version(self.local_version.clone()));
            self.version_sent = true;
        }
        // Always acknowledge the peer's version.
        self.send(SessionOutbound::Verack);

        self.remote_version = Some(version.clone());
        let _ = self
            .ports
            .task_manager
            .register_peer(self.handle, version.clone());
        self.maybe_complete_handshake(ctx);
    }

    fn on_verack(&mut self, ctx: &mut ActorContext) {
        self.verack_received = true;
        self.maybe_complete_handshake(ctx);
    }

    /// Completes the handshake once both a version and a verack are present.
    fn maybe_complete_handshake(&mut self, ctx: &mut ActorContext) {
        if self.handshake_complete {
            return;
        }
        if self.remote_version.is_some() && self.verack_received {
            self.handshake_complete = true;
            if let Some(timer) = self.handshake_timer.take() {
                timer.cancel();
            }
            if self.protocol_timer.is_none() {
                self.protocol_timer = Some(ctx.schedule_repeated(
                    TIMER_INTERVAL,
                    TIMER_INTERVAL,
                    Box::new(RemoteNodeCommand::TimerTick),
                ));
            }
            if let Some(version) = &self.remote_version {
                let _ = self
                    .ports
                    .task_manager
                    .update_peer(self.handle, self.last_block_index.max(0));
                let _ = version;
            }
        }
    }

    // -----------------------------------------------------------------------
    // Keepalive handlers.
    // -----------------------------------------------------------------------

    fn on_ping(&mut self, payload: PingPayload) {
        self.last_block_index = self.last_block_index.max(payload.last_block_index);
        let _ = self
            .ports
            .task_manager
            .update_peer(self.handle, payload.last_block_index);
        self.lifecycle
            .update_peer_height(self.handle, payload.last_block_index);
        self.send(SessionOutbound::Pong(PingPayload::create_with_nonce(
            self.ports.ledger_query.current_index(),
            payload.nonce,
        )));
    }

    fn on_pong(&mut self, payload: PingPayload) {
        self.last_block_index = self.last_block_index.max(payload.last_block_index);
        let _ = self
            .ports
            .task_manager
            .update_peer(self.handle, payload.last_block_index);
        self.lifecycle
            .update_peer_height(self.handle, payload.last_block_index);
    }

    // -----------------------------------------------------------------------
    // Inventory announcement / request handlers.
    // -----------------------------------------------------------------------

    fn on_inv(&mut self, payload: InvPayload) {
        let now = Instant::now();
        let mut unknown = Vec::new();
        for hash in payload.hashes {
            if self.known_hashes.contains(&hash) || self.sent_hashes.contains(&hash) {
                continue;
            }
            let already_have = match payload.inventory_type {
                InventoryType::Block => self.ports.ledger_query.contains_block(&hash),
                InventoryType::Transaction => {
                    self.ports.mempool.contains_transaction(&hash)
                        != ContainsTransactionType::NotExist
                        || self.ports.ledger_query.contains_transaction(&hash)
                }
                InventoryType::Extensible | InventoryType::Consensus => {
                    self.ports.mempool.try_get_extensible(&hash).is_some()
                }
            };
            if already_have {
                self.known_hashes.try_add(hash);
                continue;
            }
            if self.pending_known_hashes.try_add(hash, now) {
                unknown.push(hash);
            }
        }
        if unknown.is_empty() {
            return;
        }
        let forwarded = InvPayload::new(payload.inventory_type, unknown);
        let _ = self.ports.task_manager.new_tasks(self.handle, forwarded);
    }

    fn on_get_data(&mut self, payload: InvPayload) {
        let mut served_hashes = Vec::new();
        for hash in payload.hashes.clone() {
            match payload.inventory_type {
                InventoryType::Transaction => {
                    if let Some(tx) = self.ports.mempool.try_get_transaction(&hash) {
                        self.sent_hashes.try_add(hash);
                        served_hashes.push(hash);
                        self.send(SessionOutbound::Transaction(tx));
                    }
                }
                InventoryType::Block => {
                    if let Some(mut block) = self.ports.ledger_query.try_get_block(&hash) {
                        self.sent_hashes.try_add(hash);
                        served_hashes.push(hash);
                        // Ensure the hash cache is warm before sending.
                        let _ = block.try_hash();
                        self.send(SessionOutbound::Block(block));
                    }
                }
                InventoryType::Extensible | InventoryType::Consensus => {
                    let extensible = self
                        .ports
                        .mempool
                        .try_get_relay_extensible(&hash)
                        .or_else(|| self.ports.mempool.try_get_extensible(&hash));
                    if let Some(payload) = extensible {
                        self.sent_hashes.try_add(hash);
                        served_hashes.push(hash);
                        self.send(SessionOutbound::Extensible(payload));
                    }
                }
            }
        }
        // Report any hashes we could not serve so the peer can retry elsewhere.
        let missing: Vec<UInt256> = payload
            .hashes
            .into_iter()
            .filter(|hash| !served_hashes.contains(hash))
            .collect();
        if !missing.is_empty() {
            self.send(SessionOutbound::NotFound(InvPayload::new(
                payload.inventory_type,
                missing,
            )));
        }
    }

    fn on_get_blocks(&mut self, payload: GetBlocksPayload) {
        let count = normalize_request(payload.count, MAX_HASHES_COUNT);
        if count == 0 {
            return;
        }
        let hashes = self
            .ports
            .ledger_query
            .block_hashes_from(&payload.hash_start, count);
        if hashes.is_empty() {
            return;
        }
        for chunk in hashes.chunks(MAX_HASHES_COUNT) {
            self.send(SessionOutbound::Inv(InvPayload::new(
                InventoryType::Block,
                chunk.to_vec(),
            )));
        }
    }

    fn on_get_block_by_index(&mut self, payload: GetBlockByIndexPayload) {
        let count = normalize_request(payload.count, MAX_HASHES_COUNT);
        let current = self.ports.ledger_query.current_index();
        let mut index = payload.index_start;
        let mut sent = 0usize;
        while sent < count && index <= current {
            let Some(hash) = self.ports.ledger_query.block_hash_at(index) else {
                break;
            };
            let Some(mut block) = self.ports.ledger_query.try_get_block(&hash) else {
                break;
            };
            let _ = block.try_hash();
            self.send(SessionOutbound::Block(block));
            index = index.saturating_add(1);
            sent += 1;
        }
    }

    /// Records a header-range request.
    ///
    /// Task-89 alignment point: [`LedgerQueryPort`] has no "headers from index"
    /// projection, so the reply cannot be assembled here. The request is
    /// acknowledged in tracing and the actual `Headers` reply is the chain
    /// adapter's responsibility once a header projection port lands.
    fn on_get_headers(&mut self, payload: GetBlockByIndexPayload) {
        tracing::debug!(
            peer = %self.handle,
            start = payload.index_start,
            count = payload.count,
            "get_headers received; header projection deferred to adapter (task 89)"
        );
    }

    fn on_headers(&mut self, mut headers: Vec<Header>) {
        if headers.is_empty() {
            return;
        }
        // Track the highest header index the peer has delivered.
        let tip = headers.iter().map(Header::index).max().unwrap_or(0);
        self.last_block_index = self.last_block_index.max(tip);
        let _ = self
            .ports
            .chain_sink
            .submit_headers(headers.clone(), self.handle);
        let _ = self.ports.task_manager.headers(self.handle);
        let _ = self.ports.task_manager.update_peer(self.handle, tip);
        // Warm header hashes for downstream dedup.
        for header in headers.iter_mut() {
            let _ = header.try_hash();
        }
    }

    // -----------------------------------------------------------------------
    // Inventory delivery handlers.
    // -----------------------------------------------------------------------

    fn on_transaction(&mut self, transaction: Transaction) {
        let hash = transaction.hash();
        self.pending_known_hashes.remove(&hash);
        if self.known_hashes.contains(&hash) {
            return;
        }

        match self.ports.mempool.contains_transaction(&hash) {
            ContainsTransactionType::ExistsInLedger | ContainsTransactionType::ExistsInPool => {
                self.known_hashes.try_add(hash);
                let _ = self
                    .ports
                    .task_manager
                    .inventory_completed(self.handle, hash, None, None);
                return;
            }
            ContainsTransactionType::NotExist => {}
        }

        if transaction.size() > MAX_TRANSACTION_SIZE {
            self.known_hashes.try_add(hash);
            let _ = self
                .ports
                .task_manager
                .inventory_completed(self.handle, hash, None, None);
            return;
        }

        let signers: Vec<UInt160> = transaction.signers().iter().map(|s| s.account).collect();
        if self.ports.mempool.contains_conflict_hash(&hash, &signers) {
            self.known_hashes.try_add(hash);
            let _ = self
                .ports
                .task_manager
                .inventory_completed(self.handle, hash, None, None);
            return;
        }

        self.known_hashes.try_add(hash);
        if self
            .ports
            .ingress
            .enqueue_preverify(transaction, true, self.handle)
            .is_err()
        {
            // Fail-closed: drop the dedup record so a later announcement retries.
            self.known_hashes.remove(&hash);
            let _ = self.ports.task_manager.forget_hash(hash);
        }
        let _ = self
            .ports
            .task_manager
            .inventory_completed(self.handle, hash, None, None);
    }

    fn on_block(&mut self, mut block: Block) {
        let hash = match block.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                tracing::debug!(peer = %self.handle, "invalid block hash: {err}");
                let _ = self.drop_counter.next();
                return;
            }
        };
        let index = block.index();
        self.pending_known_hashes.remove(&hash);

        let current = self.ports.ledger_query.current_index();
        if index <= current {
            // Already persisted: release the task without re-forwarding.
            self.known_hashes.try_add(hash);
            let _ = self.ports.task_manager.inventory_completed(
                self.handle,
                hash,
                Some(block),
                Some(index),
            );
            return;
        }
        if index > current.saturating_add(BLOCK_INDEX_LOOKAHEAD) {
            // Too far ahead of the frontier; release the task, keep the peer.
            let _ = self.drop_counter.next();
            let _ =
                self.ports
                    .task_manager
                    .inventory_completed(self.handle, hash, None, Some(index));
            return;
        }

        self.known_hashes.try_add(hash);
        let forwarded = block.clone();
        if self
            .ports
            .chain_sink
            .submit_inventory_block(forwarded, true, false, self.handle)
            .is_err()
        {
            self.known_hashes.remove(&hash);
            let _ = self.ports.task_manager.forget_hash(hash);
        }
        let _ = self.ports.task_manager.inventory_completed(
            self.handle,
            hash,
            Some(block),
            Some(index),
        );
    }

    fn on_extensible(&mut self, mut payload: Extensible) {
        let hash = match payload.try_hash() {
            Ok(hash) => hash,
            Err(err) => {
                tracing::debug!(peer = %self.handle, "invalid extensible hash: {err}");
                return;
            }
        };
        self.pending_known_hashes.remove(&hash);
        if self.known_hashes.contains(&hash) {
            return;
        }
        self.known_hashes.try_add(hash);
        if self
            .ports
            .chain_sink
            .submit_inventory_extensible(payload, true, self.handle)
            .is_err()
        {
            self.known_hashes.remove(&hash);
            let _ = self.ports.task_manager.forget_hash(hash);
        }
    }

    fn on_mempool(&mut self) {
        let hashes = self.ports.mempool.transaction_hashes();
        if hashes.is_empty() {
            return;
        }
        for chunk in hashes.chunks(MAX_HASHES_COUNT) {
            self.send(SessionOutbound::Inv(InvPayload::new(
                InventoryType::Transaction,
                chunk.to_vec(),
            )));
        }
    }

    fn on_not_found(&mut self, payload: InvPayload) {
        // The peer could not supply the announced inventory; let the task
        // manager re-schedule those hashes against other peers.
        let _ = self
            .ports
            .task_manager
            .restart_tasks(self.handle, payload.clone());
        for hash in &payload.hashes {
            self.pending_known_hashes.remove(hash);
        }
    }

    // -----------------------------------------------------------------------
    // Peer-discovery handlers.
    // -----------------------------------------------------------------------

    fn on_get_addr(&mut self) {
        let mut book = self.lifecycle.address_book();
        if book.is_empty() {
            return;
        }
        book.truncate(MAX_COUNT_TO_SEND);
        self.send(SessionOutbound::Addr(AddrPayload::create(book)));
    }

    fn on_addr(&mut self, payload: AddrPayload) {
        let endpoints: Vec<SocketAddr> = payload
            .address_list
            .iter()
            .filter_map(crate::network::p2p::payloads::NetworkAddressWithTime::endpoint)
            .collect();
        if endpoints.is_empty() {
            return;
        }
        self.lifecycle.add_peers(endpoints);
    }

    // -----------------------------------------------------------------------
    // Timers.
    // -----------------------------------------------------------------------

    fn on_timer_tick(&mut self, ctx: &mut ActorContext) {
        // Prune the pending-announcement TTL window.
        let cutoff = Instant::now();
        self.pending_known_hashes.prune_older_than(
            cutoff
                .checked_sub(super::PENDING_KNOWN_HASH_TTL)
                .unwrap_or(cutoff),
        );

        // Probe a silent, fully-handshaked peer.
        if self.handshake_complete && self.last_sent.elapsed() >= PING_INTERVAL {
            let height = self.ports.ledger_query.current_index();
            self.send(SessionOutbound::Ping(PingPayload::create(height)));
        }

        // Enforce the active-session read timeout.
        if self.handshake_complete
            && !self.is_trusted
            && self.last_sent.elapsed() >= self.config.read_timeout_active
        {
            self.disconnect(ctx, "read timeout");
        }
        let _ = self.fast_sync;
    }
}

#[async_trait]
impl Actor for RemoteNode {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        // Outbound sessions advertise our version immediately; inbound sessions
        // wait for the peer's version before replying (handled in `on_version`).
        if !self.inbound {
            self.start_protocol(ctx);
        } else if self.handshake_timer.is_none() {
            self.handshake_timer = Some(ctx.schedule_once(
                self.config.handshake_timeout,
                Box::new(RemoteNodeCommand::HandshakeTimeout),
            ));
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let Ok(command) = message.downcast::<RemoteNodeCommand>() else {
            return Ok(());
        };
        match *command {
            RemoteNodeCommand::StartProtocol => self.start_protocol(ctx),
            RemoteNodeCommand::Inbound(inbound) => self.route_inbound(inbound, ctx),
            RemoteNodeCommand::ConnectionError(reason) => self.disconnect(ctx, &reason),
            RemoteNodeCommand::Disconnect { reason } => self.disconnect(ctx, &reason),
            RemoteNodeCommand::HandshakeTimeout => {
                if !self.handshake_complete {
                    self.disconnect(ctx, "handshake timeout");
                }
            }
            RemoteNodeCommand::TimerTick => self.on_timer_tick(ctx),
            RemoteNodeCommand::RelayInventory(payload) => {
                if self.handshake_complete {
                    for hash in &payload.hashes {
                        self.sent_hashes.try_add(*hash);
                    }
                    self.send(SessionOutbound::Inv(payload));
                }
            }
            RemoteNodeCommand::SendInventory(outbound) => {
                if self.handshake_complete {
                    self.send(outbound);
                }
            }
        }
        Ok(())
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        if let Some(timer) = self.handshake_timer.take() {
            timer.cancel();
        }
        if let Some(timer) = self.protocol_timer.take() {
            timer.cancel();
        }
        self.lifecycle.connection_terminated(self.handle);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::MessageCommand;

    #[test]
    fn handshake_gate_enforces_ordering() {
        // Version is only acceptable before one has been received.
        assert_eq!(
            handshake_gate_decision(false, false, MessageCommand::Version),
            HandshakeGateDecision::AcceptVersion
        );
        assert_eq!(
            handshake_gate_decision(true, false, MessageCommand::Version),
            HandshakeGateDecision::Reject("duplicate version")
        );
        // Verack requires a prior version and a not-yet-complete handshake.
        assert_eq!(
            handshake_gate_decision(false, false, MessageCommand::Verack),
            HandshakeGateDecision::Reject("verack before version")
        );
        assert_eq!(
            handshake_gate_decision(true, false, MessageCommand::Verack),
            HandshakeGateDecision::AcceptVerack
        );
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Verack),
            HandshakeGateDecision::Reject("duplicate verack")
        );
        // Any other command needs a completed handshake.
        assert_eq!(
            handshake_gate_decision(true, false, MessageCommand::Ping),
            HandshakeGateDecision::Reject("message before handshake")
        );
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Ping),
            HandshakeGateDecision::AcceptProtocol
        );
    }
}
