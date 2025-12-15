//
// actor.rs - LocalNodeActor implementation
//

use super::*;
use super::helpers::parse_seed_entry;
use super::state::LocalNode;

/// Actor responsible for orchestrating peer management, mirroring C# `LocalNode` behaviour.
pub struct LocalNodeActor {
    pub(super) state: Arc<LocalNode>,
    pub(super) peer: PeerState,
    pub(super) listener: Option<JoinHandle<()>>,
}

impl LocalNodeActor {
    /// Creates a new actor wrapping the provided shared state.
    pub fn new(state: Arc<LocalNode>) -> Self {
        let peer = PeerState::new(state.port());
        Self {
            state,
            peer,
            listener: None,
        }
    }

    pub(super) async fn handle_peer_command(
        &mut self,
        command: PeerCommand,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match command {
            PeerCommand::Configure { config } => {
                self.peer.configure(config, ctx);
                self.state.apply_channels_config(self.peer.config());
                self.start_listener(ctx);
                self.handle_peer_timer(ctx).await
            }
            PeerCommand::AddPeers { endpoints } => {
                self.peer.add_unconnected_peers(endpoints);
                Ok(())
            }
            PeerCommand::Connect {
                endpoint,
                is_trusted,
            } => {
                if self.peer.begin_connect(endpoint, is_trusted) {
                    if self.state.is_pending(&endpoint) {
                        return Ok(());
                    }
                    self.state.track_pending(endpoint);
                    self.initiate_connect(ctx, endpoint, is_trusted).await?;
                }
                Ok(())
            }
            PeerCommand::ConnectionEstablished {
                actor,
                snapshot,
                is_trusted,
                inbound,
                version,
                reply,
            } => {
                debug!(
                    target: "neo",
                    remote = %snapshot.remote_address,
                    inbound,
                    start_height = snapshot.last_block_index,
                    listen_port = snapshot.listen_tcp_port,
                    "processing connection establishment request"
                );
                let allowed = self.state.allow_new_connection(&snapshot, &version);
                if allowed {
                    let registered =
                        self.peer
                            .register_connection(actor.clone(), &snapshot, is_trusted, ctx);
                    if !registered {
                        debug!(
                            target: "neo",
                            remote = %snapshot.remote_address,
                            "connection rejected because peer limits are reached"
                        );
                        if !inbound {
                            self.state.clear_pending(&snapshot.remote_address);
                        }
                        let _ = reply.send(false);
                        return Ok(());
                    }
                    self.state.register_remote_node(
                        actor.clone(),
                        snapshot.clone(),
                        version.clone(),
                    );
                    self.state.add_peer(
                        snapshot.remote_address,
                        Some(snapshot.listen_tcp_port),
                        version.version,
                        snapshot.services,
                        snapshot.last_block_index,
                    );
                    let _ = reply.send(true);
                } else {
                    debug!(
                        target: "neo",
                        remote = %snapshot.remote_address,
                        "connection rejected based on local node policy"
                    );
                    if !inbound {
                        self.state.clear_pending(&snapshot.remote_address);
                    }
                    let _ = reply.send(false);
                }
                Ok(())
            }
            PeerCommand::ConnectionFailed { endpoint } => {
                let was_pending = self.state.is_pending(&endpoint);
                self.peer.connection_failed(endpoint);
                self.state.clear_pending(&endpoint);
                if was_pending {
                    self.requeue_endpoint(endpoint);
                }
                Ok(())
            }
            PeerCommand::ConnectionTerminated { actor } => self.handle_terminated(actor, ctx).await,
            PeerCommand::TimerElapsed => self.handle_peer_timer(ctx).await,
            PeerCommand::QueryConnectingPeers { reply } => {
                let _ = reply.send(self.peer.connecting_endpoints());
                Ok(())
            }
        }
    }

    pub(super) async fn handle_local_command(
        &mut self,
        command: LocalNodeCommand,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match command {
            LocalNodeCommand::AddPeer {
                remote_address,
                listener_tcp_port,
                version,
                services,
                last_block_index,
            } => {
                self.state.add_peer(
                    remote_address,
                    listener_tcp_port,
                    version,
                    services,
                    last_block_index,
                );
            }
            LocalNodeCommand::UpdatePeerHeight {
                remote_address,
                last_block_index,
            } => {
                self.state
                    .update_peer_height(&remote_address, last_block_index);
            }
            LocalNodeCommand::RemovePeer {
                remote_address,
                reply,
            } => {
                let removed = self.state.remove_peer(&remote_address);
                let _ = reply.send(removed);
            }
            LocalNodeCommand::GetPeers { reply } => {
                let peers = self.state.get_peers();
                let _ = reply.send(peers);
            }
            LocalNodeCommand::GetRemoteNodes { reply } => {
                let nodes = self.state.remote_nodes();
                let _ = reply.send(nodes);
            }
            LocalNodeCommand::PeerCount { reply } => {
                let count = self.state.connected_peers_count();
                let _ = reply.send(count);
            }
            LocalNodeCommand::GetInstance { reply } => {
                let _ = reply.send(self.state.clone());
            }
            LocalNodeCommand::RelayDirectly {
                inventory,
                block_index,
            } => {
                self.state.record_relay(&inventory);
                self.send_inventory_to_peers(&inventory, block_index, true);
            }
            LocalNodeCommand::SendDirectly {
                inventory,
                block_index,
            } => {
                self.state.record_send(&inventory);
                self.send_inventory_to_peers(&inventory, block_index, false);
            }
            LocalNodeCommand::RegisterRemoteNode {
                actor,
                snapshot,
                version,
            } => {
                self.state.register_remote_node(actor, snapshot, version);
            }
            LocalNodeCommand::UnregisterRemoteNode { actor } => {
                self.state.unregister_remote_node(&actor);
            }
            LocalNodeCommand::GetRemoteActors { reply } => {
                let actors = self.state.remote_actor_refs();
                let _ = reply.send(actors);
            }
            LocalNodeCommand::UnconnectedCount { reply } => {
                let count = self.peer.unconnected_count();
                let _ = reply.send(count);
            }
            LocalNodeCommand::GetUnconnectedPeers { reply } => {
                let peers = self.peer.unconnected_peers();
                let _ = reply.send(peers);
            }
            LocalNodeCommand::AddUnconnectedPeers { endpoints } => {
                self.peer.add_unconnected_peers(endpoints);
            }
            LocalNodeCommand::InboundTcpAccepted {
                stream,
                remote,
                local,
            } => {
                self.spawn_remote(ctx, stream, remote, local, false, true)
                    .await?;
            }
        }
        Ok(())
    }

    pub(super) async fn handle_peer_timer(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let deficit = self.peer.connection_deficit();
        if deficit == 0 {
            return Ok(());
        }

        if self.peer.connecting_capacity() == 0 {
            return Ok(());
        }

        if !self.peer.has_unconnected_peers() {
            self.need_more_peers(ctx, deficit).await?;
        }

        let targets = self.peer.take_connect_targets(deficit);
        for endpoint in targets {
            if self.peer.begin_connect(endpoint, false) {
                if self.state.is_pending(&endpoint) {
                    continue;
                }
                self.state.track_pending(endpoint);
                self.initiate_connect(ctx, endpoint, false).await?;
            } else {
                self.requeue_endpoint(endpoint);
            }
        }

        Ok(())
    }

    pub(super) async fn handle_terminated(&mut self, actor: ActorRef, ctx: &mut ActorContext) -> ActorResult {
        if let Err(error) = ctx.unwatch(&actor) {
            trace!(target: "neo", error = %error, "failed to unwatch remote node");
        }

        if let Some((_, remote_endpoint)) = self.peer.unregister_connection(&actor) {
            let was_pending = self.state.is_pending(&remote_endpoint);
            self.state.remove_peer(&remote_endpoint);
            self.state.clear_pending(&remote_endpoint);
            if was_pending {
                self.requeue_endpoint(remote_endpoint);
            }
        }

        self.state.unregister_remote_node(&actor);
        Ok(())
    }

    async fn initiate_connect(
        &mut self,
        ctx: &mut ActorContext,
        endpoint: SocketAddr,
        is_trusted: bool,
    ) -> ActorResult {
        match TcpStream::connect(endpoint).await {
            Ok(stream) => {
                if let Err(err) = stream.set_nodelay(true) {
                    warn!(target: "neo", endpoint = %endpoint, error = %err, "failed to enable TCP_NODELAY");
                }

                let local_endpoint = stream
                    .local_addr()
                    .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));

                self.spawn_remote(ctx, stream, endpoint, local_endpoint, is_trusted, false)
                    .await
            }
            Err(error) => {
                if error.kind() == ErrorKind::PermissionDenied {
                    error!(target: "neo", endpoint = %endpoint, error = %error, "permission denied opening outbound connection; not retrying");
                    self.peer.connection_failed(endpoint);
                    self.state.clear_pending(&endpoint);
                } else {
                    debug!(target: "neo", endpoint = %endpoint, error = %error, "connection attempt failed");
                    self.peer.connection_failed(endpoint);
                    self.state.clear_pending(&endpoint);
                    self.requeue_endpoint(endpoint);
                }
                Ok(())
            }
        }
    }

    fn send_inventory_to_peers(
        &self,
        inventory: &RelayInventory,
        block_index: Option<u32>,
        restrict_block_height: bool,
    ) {
        match inventory {
            RelayInventory::Block(block) => {
                let target_index = block_index.unwrap_or(block.index());
                for entry in self.state.remote_entries() {
                    if restrict_block_height && entry.snapshot.last_block_index >= target_index {
                        continue;
                    }

                    let message = NetworkMessage::new(ProtocolMessage::Block(block.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay block to peer"
                        );
                    }
                }
            }
            RelayInventory::Transaction(tx) => {
                for entry in self.state.remote_entries() {
                    let message = NetworkMessage::new(ProtocolMessage::Transaction(tx.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay transaction to peer"
                        );
                    }
                }
            }
            RelayInventory::Extensible(payload) => {
                for entry in self.state.remote_entries() {
                    let message = NetworkMessage::new(ProtocolMessage::Extensible(payload.clone()));
                    if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message)) {
                        warn!(
                            target: "neo",
                            remote = %entry.snapshot.remote_address,
                            %error,
                            "failed to relay extensible payload to peer"
                        );
                    }
                }
            }
        }
    }

    async fn spawn_remote(
        &mut self,
        ctx: &mut ActorContext,
        stream: TcpStream,
        remote: SocketAddr,
        local: SocketAddr,
        is_trusted: bool,
        inbound: bool,
    ) -> ActorResult {
        let config = self.peer.config().clone();
        let connection = Arc::new(Mutex::new(config.build_connection(stream, remote, inbound)));
        let actor_name = format!("remote-{:016x}", rand::random::<u64>());

        let version_payload = self.state.version_payload();
        let settings = self.state.settings();
        let Some(system_context) = self.state.system_context() else {
            warn!(target: "neo", "system context missing when spawning remote node");
            return Ok(());
        };

        let props = RemoteNode::props(
            Arc::clone(&system_context),
            Arc::clone(&self.state),
            Arc::clone(&connection),
            remote,
            local,
            version_payload,
            settings,
            config,
            is_trusted,
            inbound,
        );

        match ctx.actor_of(props, actor_name) {
            Ok(actor) => {
                if let Err(err) = actor.tell(RemoteNodeCommand::StartProtocol) {
                    warn!(target: "neo", endpoint = %remote, error = %err, "failed to start protocol");
                    if !inbound {
                        self.peer.connection_failed(remote);
                        self.state.clear_pending(&remote);
                        self.requeue_endpoint(remote);
                    }
                }
                Ok(())
            }
            Err(err) => {
                warn!(target: "neo", endpoint = %remote, error = %err, "failed to spawn remote node actor");
                if !inbound {
                    self.peer.connection_failed(remote);
                    self.state.clear_pending(&remote);
                    self.requeue_endpoint(remote);
                }
                Ok(())
            }
        }
    }

    async fn need_more_peers(&mut self, _ctx: &mut ActorContext, count: usize) -> ActorResult {
        let requested = count.max(MAX_COUNT_FROM_SEED_LIST);

        if self.peer.connected_count() > 0 {
            trace!(target: "neo", requested, "requesting additional peers from network");
            let message = NetworkMessage::new(ProtocolMessage::GetAddr);
            for entry in self.state.remote_entries() {
                if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message.clone())) {
                    warn!(
                        target: "neo",
                        remote = %entry.snapshot.remote_address,
                        %error,
                        "failed to request peers from remote node"
                    );
                }
            }
            return Ok(());
        }

        let seeds = self.resolve_seed_endpoints().await;
        if seeds.is_empty() {
            warn!(target: "neo", "no seeds available to satisfy peer request");
            return Ok(());
        }

        let mut rng = thread_rng();
        let selection: Vec<_> = seeds
            .iter()
            .copied()
            .choose_multiple(&mut rng, requested.min(seeds.len()));

        if selection.is_empty() {
            return Ok(());
        }

        self.peer.add_unconnected_peers(selection);
        Ok(())
    }

    async fn resolve_seed_endpoints(&self) -> Vec<SocketAddr> {
        let mut endpoints = Vec::new();
        for entry in self.state.seed_list() {
            if let Some((host, port)) = parse_seed_entry(&entry) {
                match lookup_host((host.as_str(), port)).await {
                    Ok(iter) => {
                        for addr in iter {
                            endpoints.push(addr);
                        }
                    }
                    Err(error) => {
                        warn!(target: "neo", seed = %entry, error = %error, "failed to resolve seed");
                    }
                }
            }
        }
        endpoints
    }

    fn requeue_endpoint(&mut self, endpoint: SocketAddr) {
        self.peer.add_unconnected_peers([endpoint]);
    }

    fn start_listener(&mut self, ctx: &ActorContext) {
        if let Some(handle) = self.listener.take() {
            handle.abort();
        }

        let Some(endpoint) = self.peer.config().tcp else {
            return;
        };

        // Bind synchronously to detect errors early and avoid silent listener failures.
        let std_listener = match StdTcpListener::bind(endpoint) {
            Ok(listener) => listener,
            Err(error) => {
                error!(target: "neo", endpoint = %endpoint, %error, "failed to bind TCP listener; local node will stop");
                if let Err(err) = ctx.stop_self() {
                    warn!(target: "neo", error = %err, "failed to stop local node after bind error");
                }
                return;
            }
        };
        std_listener.set_nonblocking(true).unwrap_or_else(
            |err| warn!(target: "neo", error = %err, "failed to set listener non-blocking"),
        );

        let listener = match TcpListener::from_std(std_listener) {
            Ok(l) => l,
            Err(err) => {
                error!(target: "neo", %err, "failed to convert std listener to tokio; local node will stop");
                if let Err(e) = ctx.stop_self() {
                    warn!(target: "neo", error = %e, "failed to stop local node after listener conversion error");
                }
                return;
            }
        };
        let actor_ref = ctx.self_ref();
        self.listener = Some(tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, remote)) => {
                        let local = stream.local_addr().unwrap_or(endpoint);
                        if let Err(err) = stream.set_nodelay(true) {
                            warn!(target: "neo", endpoint = %remote, error = %err, "failed to enable TCP_NODELAY for inbound connection");
                        }
                        if let Err(err) = actor_ref.tell(LocalNodeCommand::InboundTcpAccepted {
                            stream,
                            remote,
                            local,
                        }) {
                            warn!(target: "neo", error = %err, "failed to enqueue inbound connection");
                        }
                    }
                    Err(error) => {
                        warn!(target: "neo", error = %error, "failed to accept inbound connection");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
        }));
    }
}
