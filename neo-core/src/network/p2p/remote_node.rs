//! Actor-based remote node implementation mirroring the Akka.NET design.
//! Submodules: handshake.rs, timers.rs, inventory.rs, routing.rs, message_handlers.rs, pending_known_hashes.rs.

mod handshake;
mod inventory;
mod message_handlers;
mod pending_known_hashes;
mod routing;
mod timers;

use super::{
    channels_config::ChannelsConfig,
    connection::{ConnectionState, PeerConnection},
    local_node::{LocalNode, RemoteNodeSnapshot},
    payloads::{
        addr_payload::AddrPayload,
        filter_add_payload::FilterAddPayload,
        filter_load_payload::FilterLoadPayload,
        get_block_by_index_payload::GetBlockByIndexPayload,
        headers_payload::{HeadersPayload, MAX_HEADERS_COUNT},
        ping_payload::PingPayload,
        VersionPayload,
    },
    peer::PeerCommand,
    task_manager::TaskManagerCommand,
};
use crate::akka::{Actor, ActorContext, ActorResult, Cancelable, Props};
use crate::cryptography::BloomFilter;
use crate::ledger::blockchain::BlockchainCommand;
use crate::network::error::NetworkError;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::MessageCommand;
use crate::{neo_system::NeoSystemContext, protocol_settings::ProtocolSettings, UInt256};
use async_trait::async_trait;
use neo_io_crate::HashSetCache;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, trace, warn};

pub use message_handlers::{
    register_message_received_handler, unregister_message_received_handler,
    MessageHandlerSubscription,
};
use pending_known_hashes::PendingKnownHashes;

/// Remote node actor responsible for protocol negotiation and message relay.
pub struct RemoteNode {
    system: Arc<NeoSystemContext>,
    connection: Arc<Mutex<PeerConnection>>,
    endpoint: SocketAddr,
    _local_endpoint: SocketAddr,
    local_version: VersionPayload,
    settings: Arc<ProtocolSettings>,
    config: ChannelsConfig,
    is_trusted: bool,
    inbound: bool,
    local_node: Arc<LocalNode>,
    remote_version: Option<VersionPayload>,
    handshake_complete: bool,
    handshake_done: Arc<AtomicBool>,
    reader_spawned: bool,
    known_hashes: HashSetCache<UInt256>,
    sent_hashes: HashSetCache<UInt256>,
    pending_known_hashes: PendingKnownHashes,
    bloom_filter: Option<BloomFilter>,
    timer: Option<Cancelable>,
    last_block_index: u32,
    _last_height_sent: u32,
    message_queue_high: VecDeque<NetworkMessage>,
    message_queue_low: VecDeque<NetworkMessage>,
    sent_commands: [bool; 256],
    verack_received: bool,
    ack_ready: bool,
    last_sent: Instant,
    /// Indicates if the remote peer advertised FullNode capability.
    is_full_node: bool,
    /// Rate limiting for bloom filter operations to prevent DoS attacks.
    filter_ops_count: u32,
    filter_ops_reset_time: Instant,
    /// SECURITY: Track memory usage per peer to prevent memory exhaustion attacks.
    /// This includes message queues, bloom filters, and pending data.
    memory_usage_bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandshakeGateDecision {
    AcceptVersion,
    AcceptVerack,
    AcceptProtocol,
    Reject(&'static str),
}

impl RemoteNode {
    /// Maximum payload size accepted for alert messages before dropping them.
    const MAX_ALERT_PAYLOAD_BYTES: usize = 4 * 1024;
    /// Maximum number of bytes logged from an alert payload to avoid log spam.
    const MAX_ALERT_LOG_BYTES: usize = 256;

    fn normalize_request(count: i16, max: usize) -> usize {
        if count < 0 {
            max
        } else {
            (count as usize).min(max)
        }
    }

    fn should_skip_inventory(&self, hash: &UInt256) -> bool {
        self.pending_known_hashes.contains(hash)
            || self.known_hashes.contains(hash)
            || self.sent_hashes.contains(hash)
    }

    fn dispatch_message_received(&self, message: &NetworkMessage) -> bool {
        let Some(system) = self.system.neo_system() else {
            return true;
        };

        let Some(wire_message) = RemoteNode::build_wire_message(message) else {
            return true;
        };

        message_handlers::with_handlers(|handlers| {
            if handlers.is_empty() {
                return true;
            }
            for entry in handlers {
                if !entry
                    .handler
                    .remote_node_message_received_handler(&system, &wire_message)
                {
                    return false;
                }
            }
            true
        })
    }

    /// Maximum bloom filter size in bytes (matches C# MAX_FILTER_SIZE = 36000).
    /// This prevents memory exhaustion attacks via oversized filter payloads.
    const MAX_BLOOM_FILTER_SIZE: usize = 36000;

    /// Maximum number of hash functions for bloom filter (reasonable upper bound).
    const MAX_BLOOM_K: u8 = 50;

    /// Maximum bloom filter operations per minute to prevent DoS attacks.
    const MAX_FILTER_OPS_PER_MINUTE: u32 = 100;

    /// Duration for filter rate limit window (60 seconds).
    const FILTER_RATE_LIMIT_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

    /// SECURITY: Maximum memory usage per peer in bytes (8 MB).
    /// This prevents a single malicious peer from exhausting node memory.
    /// The limit accounts for: message queues, bloom filters, pending hashes, and buffers.
    const MAX_MEMORY_PER_PEER: usize = 8 * 1024 * 1024;

    /// Checks and updates the filter operation rate limit.
    /// Returns true if the operation is allowed, false if rate limited.
    fn check_filter_rate_limit(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.filter_ops_reset_time) >= Self::FILTER_RATE_LIMIT_WINDOW {
            // Reset the counter for a new window
            self.filter_ops_count = 0;
            self.filter_ops_reset_time = now;
        }

        if self.filter_ops_count >= Self::MAX_FILTER_OPS_PER_MINUTE {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                ops_count = self.filter_ops_count,
                "bloom filter rate limit exceeded, rejecting operation"
            );
            return false;
        }

        self.filter_ops_count += 1;
        true
    }

    /// SECURITY: Checks if adding the specified bytes would exceed the per-peer memory quota.
    /// Returns true if the allocation is allowed, false if it would exceed the quota.
    fn check_memory_quota(&self, additional_bytes: usize) -> bool {
        self.memory_usage_bytes.saturating_add(additional_bytes) <= Self::MAX_MEMORY_PER_PEER
    }

    /// SECURITY: Adds to the tracked memory usage for this peer.
    /// Should be called when allocating buffers, adding to queues, etc.
    fn add_memory_usage(&mut self, bytes: usize) {
        self.memory_usage_bytes = self.memory_usage_bytes.saturating_add(bytes);
    }

    /// SECURITY: Subtracts from the tracked memory usage for this peer.
    /// Should be called when freeing buffers, removing from queues, etc.
    fn release_memory_usage(&mut self, bytes: usize) {
        self.memory_usage_bytes = self.memory_usage_bytes.saturating_sub(bytes);
    }

    /// SECURITY: Estimates the memory size of a network message for quota tracking.
    fn estimate_message_size(message: &NetworkMessage) -> usize {
        // Base overhead for the message structure
        const BASE_OVERHEAD: usize = 64;

        // Estimate payload size based on message type
        // Using conservative estimates to avoid needing Serializable trait
        let payload_size = match &message.payload {
            ProtocolMessage::Block(_) => 2048, // Conservative block estimate
            ProtocolMessage::Headers(headers) => headers.headers.len() * 512,
            ProtocolMessage::Transaction(_) => 1024, // Conservative tx estimate
            ProtocolMessage::Inv(inv) => inv.hashes.len() * 32,
            ProtocolMessage::GetData(inv) => inv.hashes.len() * 32,
            ProtocolMessage::GetBlocks(_) => 64, // hash_start (32) + hash_stop (32)
            ProtocolMessage::Extensible(ext) => ext.data.len() + 128,
            _ => 128, // Default estimate for other message types
        };

        BASE_OVERHEAD + payload_size
    }

    fn on_filter_load(&mut self, payload: &FilterLoadPayload) {
        // Check rate limit before processing
        if !self.check_filter_rate_limit() {
            return;
        }

        if payload.filter.is_empty() || payload.k == 0 {
            self.bloom_filter = None;
            return;
        }

        // Validate filter size to prevent memory exhaustion
        if payload.filter.len() > Self::MAX_BLOOM_FILTER_SIZE {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                filter_size = payload.filter.len(),
                max_size = Self::MAX_BLOOM_FILTER_SIZE,
                "bloom filter too large, rejecting"
            );
            self.bloom_filter = None;
            return;
        }

        // Validate k parameter (number of hash functions)
        if payload.k > Self::MAX_BLOOM_K {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                k = payload.k,
                max_k = Self::MAX_BLOOM_K,
                "bloom filter k value too large, rejecting"
            );
            self.bloom_filter = None;
            return;
        }

        let bit_size = payload.filter.len() * 8;
        match BloomFilter::with_bits(bit_size, payload.k as usize, payload.tweak, &payload.filter) {
            Ok(filter) => self.bloom_filter = Some(filter),
            Err(error) => {
                debug!(target: "neo", %error, "failed to load bloom filter from payload");
                self.bloom_filter = None;
            }
        }
    }

    fn on_filter_clear(&mut self) {
        self.bloom_filter = None;
    }

    fn on_filter_add(&mut self, payload: &FilterAddPayload) {
        // Check rate limit before processing to prevent DoS via filter updates
        if !self.check_filter_rate_limit() {
            return;
        }

        if let Some(filter) = self.bloom_filter.as_mut() {
            filter.add(&payload.data);
        }
    }

    /// Handles reject messages from peers.
    /// Reject messages indicate protocol violations or refused operations.
    fn on_reject(&mut self, data: &[u8]) {
        // Parse reject reason if available (format: command byte + reason string)
        let reason = if data.len() > 1 {
            String::from_utf8_lossy(&data[1..]).to_string()
        } else if !data.is_empty() {
            format!("Command 0x{:02x} rejected", data[0])
        } else {
            "Unknown rejection".to_string()
        };

        warn!(
            target: "neo",
            endpoint = %self.endpoint,
            reason = %reason,
            "peer sent reject message"
        );

        // Track rejections for potential peer penalties
        // Future enhancement: increment penalty counter and trigger
        // disconnection after repeated rejections
    }

    /// Handles alert messages from peers.
    /// Alert commands are deprecated on N3, so we validate and drop them.
    fn on_alert(&mut self, data: &[u8]) {
        if data.is_empty() {
            trace!(
                target: "neo",
                endpoint = %self.endpoint,
                "dropping empty alert payload"
            );
            return;
        }

        if data.len() > Self::MAX_ALERT_PAYLOAD_BYTES {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                bytes = data.len(),
                limit = Self::MAX_ALERT_PAYLOAD_BYTES,
                "dropping oversized alert payload"
            );
            return;
        }

        let summary = Self::summarize_alert_payload(data);
        warn!(
            target: "neo",
            endpoint = %self.endpoint,
            bytes = data.len(),
            message = %summary,
            "peer sent deprecated alert command; ignoring message"
        );
    }

    fn summarize_alert_payload(payload: &[u8]) -> String {
        let capture_len = payload.len().min(Self::MAX_ALERT_LOG_BYTES);
        let slice = &payload[..capture_len];
        let mut summary = match std::str::from_utf8(slice) {
            Ok(text) => text
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                .collect::<String>()
                .trim()
                .to_string(),
            Err(_) => format!("0x{}", hex::encode(slice)),
        };

        if payload.len() > capture_len {
            summary.push_str("...");
        }

        summary
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        system: Arc<NeoSystemContext>,
        local_node: Arc<LocalNode>,
        connection: Arc<Mutex<PeerConnection>>,
        endpoint: SocketAddr,
        local_endpoint: SocketAddr,
        local_version: VersionPayload,
        settings: Arc<ProtocolSettings>,
        config: ChannelsConfig,
        is_trusted: bool,
        inbound: bool,
    ) -> Self {
        let cache_capacity = config.max_known_hashes.max(1);
        Self {
            system,
            local_node,
            connection,
            endpoint,
            _local_endpoint: local_endpoint,
            local_version,
            settings,
            config,
            is_trusted,
            inbound,
            remote_version: None,
            handshake_complete: false,
            handshake_done: Arc::new(AtomicBool::new(false)),
            reader_spawned: false,
            known_hashes: HashSetCache::new(cache_capacity),
            sent_hashes: HashSetCache::new(cache_capacity),
            pending_known_hashes: PendingKnownHashes::new(cache_capacity),
            bloom_filter: None,
            timer: None,
            last_block_index: 0,
            _last_height_sent: 0,
            message_queue_high: VecDeque::new(),
            message_queue_low: VecDeque::new(),
            sent_commands: [false; 256],
            verack_received: false,
            ack_ready: true,
            last_sent: Instant::now(),
            is_full_node: false,
            filter_ops_count: 0,
            filter_ops_reset_time: Instant::now(),
            memory_usage_bytes: 0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn props(
        system: Arc<NeoSystemContext>,
        local_node: Arc<LocalNode>,
        connection: Arc<Mutex<PeerConnection>>,
        endpoint: SocketAddr,
        local_endpoint: SocketAddr,
        local_version: VersionPayload,
        settings: Arc<ProtocolSettings>,
        config: ChannelsConfig,
        is_trusted: bool,
        inbound: bool,
    ) -> Props {
        Props::new(move || {
            Self::new(
                Arc::clone(&system),
                Arc::clone(&local_node),
                Arc::clone(&connection),
                endpoint,
                local_endpoint,
                local_version.clone(),
                Arc::clone(&settings),
                config.clone(),
                is_trusted,
                inbound,
            )
        })
    }

    /// Maximum number of messages allowed in each queue to prevent memory exhaustion.
    /// This protects against DoS attacks from malicious peers flooding messages.
    const MAX_QUEUE_SIZE: usize = 1024;

    async fn enqueue_message(&mut self, message: NetworkMessage) -> ActorResult {
        let command = message.command();
        let is_high_priority = Self::is_high_priority(command);

        // SECURITY: Check memory quota before accepting the message
        let message_size = Self::estimate_message_size(&message);
        if !self.check_memory_quota(message_size) {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                command = ?command,
                message_size = message_size,
                current_usage = self.memory_usage_bytes,
                max_allowed = Self::MAX_MEMORY_PER_PEER,
                "per-peer memory quota exceeded, dropping message"
            );
            return Ok(());
        }

        // Check queue size and duplicates before borrowing mutably
        let (queue_full, has_duplicate) = if is_high_priority {
            let full = self.message_queue_high.len() >= Self::MAX_QUEUE_SIZE;
            let dup = Self::is_single_command(command)
                && self
                    .message_queue_high
                    .iter()
                    .any(|q| q.command() == command);
            (full, dup)
        } else {
            let full = self.message_queue_low.len() >= Self::MAX_QUEUE_SIZE;
            let dup = Self::is_single_command(command)
                && self
                    .message_queue_low
                    .iter()
                    .any(|q| q.command() == command);
            (full, dup)
        };

        // Prevent queue overflow - drop message if queue is full
        if queue_full {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                command = ?command,
                "message queue full, dropping message"
            );
            return Ok(());
        }

        if has_duplicate {
            return Ok(());
        }

        // SECURITY: Track memory usage when adding to queue
        self.add_memory_usage(message_size);

        // Now push to the appropriate queue
        if is_high_priority {
            self.message_queue_high.push_back(message);
        } else {
            self.message_queue_low.push_back(message);
        }

        self.flush_queue().await
    }

    async fn flush_queue(&mut self) -> ActorResult {
        if !self.verack_received || !self.ack_ready {
            return Ok(());
        }

        while self.verack_received && self.ack_ready {
            let next_message = if let Some(msg) = self.message_queue_high.pop_front() {
                Some(msg)
            } else {
                self.message_queue_low.pop_front()
            };

            let Some(message) = next_message else {
                break;
            };

            // SECURITY: Release memory quota when message is dequeued for sending
            let message_size = Self::estimate_message_size(&message);
            self.release_memory_usage(message_size);

            self.ack_ready = false;
            self.last_sent = Instant::now();
            let index = message.command().to_byte() as usize;
            if index < self.sent_commands.len() {
                self.sent_commands[index] = true;
            }
            self.send_wire_message(&message).await?;
            self.ack_ready = true;
        }

        Ok(())
    }

    fn consume_sent_command(&mut self, command: MessageCommand) -> bool {
        let index = command.to_byte() as usize;
        if let Some(flag) = self.sent_commands.get_mut(index) {
            if *flag {
                *flag = false;
                return true;
            }
        }
        false
    }

    fn current_local_block_index(&self) -> u32 {
        self.system.current_block_index()
    }

    fn handshake_gate_decision(
        version_received: bool,
        handshake_complete: bool,
        command: MessageCommand,
    ) -> HandshakeGateDecision {
        if !version_received {
            return match command {
                MessageCommand::Version => HandshakeGateDecision::AcceptVersion,
                _ => HandshakeGateDecision::Reject("expected version message before handshake"),
            };
        }

        if !handshake_complete {
            return match command {
                MessageCommand::Verack => HandshakeGateDecision::AcceptVerack,
                _ => HandshakeGateDecision::Reject("expected verack message after version"),
            };
        }

        match command {
            MessageCommand::Version | MessageCommand::Verack => {
                HandshakeGateDecision::Reject("duplicate handshake message after completion")
            }
            _ => HandshakeGateDecision::AcceptProtocol,
        }
    }

    async fn on_inbound(&mut self, message: NetworkMessage, ctx: &mut ActorContext) -> ActorResult {
        let command = message.command();
        debug!(target: "neo", endpoint = %self.endpoint, ?command, "processing inbound message");
        if !self.dispatch_message_received(&message) {
            trace!(
                target: "neo",
                ?command,
                "message processing cancelled by handler"
            );
            return Ok(());
        }

        match Self::handshake_gate_decision(
            self.remote_version.is_some(),
            self.handshake_complete,
            command,
        ) {
            HandshakeGateDecision::AcceptVersion => {
                if let ProtocolMessage::Version(payload) = &message.payload {
                    self.on_version(payload.clone(), ctx).await
                } else {
                    let error = NetworkError::ProtocolViolation {
                        peer: self.endpoint,
                        violation: "expected version payload".to_string(),
                    };
                    self.fail(ctx, error).await
                }
            }
            HandshakeGateDecision::AcceptVerack => self.on_verack(ctx).await,
            HandshakeGateDecision::AcceptProtocol => self.forward_protocol(message, ctx).await,
            HandshakeGateDecision::Reject(reason) => {
                let error = NetworkError::ProtocolViolation {
                    peer: self.endpoint,
                    violation: reason.to_string(),
                };
                self.fail(ctx, error).await
            }
        }
    }

    async fn on_version(&mut self, payload: VersionPayload, ctx: &mut ActorContext) -> ActorResult {
        if payload.network != self.settings.network {
            let error = NetworkError::ProtocolViolation {
                peer: self.endpoint,
                violation: format!(
                    "network magic mismatch (expected {:#X}, received {:#X})",
                    self.settings.network, payload.network
                ),
            };
            return self.fail(ctx, error).await;
        }

        {
            let mut connection = self.connection.lock().await;

            // Check compression based on capabilities (C# parity)
            let local_allows = !self.local_version.capabilities.iter().any(|c| {
                matches!(
                    c,
                    crate::network::p2p::capabilities::NodeCapability::DisableCompression
                )
            });
            let remote_allows = !payload.capabilities.iter().any(|c| {
                matches!(
                    c,
                    crate::network::p2p::capabilities::NodeCapability::DisableCompression
                )
            });

            connection.compression_allowed =
                self.config.enable_compression && local_allows && remote_allows;
            connection.set_node_info();
        }

        let snapshot = self.build_snapshot(&payload);

        let parent = match ctx.parent() {
            Some(parent) => parent,
            None => return Ok(()),
        };

        debug!(
            target: "neo",
            endpoint = %self.endpoint,
            user_agent = %payload.user_agent,
            start_height = snapshot.last_block_index,
            listen_port = snapshot.listen_tcp_port,
            allow_compression = !payload.capabilities
                .iter()
                .any(|c| matches!(c, crate::network::p2p::capabilities::NodeCapability::DisableCompression)),
            "received version payload"
        );

        let self_ref = ctx.self_ref();
        let (reply_tx, reply_rx) = oneshot::channel();
        if let Err(err) = parent.tell(PeerCommand::ConnectionEstablished {
            actor: self_ref,
            snapshot: snapshot.clone(),
            is_trusted: self.is_trusted,
            inbound: self.inbound,
            version: payload.clone(),
            reply: reply_tx,
        }) {
            warn!(
                target: "neo",
                error = %err,
                "failed to notify parent about established connection"
            );
            let error = NetworkError::ConnectionError(
                "unable to coordinate connection establishment with local node".to_string(),
            );
            return self.fail(ctx, error).await;
        }

        match reply_rx.await {
            Ok(true) => {
                self.remote_version = Some(payload.clone());
                self.last_block_index = snapshot.last_block_index;
                // Set is_full_node based on FullNode capability presence
                self.is_full_node = payload
                    .capabilities
                    .iter()
                    .any(|cap| matches!(cap, super::capabilities::NodeCapability::FullNode { .. }));
                self.local_node
                    .update_peer_height(&self.endpoint, snapshot.last_block_index);
                debug!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    is_full_node = self.is_full_node,
                    "connection accepted by local node"
                );
            }
            Ok(false) => {
                debug!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    "connection rejected by local node"
                );
                let error = NetworkError::ProtocolViolation {
                    peer: self.endpoint,
                    violation: "connection rejected by local node".to_string(),
                };
                return self.fail(ctx, error).await;
            }
            Err(_) => {
                debug!(
                    target: "neo",
                    endpoint = %self.endpoint,
                    "connection authorization response dropped"
                );
                let error = NetworkError::ConnectionError(
                    "connection authorization response dropped".to_string(),
                );
                return self.fail(ctx, error).await;
            }
        }

        debug!(target: "neo", endpoint = %self.endpoint, "sending verack after version");
        self.send_verack().await?;
        debug!(target: "neo", endpoint = %self.endpoint, "verack sent after version");
        Ok(())
    }

    async fn on_verack(&mut self, ctx: &mut ActorContext) -> ActorResult {
        {
            let mut connection = self.connection.lock().await;
            connection.set_state(ConnectionState::Ready);
        }
        self.handshake_complete = true;
        self.handshake_done.store(true, Ordering::Relaxed);
        self.verack_received = true;
        self.ack_ready = true;
        self.local_node
            .update_peer_height(&self.endpoint, self.last_block_index);

        info!(
            target: "neo",
            endpoint = %self.endpoint,
            last_block_index = self.last_block_index,
            "verack received; handshake complete"
        );

        if let Some(version) = self.remote_version.clone() {
            let register = TaskManagerCommand::Register { version };
            if let Err(err) = self
                .system
                .task_manager
                .tell_from(register, Some(ctx.self_ref()))
            {
                warn!(target: "neo", error = %err, "failed to notify task manager about session registration");
            }
        }

        self.flush_queue().await
    }

    async fn on_ping(&mut self, payload: &PingPayload, ctx: &mut ActorContext) -> ActorResult {
        self.last_block_index = payload.last_block_index;
        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::Update {
                last_block_index: payload.last_block_index,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", error = %err, "failed to forward peer height update to task manager");
        }
        let local_index = self.current_local_block_index();
        let pong = ProtocolMessage::pong_with_block_index(local_index, payload.nonce);
        self.enqueue_message(NetworkMessage::new(pong)).await
    }

    fn on_pong(&mut self, payload: &PingPayload) {
        if payload.last_block_index > self.last_block_index {
            self.last_block_index = payload.last_block_index;
        }
    }

    fn on_addr(&mut self, payload: AddrPayload, ctx: &ActorContext) {
        if !self.consume_sent_command(MessageCommand::GetAddr) {
            return;
        }

        let mut endpoints = Vec::with_capacity(payload.address_list.len());
        let mut seen = HashSet::new();

        for address in payload.address_list {
            if let Some(endpoint) = address.endpoint() {
                if endpoint.port() > 0 && seen.insert(endpoint) {
                    endpoints.push(endpoint);
                }
            }
        }

        if endpoints.is_empty() {
            return;
        }

        if let Some(parent) = ctx.parent() {
            if let Err(err) = parent.tell(PeerCommand::AddPeers { endpoints }) {
                warn!(
                    target: "neo",
                    error = %err,
                    "failed to forward peer addresses to local node"
                );
            }
        }
    }

    async fn on_get_headers(&mut self, payload: GetBlockByIndexPayload) -> ActorResult {
        let count = Self::normalize_request(payload.count, MAX_HEADERS_COUNT);
        let headers = self.system.headers_from_index(payload.index_start, count);

        if headers.is_empty() {
            return Ok(());
        }

        let message =
            NetworkMessage::new(ProtocolMessage::Headers(HeadersPayload::create(headers)));
        self.enqueue_message(message).await
    }

    fn on_headers(&mut self, payload: HeadersPayload, ctx: &ActorContext) {
        if let Some(last) = payload.headers.last() {
            self.last_block_index = last.index();
            if let Err(err) = self.system.task_manager.tell_from(
                TaskManagerCommand::Update {
                    last_block_index: last.index(),
                },
                Some(ctx.self_ref()),
            ) {
                warn!(target: "neo", error = %err, "failed to report header progress to task manager");
            }
        }

        if !payload.headers.is_empty() {
            let headers = payload.headers.clone();
            if let Err(err) = self.system.blockchain.tell_from(
                BlockchainCommand::Headers(headers.clone()),
                Some(ctx.self_ref()),
            ) {
                warn!(target: "neo", error = %err, "failed to forward headers to blockchain");
            }

            if let Err(err) = self.system.task_manager.tell_from(
                TaskManagerCommand::Headers { headers },
                Some(ctx.self_ref()),
            ) {
                warn!(target: "neo", error = %err, "failed to notify task manager about headers");
            }
        }
    }

    async fn fail(&mut self, ctx: &mut ActorContext, error: NetworkError) -> ActorResult {
        warn!(target: "neo", endpoint = %self.endpoint, error = %error, "remote node failure");
        if !self.inbound {
            if let Some(parent) = ctx.parent() {
                if let Err(err) = parent.tell(PeerCommand::ConnectionFailed {
                    endpoint: self.endpoint,
                }) {
                    error!(target: "neo", error = %err, "failed to notify parent about connection failure");
                }
            }
        }
        ctx.stop_self()?;
        Ok(())
    }

    fn build_snapshot(&self, version: &VersionPayload) -> RemoteNodeSnapshot {
        let listen_tcp_port = version
            .capabilities
            .iter()
            .find_map(|capability| match capability {
                super::capabilities::NodeCapability::TcpServer { port } => Some(*port),
                _ => None,
            })
            .unwrap_or(self.endpoint.port());

        let last_block_index = version
            .capabilities
            .iter()
            .find_map(|capability| match capability {
                super::capabilities::NodeCapability::FullNode { start_height } => {
                    Some(*start_height)
                }
                _ => None,
            })
            .unwrap_or(0);

        let services = version.capabilities.iter().fold(0u64, |mask, capability| {
            mask | (1u64 << capability.capability_type().to_byte() as u64)
        });

        RemoteNodeSnapshot {
            remote_address: self.endpoint,
            remote_port: self.endpoint.port(),
            listen_tcp_port,
            last_block_index,
            version: version.version,
            services,
            timestamp: current_unix_timestamp(),
        }
    }
}

#[async_trait]
impl Actor for RemoteNode {
    async fn handle(
        &mut self,
        message: Box<dyn std::any::Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match message.downcast::<RemoteNodeCommand>() {
            Ok(command) => match *command {
                RemoteNodeCommand::StartProtocol => self.start_protocol(ctx).await,
                RemoteNodeCommand::Send(message) => self.enqueue_message(message).await,
                RemoteNodeCommand::Inbound(message) => self.on_inbound(message, ctx).await,
                RemoteNodeCommand::ConnectionError { error } => self.fail(ctx, error).await,
                RemoteNodeCommand::HandshakeTimeout => {
                    if self.handshake_complete {
                        return Ok(());
                    }
                    let error = NetworkError::ProtocolViolation {
                        peer: self.endpoint,
                        violation: "handshake timeout".to_string(),
                    };
                    self.fail(ctx, error).await
                }
                RemoteNodeCommand::TimerTick => self.on_timer(ctx).await,
                RemoteNodeCommand::Disconnect { reason } => {
                    debug!(target: "neo", endpoint = %self.endpoint, reason, "disconnecting remote node");
                    ctx.stop_self()?;
                    Ok(())
                }
            },
            Err(other) => {
                // Drop unknown message types quietly to avoid log spam and mismatched routing.
                trace!(
                    target: "neo",
                    message_type_id = ?other.as_ref().type_id(),
                    "unknown message routed to remote node actor"
                );
                Ok(())
            }
        }
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        {
            let mut connection = self.connection.lock().await;
            if let Err(err) = connection.close().await {
                warn!(target: "neo", error = %err, "error shutting down TCP stream during stop");
            }
        }
        self.known_hashes.clear();
        self.sent_hashes.clear();
        self.pending_known_hashes.clear();
        self.bloom_filter = None;
        self.cancel_timer();
        if let Some(parent) = ctx.parent() {
            let self_ref = ctx.self_ref();
            if let Err(err) = parent.tell(PeerCommand::ConnectionTerminated { actor: self_ref }) {
                trace!(target: "neo", error = %err, "failed to notify parent about remote node termination");
            }
        }
        Ok(())
    }
}

/// Remote node control messages.
#[derive(Debug, Clone)]
pub enum RemoteNodeCommand {
    StartProtocol,
    Send(NetworkMessage),
    Inbound(NetworkMessage),
    ConnectionError { error: NetworkError },
    Disconnect { reason: String },
    HandshakeTimeout,
    TimerTick,
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        message_handlers, register_message_received_handler, HandshakeGateDecision,
        PendingKnownHashes, RemoteNode, UInt256,
    };
    use crate::i_event_handlers::IMessageReceivedHandler;
    use crate::network::p2p::{message::Message, message_command::MessageCommand, timeouts};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};

    fn make_hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn prune_older_than_removes_stale_entries() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        // Use timestamps within MAX_PENDING_TTL (60s) to avoid auto-prune during try_add.
        // Entry 1 at now - 55s, Entry 2 at now - 5s.
        // When entry 2 is added, auto-prune cutoff is (now - 5) - 60 = now - 65s,
        // which won't remove entry 1 (at now - 55s).
        cache.try_add(make_hash(1), now - Duration::from_secs(55));
        cache.try_add(make_hash(2), now - Duration::from_secs(5));

        // Prune entries older than now - 30s; should remove entry 1 but keep entry 2
        let removed = cache.prune_older_than(now - Duration::from_secs(30));
        assert_eq!(removed, 1);
        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
    }

    #[test]
    fn timeout_stats_increment() {
        timeouts::reset();
        timeouts::inc_handshake_timeout();
        timeouts::inc_read_timeout();
        timeouts::inc_write_timeout();
        let stats = timeouts::stats();
        assert_eq!(stats.handshake, 1);
        assert_eq!(stats.read, 1);
        assert_eq!(stats.write, 1);
    }

    #[test]
    fn timeout_stats_logged() {
        timeouts::reset();
        timeouts::log_stats();
    }

    #[derive(Default)]
    struct TestHandler {
        invocations: AtomicUsize,
    }

    impl IMessageReceivedHandler for TestHandler {
        fn remote_node_message_received_handler(
            &self,
            _system: &dyn std::any::Any,
            _message: &Message,
        ) -> bool {
            self.invocations.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    #[test]
    fn register_handler_tracks_subscription() {
        message_handlers::reset();

        let handler = Arc::new(TestHandler::default());
        let subscription = register_message_received_handler(handler.clone());
        let count = message_handlers::with_handlers(|handlers| handlers.len());
        assert_eq!(count, 1);

        drop(subscription);
        let count = message_handlers::with_handlers(|handlers| handlers.len());
        assert_eq!(count, 0);
    }

    #[test]
    fn summarize_alert_payload_strips_control_chars() {
        let payload = b"Node alert:\nRestart\x07 now";
        let summary = RemoteNode::summarize_alert_payload(payload);
        assert_eq!(summary, "Node alert:\nRestart now");
    }

    #[test]
    fn summarize_alert_payload_serializes_binary_as_hex() {
        let payload = [0xFFu8, 0x00, 0x34, 0xAB];
        let summary = RemoteNode::summarize_alert_payload(&payload);
        assert_eq!(summary, "0xff0034ab");
    }

    #[test]
    fn summarize_alert_payload_truncates_output() {
        let payload = vec![b'a'; RemoteNode::MAX_ALERT_LOG_BYTES + 8];
        let summary = RemoteNode::summarize_alert_payload(&payload);
        assert!(summary.ends_with("..."));
        assert_eq!(
            summary.len(),
            RemoteNode::MAX_ALERT_LOG_BYTES + 3,
            "appends ellipsis when payload is longer than capture window"
        );
    }

    #[test]
    fn handshake_gate_requires_version_first() {
        let version = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Version);
        assert!(matches!(version, HandshakeGateDecision::AcceptVersion));

        let verack = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Verack);
        assert!(matches!(verack, HandshakeGateDecision::Reject(_)));

        let ping = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Ping);
        assert!(matches!(ping, HandshakeGateDecision::Reject(_)));
    }

    #[test]
    fn handshake_gate_requires_verack_after_version() {
        let verack = RemoteNode::handshake_gate_decision(true, false, MessageCommand::Verack);
        assert!(matches!(verack, HandshakeGateDecision::AcceptVerack));

        let ping = RemoteNode::handshake_gate_decision(true, false, MessageCommand::Ping);
        assert!(matches!(ping, HandshakeGateDecision::Reject(_)));
    }

    #[test]
    fn handshake_gate_rejects_duplicate_handshake_messages_after_completion() {
        let version = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Version);
        assert!(matches!(version, HandshakeGateDecision::Reject(_)));

        let verack = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Verack);
        assert!(matches!(verack, HandshakeGateDecision::Reject(_)));

        let ping = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Ping);
        assert!(matches!(ping, HandshakeGateDecision::AcceptProtocol));
    }
}
