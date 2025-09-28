//! Actor-based remote node implementation mirroring the Akka.NET design.

use super::{
    channels_config::ChannelsConfig,
    connection::{ConnectionState, PeerConnection},
    local_node::{LocalNode, RemoteNodeSnapshot},
    message_command::MessageCommand,
    payloads::{
        addr_payload::{AddrPayload, MAX_COUNT_TO_SEND},
        block::Block,
        extensible_payload::ExtensiblePayload,
        get_block_by_index_payload::GetBlockByIndexPayload,
        get_blocks_payload::GetBlocksPayload,
        headers_payload::{HeadersPayload, MAX_HEADERS_COUNT},
        inv_payload::{InvPayload, MAX_HASHES_COUNT},
        inventory_type::InventoryType,
        ping_payload::PingPayload,
        transaction::Transaction,
        VersionPayload,
    },
    peer::PeerCommand,
    task_manager::TaskManagerCommand,
};
use crate::neo_system::{NeoSystemContext, ProtocolSettings};
use crate::network::error::NetworkError;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::uint256::UInt256;
use akka::{Actor, ActorContext, ActorRef, ActorResult, Props};
use async_trait::async_trait;
use rand::{seq::IteratorRandom, thread_rng};
use std::collections::{HashSet, VecDeque};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, trace, warn};

/// Remote node actor responsible for protocol negotiation and message relay.
pub struct RemoteNode {
    system: Arc<NeoSystemContext>,
    connection: Arc<Mutex<PeerConnection>>,
    endpoint: SocketAddr,
    local_endpoint: SocketAddr,
    local_version: VersionPayload,
    settings: Arc<ProtocolSettings>,
    config: ChannelsConfig,
    is_trusted: bool,
    inbound: bool,
    local_node: Arc<LocalNode>,
    remote_version: Option<VersionPayload>,
    handshake_complete: bool,
    reader_spawned: bool,
    known_hashes: HashSet<UInt256>,
    last_block_index: u32,
    last_height_sent: u32,
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
        Self {
            system,
            local_node,
            connection,
            endpoint,
            local_endpoint,
            local_version,
            settings,
            config,
            is_trusted,
            inbound,
            remote_version: None,
            handshake_complete: false,
            reader_spawned: false,
            known_hashes: HashSet::new(),
            last_block_index: 0,
            last_height_sent: 0,
            message_queue_high: VecDeque::new(),
            message_queue_low: VecDeque::new(),
            sent_commands: [false; 256],
            verack_received: false,
            ack_ready: true,
            last_sent: Instant::now(),
        }
    }

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
        tokio::spawn(async move {
            loop {
                let result = {
                    let mut guard = connection.lock().await;
                    guard.receive_message().await
                };

                match result {
                    Ok(message) => {
                        if let Err(err) = actor.tell(RemoteNodeCommand::Inbound(message)) {
                            warn!(target: "neo", error = %err, "failed to deliver inbound message to remote node actor");
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = actor.tell(RemoteNodeCommand::ConnectionError { error });
                        break;
                    }
                }
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

    fn current_local_block_index(&self) -> u32 {
        self.system.current_block_index()
    }

    async fn on_inbound(&mut self, message: NetworkMessage, ctx: &mut ActorContext) -> ActorResult {
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
            connection.compression_allowed =
                connection.compression_allowed && payload.allow_compression;
            connection.set_node_info();
        }

        let snapshot = self.build_snapshot(&payload);

        let parent = match ctx.parent() {
            Some(parent) => parent,
            None => return Ok(()),
        };

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
            }
            Ok(false) => {
                let error = NetworkError::ProtocolViolation {
                    peer: self.endpoint,
                    violation: "connection rejected by local node".to_string(),
                };
                return self.fail(ctx, error).await;
            }
            Err(_) => {
                let error = NetworkError::ConnectionError(
                    "connection authorization response dropped".to_string(),
                );
                return self.fail(ctx, error).await;
            }
        }

        self.send_verack().await?;
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

        let mut filtered = Vec::new();
        for hash in payload.hashes.iter().copied() {
            if self.known_hashes.insert(hash) {
                filtered.push(hash);
            }
        }

        if filtered.is_empty() {
            return;
        }

        let command = TaskManagerCommand::NewTasks {
            payload: InvPayload::new(payload.inventory_type, filtered),
        };
        if let Err(err) = self
            .system
            .task_manager
            .tell_from(command, Some(ctx.self_ref()))
        {
            warn!(target: "neo", error = %err, "failed to forward inventory announcement to task manager");
        }
    }

    fn notify_inventory_completed(&self, hash: UInt256, ctx: &ActorContext) {
        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::InventoryCompleted { hash },
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
        self.notify_inventory_completed(hash, ctx);
        trace!(target: "neo", hash = %hash, "transaction received from remote node");
        // TODO: forward to mempool/transaction router once available.
        Ok(())
    }

    async fn on_block(&mut self, mut block: Block, ctx: &mut ActorContext) -> ActorResult {
        let hash = block.hash();
        self.last_block_index = block.index();
        self.notify_inventory_completed(hash, ctx);
        trace!(target: "neo", index = block.index(), hash = %hash, "block received from remote node");
        // TODO: forward to blockchain actor once implemented.
        Ok(())
    }

    async fn on_extensible(
        &mut self,
        mut payload: ExtensiblePayload,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        let hash = payload.hash();
        self.notify_inventory_completed(hash, ctx);
        trace!(target: "neo", hash = %hash, "extensible payload received");
        Ok(())
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

    fn on_not_found(&self, payload: InvPayload, ctx: &ActorContext) {
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
    }

    async fn on_mempool(&mut self) -> ActorResult {
        let hashes = self.system.mempool_transaction_hashes();
        if hashes.is_empty() {
            return Ok(());
        }

        for group in InvPayload::create_group(InventoryType::TX, hashes) {
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
            InventoryType::TX => {
                for hash in payload.hashes.iter().copied() {
                    if let Some(transaction) = self.system.try_get_transaction(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Transaction(
                            transaction.clone(),
                        )))
                        .await?;
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Block => {
                for hash in payload.hashes.iter().copied() {
                    if let Some(block) = self.system.try_get_block(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Block(
                            block.clone(),
                        )))
                        .await?;
                        self.last_block_index = block.index();
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Extensible => {
                for hash in payload.hashes.iter().copied() {
                    if let Some(extensible) = self.system.try_get_extensible(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Extensible(
                            extensible.clone(),
                        )))
                        .await?;
                    } else {
                        not_found.push(hash);
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
            ProtocolMessage::GetHeaders(payload) => self.on_get_headers(payload.clone()).await,
            ProtocolMessage::Headers(payload) => {
                self.on_headers(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::Mempool => self.on_mempool().await,
            ProtocolMessage::GetData(payload) => self.on_get_data(payload).await,
            ProtocolMessage::NotFound(payload) => {
                self.on_not_found(payload.clone(), ctx);
                Ok(())
            }
            ProtocolMessage::GetAddr => {
                let mut rng = thread_rng();
                let addresses = self
                    .local_node
                    .address_book()
                    .into_iter()
                    .filter(|addr| addr.endpoint().map(|e| e.port() > 0).unwrap_or(false))
                    .choose_multiple(&mut rng, MAX_COUNT_TO_SEND);

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
            .map_err(|err| akka::error::AkkaError::system(err.to_string()))?;
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
                RemoteNodeCommand::Disconnect { reason } => {
                    debug!(target: "neo", endpoint = %self.endpoint, reason, "disconnecting remote node");
                    ctx.stop_self()?;
                    Ok(())
                }
            },
            Err(other) => {
                warn!(
                    target: "neo",
                    message_type = %other.type_id().name(),
                    "unknown message routed to remote node actor"
                );
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
#[derive(Debug)]
pub enum RemoteNodeCommand {
    StartProtocol,
    Send(NetworkMessage),
    Inbound(NetworkMessage),
    ConnectionError { error: NetworkError },
    Disconnect { reason: String },
}

fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
