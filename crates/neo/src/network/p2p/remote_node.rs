//! Actor-based remote node implementation mirroring the Akka.NET design.

use super::{
    channels_config::ChannelsConfig,
    connection::{ConnectionState, PeerConnection},
    local_node::{LocalNode, RemoteNodeSnapshot},
    message::Message as WireMessage,
    message_command::MessageCommand,
    message_flags::MessageFlags,
    payloads::{
        addr_payload::{AddrPayload, MAX_COUNT_TO_SEND},
        block::Block,
        extensible_payload::ExtensiblePayload,
        filter_add_payload::FilterAddPayload,
        filter_load_payload::FilterLoadPayload,
        get_block_by_index_payload::GetBlockByIndexPayload,
        get_blocks_payload::GetBlocksPayload,
        headers_payload::{HeadersPayload, MAX_HEADERS_COUNT},
        inv_payload::{InvPayload, MAX_HASHES_COUNT},
        inventory_type::InventoryType,
        merkle_block_payload::MerkleBlockPayload,
        ping_payload::PingPayload,
        transaction::Transaction,
        VersionPayload,
    },
    peer::PeerCommand,
    task_manager::TaskManagerCommand,
};
use crate::compression::compress_lz4;
use crate::contains_transaction_type::ContainsTransactionType;
use crate::cryptography::BloomFilter;
use crate::i_event_handlers::IMessageReceivedHandler;
use crate::ledger::blockchain::BlockchainCommand;
use crate::network::error::NetworkError;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::{
    neo_system::{NeoSystemContext, TransactionRouterMessage},
    protocol_settings::ProtocolSettings,
    UInt160, UInt256,
};
use akka::{Actor, ActorContext, ActorResult, Cancelable, Props};
use async_trait::async_trait;
use neo_io_crate::{HashSetCache, KeyedCollectionSlim};
use rand::{seq::IteratorRandom, thread_rng};
use std::any::type_name_of_val;
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, OnceLock, RwLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, trace, warn};

#[derive(Clone, Copy)]
struct PendingKnownHash {
    hash: UInt256,
    timestamp: Instant,
}

struct PendingKnownHashes {
    inner: KeyedCollectionSlim<UInt256, PendingKnownHash>,
}

impl PendingKnownHashes {
    fn new(capacity: usize) -> Self {
        Self {
            inner: KeyedCollectionSlim::with_selector(capacity, |entry: &PendingKnownHash| {
                entry.hash
            }),
        }
    }

    fn contains(&self, hash: &UInt256) -> bool {
        self.inner.contains(hash)
    }

    fn try_add(&mut self, hash: UInt256, timestamp: Instant) -> bool {
        self.inner.try_add(PendingKnownHash { hash, timestamp })
    }

    fn remove(&mut self, hash: &UInt256) -> bool {
        self.inner.remove(hash)
    }

    fn clear(&mut self) {
        self.inner.clear();
    }

    fn prune_older_than(&mut self, cutoff: Instant) -> usize {
        let mut removed = 0;
        loop {
            let Some(entry) = self.inner.first_or_default() else {
                break;
            };
            if entry.timestamp >= cutoff {
                break;
            }
            if !self.inner.remove_first() {
                break;
            }
            removed += 1;
        }
        removed
    }
}

struct MessageHandlerEntry {
    id: usize,
    handler: Arc<dyn IMessageReceivedHandler + Send + Sync>,
}

static MESSAGE_HANDLERS: OnceLock<RwLock<Vec<MessageHandlerEntry>>> = OnceLock::new();
static NEXT_HANDLER_ID: AtomicUsize = AtomicUsize::new(1);

fn handler_registry() -> &'static RwLock<Vec<MessageHandlerEntry>> {
    MESSAGE_HANDLERS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Subscription handle returned when registering message-received callbacks.
#[derive(Debug)]
pub struct MessageHandlerSubscription {
    id: Option<usize>,
}

impl MessageHandlerSubscription {
    /// Explicitly unregisters the handler associated with this subscription.
    pub fn unregister(mut self) {
        if let Some(id) = self.id.take() {
            remove_handler(id);
        }
    }
}

impl Drop for MessageHandlerSubscription {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            remove_handler(id);
        }
    }
}

fn remove_handler(id: usize) {
    if let Ok(mut handlers) = handler_registry().write() {
        handlers.retain(|entry| entry.id != id);
    }
}

/// Registers a new message-received handler (parity with C# `RemoteNode.MessageReceived`).
pub fn register_message_received_handler(
    handler: Arc<dyn IMessageReceivedHandler + Send + Sync>,
) -> MessageHandlerSubscription {
    let id = NEXT_HANDLER_ID.fetch_add(1, Ordering::Relaxed);
    let entry = MessageHandlerEntry { id, handler };
    if let Ok(mut handlers) = handler_registry().write() {
        handlers.push(entry);
    } else {
        warn!(
            target: "neo",
            "message handler registry poisoned; handler will not be retained"
        );
    }
    MessageHandlerSubscription { id: Some(id) }
}

/// Removes a previously registered handler using its subscription token.
pub fn unregister_message_received_handler(subscription: MessageHandlerSubscription) {
    subscription.unregister();
}

const TIMER_INTERVAL: Duration = Duration::from_secs(30);
const PENDING_HASH_TTL: Duration = Duration::from_secs(60);
const PING_INTERVAL: Duration = Duration::from_secs(60);

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
}

impl RemoteNode {
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

    fn ensure_timer(&mut self, ctx: &mut ActorContext) {
        if self.timer.is_some() {
            return;
        }

        let handle = ctx.schedule_tell_repeatedly_cancelable(
            TIMER_INTERVAL,
            TIMER_INTERVAL,
            &ctx.self_ref(),
            RemoteNodeCommand::TimerTick,
            None,
        );
        self.timer = Some(handle);
    }

    fn cancel_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }

    fn dispatch_message_received(&self, message: &NetworkMessage) -> bool {
        let Some(system) = self.system.neo_system() else {
            return true;
        };

        let registry = handler_registry();
        let guard = match registry.read() {
            Ok(guard) => guard,
            Err(poisoned) => {
                warn!(target: "neo", "message handler registry poisoned: {}", poisoned);
                return true;
            }
        };
        if guard.is_empty() {
            return true;
        }

        let Some(wire_message) = Self::build_wire_message(message) else {
            return true;
        };

        for entry in guard.iter() {
            if !entry
                .handler
                .remote_node_message_received_handler(&system, &wire_message)
            {
                return false;
            }
        }
        true
    }

    fn build_wire_message(message: &NetworkMessage) -> Option<WireMessage> {
        if let Some(raw) = message.wire_payload() {
            return WireMessage::from_wire_parts(message.flags, message.command(), raw).ok();
        }

        let payload = match message.payload.to_bytes() {
            Ok(bytes) => bytes,
            Err(err) => {
                warn!(
                    target: "neo",
                    error = %err,
                    "failed to serialize protocol payload for message handlers"
                );
                return None;
            }
        };

        let mut wire = WireMessage {
            flags: message.flags,
            command: message.command(),
            payload_raw: payload.clone(),
            payload_compressed: payload.clone(),
        };

        if wire.flags.is_compressed() {
            match compress_lz4(&wire.payload_raw) {
                Ok(compressed) => wire.payload_compressed = compressed,
                Err(err) => {
                    warn!(
                        target: "neo",
                        error = %err,
                        "failed to recompress payload for message handlers"
                    );
                    wire.flags = MessageFlags::NONE;
                    wire.payload_compressed = wire.payload_raw.clone();
                }
            }
        }

        Some(wire)
    }

    fn on_filter_load(&mut self, payload: &FilterLoadPayload) {
        if payload.filter.is_empty() || payload.k == 0 {
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
        if let Some(filter) = self.bloom_filter.as_mut() {
            filter.add(&payload.data);
        }
    }

    fn bloom_filter_flags(&self, block: &Block) -> Option<Vec<bool>> {
        let filter = self.bloom_filter.as_ref()?;
        Some(
            block
                .transactions
                .iter()
                .map(|tx| Self::filter_matches_transaction(filter, tx))
                .collect(),
        )
    }

    fn filter_matches_transaction(filter: &BloomFilter, tx: &Transaction) -> bool {
        let hash_bytes = tx.hash().to_array();
        if filter.check(&hash_bytes) {
            return true;
        }

        tx.signers().iter().any(|signer| {
            let account_bytes = signer.account.as_bytes();
            filter.check(account_bytes.as_ref())
        })
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

    async fn start_protocol(&mut self, ctx: &mut ActorContext) -> ActorResult {
        debug!(
            target: "neo",
            endpoint = %self.endpoint,
            reader_spawned = self.reader_spawned,
            "starting protocol handshake"
        );
        self.ensure_timer(ctx);
        self.spawn_reader(ctx);

        let mut connection = self.connection.lock().await;
        connection.set_state(ConnectionState::Handshaking);
        connection.compression_allowed =
            self.local_version.allow_compression && self.config.enable_compression;

        let message = NetworkMessage::new(ProtocolMessage::Version(self.local_version.clone()));
        drop(connection);
        if let Err(error) = self.send_wire_message(&message).await {
            let network_error = NetworkError::ConnectionError(error.to_string());
            self.fail(ctx, network_error).await?;
        }
        Ok(())
    }

    fn spawn_reader(&mut self, ctx: &ActorContext) {
        if self.reader_spawned {
            return;
        }

        let actor = ctx.self_ref();
        let connection = Arc::clone(&self.connection);
        let endpoint = self.endpoint;
        tokio::spawn(async move {
            loop {
                let result = {
                    let mut guard = connection.lock().await;
                    debug!(target: "neo", endpoint = %guard.address, "waiting for inbound message");
                    guard.receive_message().await
                };

                match result {
                    Ok(message) => {
                        let command = message.command();
                        if let Err(err) = actor.tell(RemoteNodeCommand::Inbound(message)) {
                            warn!(target: "neo", error = %err, "failed to deliver inbound message to remote node actor");
                            break;
                        } else {
                            debug!(
                                target: "neo",
                                endpoint = %endpoint,
                                ?command,
                                "enqueued inbound message to actor"
                            );
                        }
                    }
                    Err(error) => {
                        let _ = actor.tell(RemoteNodeCommand::ConnectionError { error });
                        break;
                    }
                }

                tokio::task::yield_now().await;
            }
        });

        self.reader_spawned = true;
    }

    fn is_single_command(command: MessageCommand) -> bool {
        matches!(
            command,
            MessageCommand::Addr
                | MessageCommand::GetAddr
                | MessageCommand::GetBlocks
                | MessageCommand::GetHeaders
                | MessageCommand::Mempool
                | MessageCommand::Ping
                | MessageCommand::Pong
        )
    }

    fn is_high_priority(command: MessageCommand) -> bool {
        matches!(
            command,
            MessageCommand::Alert
                | MessageCommand::Extensible
                | MessageCommand::FilterAdd
                | MessageCommand::FilterClear
                | MessageCommand::FilterLoad
                | MessageCommand::GetAddr
                | MessageCommand::Mempool
        )
    }

    async fn enqueue_message(&mut self, message: NetworkMessage) -> ActorResult {
        let command = message.command();
        let target_queue = if Self::is_high_priority(command) {
            &mut self.message_queue_high
        } else {
            &mut self.message_queue_low
        };

        if Self::is_single_command(command)
            && target_queue
                .iter()
                .any(|queued| queued.command() == command)
        {
            return Ok(());
        }

        target_queue.push_back(message);
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

    async fn on_inbound(&mut self, message: NetworkMessage, ctx: &mut ActorContext) -> ActorResult {
        debug!(
            target: "neo",
            endpoint = %self.endpoint,
            command = ?message.command(),
            "processing inbound message"
        );
        if !self.dispatch_message_received(&message) {
            trace!(
                target: "neo",
                command = ?message.command(),
                "message processing cancelled by handler"
            );
            return Ok(());
        }

        match &message.payload {
            ProtocolMessage::Version(payload) => self.on_version(payload.clone(), ctx).await,
            ProtocolMessage::Verack => self.on_verack(ctx).await,
            _ => self.forward_protocol(message, ctx).await,
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
            connection.compression_allowed = self.config.enable_compression
                && self.local_version.allow_compression
                && payload.allow_compression;
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
            allow_compression = payload.allow_compression,
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
                self.local_node
                    .update_peer_height(&self.endpoint, snapshot.last_block_index);
                debug!(
                    target: "neo",
                    endpoint = %self.endpoint,
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

    fn on_inv(&mut self, payload: &InvPayload, ctx: &mut ActorContext) {
        if payload.is_empty() {
            return;
        }

        let now = Instant::now();
        let ledger_contract = LedgerContract::new();
        let mut hashes = Vec::new();

        match payload.inventory_type {
            InventoryType::Block => {
                let store_cache = self.system.store_cache();
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    if ledger_contract.contains_block(&store_cache, &hash) {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
            InventoryType::Transaction => {
                let store_cache = self.system.store_cache();
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    if ledger_contract
                        .contains_transaction(&store_cache, &hash)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
            _ => {
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
        }

        if hashes.is_empty() {
            return;
        }

        for hash in &hashes {
            self.pending_known_hashes.try_add(*hash, now);
        }

        let command = TaskManagerCommand::NewTasks {
            payload: InvPayload::new(payload.inventory_type, hashes),
        };
        if let Err(err) = self
            .system
            .task_manager
            .tell_from(command, Some(ctx.self_ref()))
        {
            warn!(target: "neo", error = %err, "failed to forward inventory announcement to task manager");
        }
    }

    fn notify_inventory_completed(
        &self,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
        ctx: &ActorContext,
    ) {
        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::InventoryCompleted {
                hash,
                block: Box::new(block),
                block_index,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", error = %err, "failed to notify task manager about inventory completion");
        }
    }

    async fn on_transaction(
        &mut self,
        transaction: Transaction,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let hash = transaction.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.notify_inventory_completed(hash, None, None, ctx);

        let contains = self.system.contains_transaction(&hash);
        let signer_accounts: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        let has_conflict = !signer_accounts.is_empty()
            && self.system.contains_conflict_hash(&hash, &signer_accounts);

        if contains != ContainsTransactionType::NotExist || has_conflict {
            trace!(
                target: "neo",
                hash = %hash,
                contains = ?contains,
                has_conflict,
                "transaction skipped because it is already known or conflicts on-chain"
            );
            return Ok(());
        }

        if let Err(err) = self.system.tx_router.tell_from(
            TransactionRouterMessage::Preverify {
                transaction: transaction.clone(),
                relay: true,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", %hash, error = %err, "failed to enqueue transaction for preverification");
        }

        Ok(())
    }

    async fn on_block(&mut self, mut block: Block, ctx: &mut ActorContext) -> ActorResult {
        let hash = block.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.last_block_index = block.index();
        let block_clone = block.clone();
        self.notify_inventory_completed(hash, Some(block_clone), Some(block.index()), ctx);
        let current_height = self.system.current_block_index();
        if block.index() > current_height.saturating_add(MAX_HASHES_COUNT as u32) {
            return Ok(());
        }
        trace!(target: "neo", index = block.index(), hash = %hash, "block received from remote node");

        if let Err(err) = self.system.blockchain.tell_from(
            BlockchainCommand::InventoryBlock { block, relay: true },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", hash = %hash, error = %err, "failed to forward block to blockchain actor");
        }
        Ok(())
    }

    async fn on_extensible(
        &mut self,
        mut payload: ExtensiblePayload,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let hash = payload.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.notify_inventory_completed(hash, None, None, ctx);
        trace!(target: "neo", hash = %hash, "extensible payload received");
        if let Err(err) = self.system.blockchain.tell_from(
            BlockchainCommand::InventoryExtensible {
                payload,
                relay: true,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                hash = %hash,
                error = %err,
                "failed to forward extensible payload to blockchain actor"
            );
        }
        Ok(())
    }

    fn on_addr(&mut self, payload: AddrPayload, ctx: &ActorContext) {
        if !self.consume_sent_command(MessageCommand::GetAddr) {
            return;
        }

        let mut endpoints = Vec::new();
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

    async fn on_get_blocks(&mut self, payload: GetBlocksPayload) -> ActorResult {
        let count = Self::normalize_request(payload.count, MAX_HASHES_COUNT);
        let hashes = self.system.block_hashes_from(&payload.hash_start, count);

        if hashes.is_empty() {
            return Ok(());
        }

        for group in InvPayload::create_group(InventoryType::Block, hashes) {
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Inv(group)))
                .await?;
        }

        Ok(())
    }

    async fn on_get_block_by_index(&mut self, payload: GetBlockByIndexPayload) -> ActorResult {
        let count = Self::normalize_request(payload.count, MAX_HASHES_COUNT);
        if count == 0 {
            return Ok(());
        }

        for offset in 0..count {
            let index = payload.index_start.saturating_add(offset as u32);

            let Some(hash) = self.system.block_hash_at(index) else {
                break;
            };

            let Some(mut block) = self.system.try_get_block(&hash) else {
                break;
            };

            if let Some(flags) = self.bloom_filter_flags(&block) {
                let payload = MerkleBlockPayload::create(&mut block, flags);
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::MerkleBlock(payload)))
                    .await?;
            } else {
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::Block(block)))
                    .await?;
            }
        }

        Ok(())
    }

    fn on_not_found(&mut self, payload: InvPayload, ctx: &ActorContext) {
        for hash in &payload.hashes {
            self.pending_known_hashes.remove(hash);
        }

        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::RestartTasks { payload },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", error = %err, "failed to request task restart after notfound");
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

    async fn on_mempool(&mut self) -> ActorResult {
        let hashes = self.system.mempool_transaction_hashes();
        if hashes.is_empty() {
            return Ok(());
        }

        for group in InvPayload::create_group(InventoryType::Transaction, hashes) {
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Inv(group)))
                .await?;
        }

        Ok(())
    }

    async fn on_get_data(&mut self, payload: &InvPayload) -> ActorResult {
        if payload.is_empty() {
            return Ok(());
        }

        let mut not_found = Vec::new();

        match payload.inventory_type {
            InventoryType::Transaction => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(transaction) = self.system.try_get_transaction_from_mempool(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Transaction(
                            transaction,
                        )))
                        .await?;
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Block => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(mut block) = self.system.try_get_block(&hash) {
                        if let Some(flags) = self.bloom_filter_flags(&block) {
                            let payload = MerkleBlockPayload::create(&mut block, flags);
                            self.enqueue_message(NetworkMessage::new(
                                ProtocolMessage::MerkleBlock(payload),
                            ))
                            .await?;
                        } else {
                            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Block(
                                block,
                            )))
                            .await?;
                        }
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Consensus | InventoryType::Extensible => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(extensible) = self.system.try_get_relay_extensible(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Extensible(
                            extensible,
                        )))
                        .await?;
                    } else if let Some(extensible) = self.system.try_get_extensible(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Extensible(
                            extensible,
                        )))
                        .await?;
                    }
                }
            }
        }

        if !not_found.is_empty() {
            for group in InvPayload::create_group(payload.inventory_type, not_found) {
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::NotFound(group)))
                    .await?;
            }
        }

        Ok(())
    }

    async fn on_timer(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let cutoff = Instant::now()
            .checked_sub(PENDING_HASH_TTL)
            .unwrap_or_else(Instant::now);
        let removed = self.pending_known_hashes.prune_older_than(cutoff);
        if removed > 0 {
            trace!(target: "neo", removed, "expired pending known hashes removed");
        }

        if self.handshake_complete && self.last_sent.elapsed() >= PING_INTERVAL {
            let payload = PingPayload::create(self.current_local_block_index());
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Ping(payload)))
                .await?;
        }

        self.ensure_timer(ctx);
        Ok(())
    }

    async fn forward_protocol(
        &mut self,
        message: NetworkMessage,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if !self.handshake_complete {
            trace!(target: "neo", command = ?message.command(), "dropping protocol message prior to handshake");
            return Ok(());
        }

        match &message.payload {
            ProtocolMessage::Ping(payload) => self.on_ping(payload, ctx).await,
            ProtocolMessage::Pong(payload) => {
                self.on_pong(payload);
                Ok(())
            }
            ProtocolMessage::Inv(payload) => {
                self.on_inv(payload, ctx);
                Ok(())
            }
            ProtocolMessage::Transaction(tx) => self.on_transaction(tx.clone(), ctx).await,
            ProtocolMessage::Block(block) => self.on_block(block.clone(), ctx).await,
            ProtocolMessage::Extensible(payload) => self.on_extensible(payload.clone(), ctx).await,
            ProtocolMessage::GetBlocks(payload) => self.on_get_blocks(payload.clone()).await,
            ProtocolMessage::GetBlockByIndex(payload) => {
                self.on_get_block_by_index(payload.clone()).await
            }
            ProtocolMessage::GetHeaders(payload) => self.on_get_headers(payload.clone()).await,
            ProtocolMessage::Headers(payload) => {
                self.on_headers(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Addr(payload) => {
                self.on_addr(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Mempool => self.on_mempool().await,
            ProtocolMessage::GetData(payload) => self.on_get_data(payload).await,
            ProtocolMessage::FilterLoad(payload) => {
                self.on_filter_load(payload);
                Ok(())
            }
            ProtocolMessage::FilterAdd(payload) => {
                self.on_filter_add(payload);
                Ok(())
            }
            ProtocolMessage::FilterClear => {
                self.on_filter_clear();
                Ok(())
            }
            ProtocolMessage::NotFound(payload) => {
                self.on_not_found(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::GetAddr => {
                let addresses = {
                    let mut rng = thread_rng();
                    self.local_node
                        .address_book()
                        .into_iter()
                        .filter(|addr| addr.endpoint().map(|e| e.port() > 0).unwrap_or(false))
                        .choose_multiple(&mut rng, MAX_COUNT_TO_SEND)
                };

                if addresses.is_empty() {
                    return Ok(());
                }

                let payload = AddrPayload::create(addresses);
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::Addr(payload)))
                    .await
            }
            _ => Ok(()),
        }
    }

    async fn send_verack(&mut self) -> ActorResult {
        let message = NetworkMessage::new(ProtocolMessage::Verack);
        self.send_wire_message(&message).await
    }

    async fn send_wire_message(&mut self, message: &NetworkMessage) -> ActorResult {
        let mut connection = self.connection.lock().await;
        connection
            .send_message(message.clone())
            .await
            .map_err(|err| akka::AkkaError::system(err.to_string()))?;
        self.last_sent = Instant::now();
        let index = message.command().to_byte() as usize;
        if index < self.sent_commands.len() {
            self.sent_commands[index] = true;
        }
        Ok(())
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
                RemoteNodeCommand::TimerTick => self.on_timer(ctx).await,
                RemoteNodeCommand::Disconnect { reason } => {
                    debug!(target: "neo", endpoint = %self.endpoint, reason, "disconnecting remote node");
                    ctx.stop_self()?;
                    Ok(())
                }
            },
            Err(other) => {
                // Drop unknown message types quietly to avoid log spam and mismatched routing.
                trace!(target: "neo", message_type = %type_name_of_val(other.as_ref()), "unknown message routed to remote node actor");
                Ok(())
            }
        }
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        if let Ok(mut connection) = self.connection.try_lock() {
            if let Err(err) = connection.stream.shutdown().await {
                trace!(target: "neo", error = %err, "failed to shutdown TCP stream during stop");
            }
            connection.set_state(ConnectionState::Disconnected);
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
        handler_registry, register_message_received_handler, IMessageReceivedHandler,
        PendingKnownHashes, UInt256,
    };
    use crate::network::p2p::message::Message;
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };

    fn make_hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn prune_older_than_removes_stale_entries() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now - Duration::from_secs(120));
        cache.try_add(make_hash(2), now - Duration::from_secs(30));

        let removed = cache.prune_older_than(now - Duration::from_secs(60));
        assert_eq!(removed, 1);
        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
    }

    #[derive(Default)]
    struct TestHandler {
        invocations: AtomicUsize,
    }

    impl IMessageReceivedHandler for TestHandler {
        fn remote_node_message_received_handler(
            &self,
            _system: &crate::neo_system::NeoSystem,
            _message: &Message,
        ) -> bool {
            self.invocations.fetch_add(1, Ordering::Relaxed);
            true
        }
    }

    #[test]
    fn register_handler_tracks_subscription() {
        // ensure registry starts empty
        handler_registry().write().unwrap().clear();

        let handler = Arc::new(TestHandler::default());
        let subscription = register_message_received_handler(handler.clone());
        {
            let guard = handler_registry().read().unwrap();
            assert_eq!(guard.len(), 1);
        }
        drop(subscription);
        {
            let guard = handler_registry().read().unwrap();
            assert!(guard.is_empty());
        }
    }
}
