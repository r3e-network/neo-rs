//
// handle_tokio.rs - Tokio channel-based LocalNode handle and worker
//

use super::{
    LocalNode, LocalNodeCommand, RelayInventory, RemoteNodeSnapshot, TCP_CONNECTION_TIMEOUT,
    BACKGROUND_TASK_SHUTDOWN_TIMEOUT,
};
use crate::network::p2p::{
    ChannelsConfig, NetworkMessage, ProtocolMessage, PeerState,
    RemoteNodeCommand, validate_peer_endpoint,
    MAX_COUNT_FROM_SEED_LIST,
};
use neo_actors::ActorRef;
use rand::{seq::IteratorRandom, thread_rng};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream, lookup_host};
use tokio::sync::{mpsc, broadcast, oneshot};
use tokio::time::{interval, timeout};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::{debug, error, trace, warn};

/// Typed handle for sending commands to the LocalNode worker.
#[derive(Clone)]
pub struct LocalNodeHandle {
    /// Command channel sender.
    tx: mpsc::Sender<LocalNodeCommand>,
    /// Shared state reference.
    state: Arc<LocalNode>,
}

/// Disconnect event broadcast from RemoteNode workers to LocalNode.
#[derive(Debug, Clone)]
pub struct DisconnectEvent {
    pub endpoint: SocketAddr,
    pub reason: String,
}

/// Internal state for the LocalNode worker.
struct LocalNodeWorkerState {
    /// Shared LocalNode state.
    state: Arc<LocalNode>,
    /// Peer management state.
    peer: PeerState,
    /// Listener cancellation token.
    listener_cancellation: Option<CancellationToken>,
    /// Background task tracker.
    spawned_tasks: TaskTracker,
    /// Master cancellation token.
    task_cancellation: CancellationToken,
    /// Deferred connect requests before configuration.
    deferred_connects: Vec<(SocketAddr, bool)>,
    /// Channel to receive disconnect events from RemoteNode workers.
    disconnect_rx: broadcast::Receiver<DisconnectEvent>,
    /// Map of RemoteNode actor refs by endpoint (temporary until full migration).
    remote_actors: HashMap<SocketAddr, ActorRef>,
    /// Command sender for background tasks to communicate with the worker.
    command_tx: mpsc::Sender<LocalNodeCommand>,
}

impl LocalNodeHandle {
    /// Spawns a new LocalNode worker and returns a handle to it.
    pub fn spawn(
        state: Arc<LocalNode>,
        disconnect_rx: broadcast::Receiver<DisconnectEvent>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(256);

        let handle = Self {
            tx: tx.clone(),
            state: Arc::clone(&state),
        };

        let worker_state = LocalNodeWorkerState {
            state: Arc::clone(&state),
            peer: PeerState::new(state.port()),
            listener_cancellation: None,
            spawned_tasks: TaskTracker::new(),
            task_cancellation: CancellationToken::new(),
            deferred_connects: Vec::new(),
            disconnect_rx,
            remote_actors: HashMap::new(),
            command_tx: tx.clone(),
        };

        tokio::spawn(async move {
            local_node_worker(worker_state, rx).await;
        });

        handle
    }

    /// Returns the shared LocalNode state.
    pub fn state(&self) -> &Arc<LocalNode> {
        &self.state
    }

    /// Configures the local node with channel settings.
    pub async fn configure(&self, config: ChannelsConfig) -> Result<(), &'static str> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(LocalNodeCommand::Configure { config, reply })
            .await
            .map_err(|_| "local node worker stopped")?;
        rx.await
            .map_err(|_| "reply channel dropped")
            .and_then(|r| r)
    }

    /// Connects to a peer.
    pub async fn connect_to_peer(
        &self,
        endpoint: SocketAddr,
        is_trusted: bool,
    ) -> Result<(), &'static str> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(LocalNodeCommand::Connect {
                endpoint,
                is_trusted,
                reply,
            })
            .await
            .map_err(|_| "local node worker stopped")?;
        rx.await
            .map_err(|_| "reply channel dropped")
            .and_then(|r| r)
    }

    /// Broadcasts inventory to all connected peers.
    pub async fn broadcast_inventory(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> Result<(), &'static str> {
        self.tx
            .send(LocalNodeCommand::RelayDirectly {
                inventory,
                block_index,
            })
            .await
            .map_err(|_| "local node worker stopped")
    }

    /// Returns the count of connected peers.
    pub async fn get_connected_count(&self) -> Result<usize, &'static str> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(LocalNodeCommand::PeerCount { reply })
            .await
            .map_err(|_| "local node worker stopped")?;
        rx.await.map_err(|_| "reply channel dropped")
    }

    /// Returns connected peer endpoints.
    pub async fn get_peers(&self) -> Result<Vec<SocketAddr>, &'static str> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(LocalNodeCommand::GetPeers { reply })
            .await
            .map_err(|_| "local node worker stopped")?;
        rx.await.map_err(|_| "reply channel dropped")
    }

    /// Returns detailed remote node snapshots.
    pub async fn get_remote_nodes(&self) -> Result<Vec<RemoteNodeSnapshot>, &'static str> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(LocalNodeCommand::GetRemoteNodes { reply })
            .await
            .map_err(|_| "local node worker stopped")?;
        rx.await.map_err(|_| "reply channel dropped")
    }

    /// Adds unconnected peers to the pool.
    pub async fn add_unconnected_peers(
        &self,
        endpoints: Vec<SocketAddr>,
    ) -> Result<(), &'static str> {
        self.tx
            .send(LocalNodeCommand::AddUnconnectedPeers { endpoints })
            .await
            .map_err(|_| "local node worker stopped")
    }
}

/// Main worker loop for LocalNode.
async fn local_node_worker(
    mut worker: LocalNodeWorkerState,
    mut commands: mpsc::Receiver<LocalNodeCommand>,
) {
    let mut peer_timer = interval(Duration::from_secs(5));
    peer_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            Some(cmd) = commands.recv() => {
                if let Err(e) = handle_command(&mut worker, cmd).await {
                    error!(target: "neo", error = %e, "error handling local node command");
                }
            }
            Ok(disconnect) = worker.disconnect_rx.recv() => {
                handle_disconnect(&mut worker, disconnect).await;
            }
            _ = peer_timer.tick() => {
                if let Err(e) = handle_peer_timer(&mut worker).await {
                    error!(target: "neo", error = %e, "error in peer timer");
                }
            }
        }
    }
}

/// Handles incoming LocalNode commands.
async fn handle_command(
    worker: &mut LocalNodeWorkerState,
    command: LocalNodeCommand,
) -> Result<(), &'static str> {
    match command {
        LocalNodeCommand::Configure { config, reply } => {
            // Apply configuration without ActorContext
            worker.peer.config = config.clone();
            worker.peer.listener_tcp_port = config.tcp.map(|e| e.port()).unwrap_or(0);
            worker.peer.configured = true;

            worker.state.apply_channels_config(&config);
            start_listener(worker, config.tcp);

            // Process deferred connects
            let deferred = std::mem::take(&mut worker.deferred_connects);
            for (endpoint, is_trusted) in deferred {
                if worker.peer.begin_connect(endpoint, is_trusted)
                    && !worker.state.is_pending(&endpoint)
                {
                    worker.state.track_pending(endpoint);
                    queue_outbound_connect(worker, endpoint, is_trusted);
                }
            }

            let _ = reply.send(Ok(()));
            handle_peer_timer(worker).await?;
        }
        LocalNodeCommand::Connect {
            endpoint,
            is_trusted,
            reply,
        } => {
            if !worker.peer.is_configured() {
                worker.deferred_connects.push((endpoint, is_trusted));
                let _ = reply.send(Ok(()));
                return Ok(());
            }

            if worker.peer.begin_connect(endpoint, is_trusted) {
                if !worker.state.is_pending(&endpoint) {
                    worker.state.track_pending(endpoint);
                    queue_outbound_connect(worker, endpoint, is_trusted);
                }
            }
            let _ = reply.send(Ok(()));
        }
        LocalNodeCommand::AddPeer {
            remote_address,
            listener_tcp_port,
            version,
            services,
            last_block_index,
        } => {
            worker.state.add_peer(
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
            worker
                .state
                .update_peer_height(&remote_address, last_block_index);
        }
        LocalNodeCommand::RemovePeer {
            remote_address,
            reply,
        } => {
            let removed = worker.state.remove_peer(&remote_address);
            let _ = reply.send(removed);
        }
        LocalNodeCommand::GetPeers { reply } => {
            let peers = worker.state.get_peers();
            let _ = reply.send(peers);
        }
        LocalNodeCommand::GetRemoteNodes { reply } => {
            let nodes = worker.state.remote_nodes();
            let _ = reply.send(nodes);
        }
        LocalNodeCommand::PeerCount { reply } => {
            let count = worker.state.connected_peers_count();
            let _ = reply.send(count);
        }
        LocalNodeCommand::GetInstance { reply } => {
            let _ = reply.send(worker.state.clone());
        }
        LocalNodeCommand::RelayDirectly {
            inventory,
            block_index,
        } => {
            worker.state.record_relay(&inventory);
            send_inventory_to_peers(worker, &inventory, block_index, true);
        }
        LocalNodeCommand::SendDirectly {
            inventory,
            block_index,
        } => {
            worker.state.record_send(&inventory);
            send_inventory_to_peers(worker, &inventory, block_index, false);
        }
        LocalNodeCommand::UnconnectedCount { reply } => {
            let count = worker.peer.unconnected_count();
            let _ = reply.send(count);
        }
        LocalNodeCommand::GetUnconnectedPeers { reply } => {
            let peers = worker.peer.unconnected_peers();
            let _ = reply.send(peers);
        }
        LocalNodeCommand::AddUnconnectedPeers { endpoints } => {
            worker.peer.add_unconnected_peers(endpoints);
        }
        LocalNodeCommand::InboundTcpAccepted {
            stream,
            remote,
            local,
        } => {
            handle_inbound_connection(worker, stream, remote, local).await?;
        }
        LocalNodeCommand::OutboundTcpConnected {
            stream,
            endpoint,
            local,
            is_trusted,
        } => {
            handle_outbound_connected(worker, stream, endpoint, local, is_trusted).await?;
        }
        LocalNodeCommand::OutboundTcpFailed {
            endpoint,
            is_trusted,
            error,
            timed_out,
            permission_denied,
        } => {
            if timed_out {
                warn!(target: "neo", endpoint = %endpoint, "connection attempt timed out");
            } else if permission_denied {
                error!(target: "neo", endpoint = %endpoint, error = %error, "permission denied");
            } else {
                debug!(target: "neo", endpoint = %endpoint, error = %error, "connection failed");
            }
            worker.peer.connection_failed(endpoint);
            worker.state.clear_pending(&endpoint);
        }
    }
    Ok(())
}

/// Handles peer disconnection events.
async fn handle_disconnect(worker: &mut LocalNodeWorkerState, event: DisconnectEvent) {
    debug!(target: "neo", endpoint = %event.endpoint, reason = %event.reason, "peer disconnected");

    worker.remote_actors.remove(&event.endpoint);
    worker.state.remove_peer(&event.endpoint);
    worker.state.clear_pending(&event.endpoint);

    // Remove from peer state
    if let Some(actor_ref) = worker.remote_actors.remove(&event.endpoint) {
        worker.state.unregister_remote_node(&actor_ref);
    }
}

/// Handles periodic peer timer to initiate connections.
async fn handle_peer_timer(worker: &mut LocalNodeWorkerState) -> Result<(), &'static str> {
    let deficit = worker.peer.connection_deficit();
    if deficit == 0 {
        return Ok(());
    }

    if worker.peer.connecting_capacity() == 0 {
        return Ok(());
    }

    // Request more peers if unconnected pool is empty
    if worker.peer.unconnected_count() == 0 {
        need_more_peers(worker, MAX_COUNT_FROM_SEED_LIST).await?;
    }

    let targets = worker.peer.take_connect_targets(deficit);
    for endpoint in targets {
        if worker.peer.begin_connect(endpoint, false) {
            if worker.state.is_pending(&endpoint) {
                continue;
            }
            worker.state.track_pending(endpoint);
            queue_outbound_connect(worker, endpoint, false);
        }
    }

    Ok(())
}

/// Queues an outbound connection attempt.
fn queue_outbound_connect(
    worker: &mut LocalNodeWorkerState,
    endpoint: SocketAddr,
    is_trusted: bool,
) {
    let tx = worker.state.clone(); // Clone for background task
    let cancel = worker.task_cancellation.child_token();
    let command_tx = worker.command_tx.clone();

    drop(worker.spawned_tasks.spawn(async move {
        tokio::select! {
            _ = cancel.cancelled() => {}
            _ = async {
                match timeout(TCP_CONNECTION_TIMEOUT, TcpStream::connect(endpoint)).await {
                    Ok(Ok(stream)) => {
                        if let Err(err) = stream.set_nodelay(true) {
                            warn!(target: "neo", endpoint = %endpoint, error = %err, "failed to enable TCP_NODELAY");
                        }

                        let local = stream
                            .local_addr()
                            .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));

                        debug!(target: "neo", endpoint = %endpoint, "outbound connection established");

                        // Send OutboundTcpConnected command back to worker
                        let _ = command_tx.send(LocalNodeCommand::OutboundTcpConnected {
                            stream,
                            endpoint,
                            local,
                            is_trusted,
                        }).await;
                    }
                    Ok(Err(error)) => {
                        let permission_denied = error.kind() == ErrorKind::PermissionDenied;
                        let _ = command_tx.send(LocalNodeCommand::OutboundTcpFailed {
                            endpoint,
                            is_trusted,
                            error: error.to_string(),
                            timed_out: false,
                            permission_denied,
                        }).await;
                    }
                    Err(_) => {
                        let _ = command_tx.send(LocalNodeCommand::OutboundTcpFailed {
                            endpoint,
                            is_trusted,
                            error: "connection timed out".to_string(),
                            timed_out: true,
                            permission_denied: false,
                        }).await;
                    }
                }
            } => {}
        }
    }));
}

/// Starts the TCP listener for inbound connections.
fn start_listener(worker: &mut LocalNodeWorkerState, tcp_endpoint: Option<SocketAddr>) {
    if let Some(cancel) = worker.listener_cancellation.take() {
        cancel.cancel();
    }

    let Some(endpoint) = tcp_endpoint else {
        return;
    };

    let std_listener = match StdTcpListener::bind(endpoint) {
        Ok(listener) => listener,
        Err(error) => {
            error!(target: "neo", endpoint = %endpoint, %error, "failed to bind TCP listener");
            return;
        }
    };

    std_listener.set_nonblocking(true).unwrap_or_else(|err| {
        warn!(target: "neo", error = %err, "failed to set listener non-blocking")
    });

    let listener = match TcpListener::from_std(std_listener) {
        Ok(l) => l,
        Err(err) => {
            error!(target: "neo", %err, "failed to convert std listener to tokio");
            return;
        }
    };

    let listener_cancellation = worker.task_cancellation.child_token();
    worker.listener_cancellation = Some(listener_cancellation.clone());
    let command_tx = worker.command_tx.clone();

    drop(worker.spawned_tasks.spawn(async move {
        loop {
            tokio::select! {
                _ = listener_cancellation.cancelled() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, remote)) => {
                            debug!(target: "neo", remote = %remote, "inbound connection accepted");

                            let local = stream
                                .local_addr()
                                .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0)));

                            // Send InboundTcpAccepted command to worker
                            let _ = command_tx.send(LocalNodeCommand::InboundTcpAccepted {
                                stream,
                                remote,
                                local,
                            }).await;
                        }
                        Err(error) => {
                            warn!(target: "neo", error = %error, "failed to accept inbound connection");
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                    }
                }
            }
        }
    }));
}

/// Handles inbound connection acceptance.
async fn handle_inbound_connection(
    worker: &mut LocalNodeWorkerState,
    stream: TcpStream,
    remote: SocketAddr,
    _local: SocketAddr,
) -> Result<(), &'static str> {
    if let Err(reason) = worker.state.validate_inbound_connection(&remote) {
        debug!(target: "neo", remote = %remote, reason = %reason, "inbound connection rejected");
        return Ok(());
    }

    if let Err(reason) = validate_peer_endpoint(&remote) {
        debug!(target: "neo", remote = %remote, reason = %reason, "invalid endpoint");
        return Ok(());
    }

    debug!(target: "neo", remote = %remote, "inbound connection validated");
    Ok(())
}

/// Handles successful outbound connection.
async fn handle_outbound_connected(
    worker: &mut LocalNodeWorkerState,
    _stream: TcpStream,
    endpoint: SocketAddr,
    _local: SocketAddr,
    _is_trusted: bool,
) -> Result<(), &'static str> {
    debug!(target: "neo", endpoint = %endpoint, "outbound connection ready for handshake");
    Ok(())
}

/// Sends inventory to all connected peers.
fn send_inventory_to_peers(
    worker: &LocalNodeWorkerState,
    inventory: &RelayInventory,
    block_index: Option<u32>,
    restrict_block_height: bool,
) {
    for entry in worker.state.remote_entries() {
        if restrict_block_height {
            if let RelayInventory::Block(block) = inventory {
                let target_index = block_index.unwrap_or(block.index());
                if entry.snapshot.last_block_index >= target_index {
                    continue;
                }
            }
        }

        let command = if restrict_block_height {
            RemoteNodeCommand::RelayInventory(inventory.clone())
        } else {
            RemoteNodeCommand::SendInventory {
                inventory: inventory.clone(),
            }
        };

        if let Err(error) = entry.actor.tell(command) {
            warn!(target: "neo", remote = %entry.snapshot.remote_address, %error, "failed to send inventory");
        }
    }
}

/// Requests more peers from the network or seed list.
async fn need_more_peers(
    worker: &mut LocalNodeWorkerState,
    count: usize,
) -> Result<(), &'static str> {
    if count == 0 {
        return Ok(());
    }

    if worker.peer.connected_count() > 0 {
        trace!(target: "neo", count, "requesting peers from network");
        let message = NetworkMessage::new(ProtocolMessage::GetAddr);
        for entry in worker.state.remote_entries() {
            if let Err(error) = entry.actor.tell(RemoteNodeCommand::Send(message.clone())) {
                warn!(target: "neo", remote = %entry.snapshot.remote_address, %error, "failed to request peers");
            }
        }
        return Ok(());
    }

    // No connected peers - use seed list
    let seeds = resolve_seed_endpoints(&worker.state).await;
    if seeds.is_empty() {
        warn!(target: "neo", "no seeds available");
        return Ok(());
    }

    let mut rng = thread_rng();
    let selection: Vec<_> = seeds
        .iter()
        .copied()
        .choose_multiple(&mut rng, count.min(seeds.len()));

    if !selection.is_empty() {
        worker.peer.add_unconnected_peers(selection);
    }

    Ok(())
}

/// Resolves seed list to socket addresses.
async fn resolve_seed_endpoints(state: &Arc<LocalNode>) -> Vec<SocketAddr> {
    let mut endpoints = Vec::with_capacity(state.seed_list().len() * 2);
    for entry in state.seed_list() {
        let seed = entry.trim();
        if seed.is_empty() {
            continue;
        }

        match lookup_host(seed).await {
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
    endpoints
}
