//! Actor-based remote node implementation for the Neo P2P protocol.
//! Submodules: handshake.rs, timers.rs, inventory.rs, outbound_queue.rs,
//! routing.rs, message_handlers.rs, pending_known_hashes.rs.

mod alerts;
mod bloom_filter;
mod handshake;
mod inventory;
mod lifecycle;
mod message_handlers;
mod outbound_queue;
mod peer_messages;
mod pending_known_hashes;
mod routing;
mod timers;

use super::{
    channels_config::ChannelsConfig,
    connection::{ConnectionState, PeerConnection},
    local_node::{LocalNode, RemoteNodeSnapshot},
    payloads::VersionPayload,
    peer::PeerCommand,
    task_manager::TaskManagerCommand,
};
use crate::network::error::NetworkError;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::p2p::payloads::inv_payload::InvPayload;
use crate::runtime::{Actor, ActorContext, ActorResult, Cancelable, Props};
use crate::{
    neo_system::NeoSystemContext, protocol_settings::ProtocolSettings, CoreResult, UInt256,
};
use async_trait::async_trait;
use neo_io_crate::HashSetCache;
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{oneshot, Mutex};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, error, info, trace, warn};

use bloom_filter::BloomFilterState;
use lifecycle::HandshakeGateDecision;
pub use message_handlers::{
    register_message_received_handler, unregister_message_received_handler,
    MessageHandlerSubscription,
};
use outbound_queue::{CommandBitSet, OutboundQueues};
use pending_known_hashes::PendingKnownHashes;

const READER_TASK_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);

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
    reader_tasks: TaskTracker,
    reader_cancellation: CancellationToken,
    known_hashes: HashSetCache<UInt256>,
    sent_hashes: HashSetCache<UInt256>,
    pending_known_hashes: PendingKnownHashes,
    bloom_filter: BloomFilterState,
    timer: Option<Cancelable>,
    handshake_timeout: Option<Cancelable>,
    last_block_index: u32,
    _last_height_sent: u32,
    message_queues: OutboundQueues,
    sent_commands: CommandBitSet,
    verack_received: bool,
    ack_ready: bool,
    last_sent: Instant,
    /// Indicates if the remote peer advertised FullNode capability.
    is_full_node: bool,
}

impl RemoteNode {
    fn should_skip_inventory(&self, hash: &UInt256) -> bool {
        self.pending_known_hashes.contains(hash)
            || self.known_hashes.contains(hash)
            || self.sent_hashes.contains(hash)
    }

    fn try_relay_inventory_hash(inventory: &mut super::RelayInventory) -> CoreResult<UInt256> {
        match inventory {
            super::RelayInventory::Block(block) => block.try_hash(),
            super::RelayInventory::Transaction(tx) => tx.try_hash(),
            super::RelayInventory::Extensible(ext) => ext.try_hash(),
        }
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
            reader_tasks: TaskTracker::new(),
            reader_cancellation: CancellationToken::new(),
            known_hashes: HashSetCache::new(cache_capacity),
            sent_hashes: HashSetCache::new(cache_capacity),
            pending_known_hashes: PendingKnownHashes::new(cache_capacity),
            bloom_filter: BloomFilterState::default(),
            timer: None,
            handshake_timeout: None,
            last_block_index: 0,
            _last_height_sent: 0,
            message_queues: OutboundQueues::default(),
            sent_commands: CommandBitSet::default(),
            verack_received: false,
            ack_ready: true,
            last_sent: Instant::now(),
            is_full_node: false,
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
                if let ProtocolMessage::Version(payload) = message.payload {
                    self.on_version(payload, ctx).await
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
        self.cancel_handshake_timeout();
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
            let register = TaskManagerCommand::Register {
                peer: ctx.self_ref(),
                version,
            };
            if let Err(err) = self.system.task_manager.tell(register) {
                warn!(target: "neo", error = %err, "failed to notify task manager about session registration");
            }
        }

        self.flush_queue().await
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
                RemoteNodeCommand::RelayInventory(mut inventory) => {
                    // Relay by sending an INV announcement containing the hash.
                    // The peer will respond with GETDATA if it needs the full payload.
                    let inventory_type = inventory.inventory_type();
                    let hash = match Self::try_relay_inventory_hash(&mut inventory) {
                        Ok(hash) => hash,
                        Err(error) => {
                            warn!(
                                target: "neo",
                                endpoint = %self.endpoint,
                                inventory_type = ?inventory_type,
                                error = %error,
                                "inventory hash computation failed, dropping relay"
                            );
                            return Ok(());
                        }
                    };
                    if self.should_skip_inventory(&hash) {
                        return Ok(());
                    }
                    let inv = InvPayload::create(inventory_type, &[hash]);
                    self.enqueue_message(NetworkMessage::new(ProtocolMessage::Inv(inv)))
                        .await
                }
                RemoteNodeCommand::SendInventory { inventory } => {
                    // Send the full inventory payload directly (no INV/GETDATA dance).
                    let message = match inventory {
                        super::RelayInventory::Block(block) => {
                            NetworkMessage::new(ProtocolMessage::Block(block))
                        }
                        super::RelayInventory::Transaction(tx) => {
                            NetworkMessage::new(ProtocolMessage::Transaction(tx))
                        }
                        super::RelayInventory::Extensible(payload) => {
                            NetworkMessage::new(ProtocolMessage::Extensible(payload))
                        }
                    };
                    self.enqueue_message(message).await
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
        self.stop_reader().await;
        {
            let mut connection = self.connection.lock().await;
            if let Err(err) = connection.close().await {
                warn!(target: "neo", error = %err, "error shutting down TCP stream during stop");
            }
        }
        self.known_hashes.clear();
        self.sent_hashes.clear();
        self.pending_known_hashes.clear();
        self.bloom_filter.clear();
        self.cancel_timer();
        self.cancel_handshake_timeout();
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
    RelayInventory(super::RelayInventory),
    SendInventory { inventory: super::RelayInventory },
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
        lifecycle::HandshakeGateDecision, message_handlers, register_message_received_handler,
        RemoteNode,
    };
    use crate::i_event_handlers::MessageReceivedHandler;
    use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
    use crate::network::p2p::{
        local_node::RelayInventory,
        message::Message,
        messages::{NetworkMessage, ProtocolMessage},
        payloads::{
            block::Block, ping_payload::PingPayload, signer::Signer, transaction::Transaction,
            witness::Witness,
        },
        timeouts,
    };
    use crate::network::{MessageCommand, MessageFlags};
    use crate::{UInt160, WitnessScope};
    use neo_vm_rs::OpCode;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn transaction_with_script(script: Vec<u8>) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(0x0102_0304);
        tx.set_system_fee(1);
        tx.set_network_fee(1);
        tx.set_valid_until_block(42);
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
        tx.set_attributes(Vec::new());
        tx.set_script(script);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
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

    #[test]
    fn relay_inventory_hash_rejects_unserializable_transaction() {
        let tx = transaction_with_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        let mut inventory = RelayInventory::Transaction(tx);

        assert!(RemoteNode::try_relay_inventory_hash(&mut inventory).is_err());
    }

    #[test]
    fn relay_inventory_hash_matches_valid_transaction_try_hash() {
        let tx = transaction_with_script(vec![OpCode::PUSH1.byte()]);
        let expected = tx.try_hash().expect("hash");
        let mut inventory = RelayInventory::Transaction(tx);

        assert_eq!(
            RemoteNode::try_relay_inventory_hash(&mut inventory).unwrap(),
            expected
        );
    }

    #[test]
    fn relay_inventory_hash_matches_valid_block_try_hash() {
        let mut block = Block::new();
        let expected = block.try_hash().expect("hash");
        let mut inventory = RelayInventory::Block(block);

        assert_eq!(
            RemoteNode::try_relay_inventory_hash(&mut inventory).unwrap(),
            expected
        );
    }

    #[test]
    fn relay_inventory_hash_rejects_unserializable_extensible_payload() {
        let mut payload = ExtensiblePayload::new();
        payload.category = "x".repeat(33);
        let mut inventory = RelayInventory::Extensible(payload);

        assert!(RemoteNode::try_relay_inventory_hash(&mut inventory).is_err());
    }

    #[test]
    fn relay_inventory_hash_matches_valid_extensible_try_hash() {
        let mut payload = ExtensiblePayload::new();
        payload.category = "oracle".to_string();
        payload.valid_block_start = 1;
        payload.valid_block_end = 2;
        let expected = payload.try_hash().expect("hash");
        let mut inventory = RelayInventory::Extensible(payload);

        assert_eq!(
            RemoteNode::try_relay_inventory_hash(&mut inventory).unwrap(),
            expected
        );
    }

    #[derive(Default)]
    struct TestHandler {
        invocations: AtomicUsize,
    }

    impl MessageReceivedHandler for TestHandler {
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
    fn handler_iteration_allows_unregister_during_callback() {
        message_handlers::reset();

        let handler = Arc::new(TestHandler::default());
        let mut subscription = Some(register_message_received_handler(handler));

        let seen = message_handlers::with_handlers(|handlers| {
            let seen = handlers.len();
            subscription.take().expect("subscription").unregister();
            seen
        });

        assert_eq!(seen, 1);
        let count = message_handlers::with_handlers(|handlers| handlers.len());
        assert_eq!(count, 0);
    }

    #[test]
    fn build_wire_message_reconstructs_flagged_compressed_message_for_handlers() {
        let mut message =
            NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(7, 77)));
        message.flags = MessageFlags::COMPRESSED;

        let wire = RemoteNode::build_wire_message(&message).expect("wire message");

        assert_eq!(wire.command, MessageCommand::Ping);
        assert!(wire.is_compressed());
        assert_ne!(wire.payload_compressed(), wire.payload());

        match wire.to_protocol_message().expect("ping payload") {
            ProtocolMessage::Ping(payload) => {
                assert_eq!(payload.last_block_index, 7);
                assert_eq!(payload.nonce, 77);
            }
            other => panic!("expected ping payload, got {other:?}"),
        }
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
