use actix::prelude::*;
use tokio::sync::{mpsc, oneshot, RwLock};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use rand::Rng;
use serde::{Serialize, Deserialize};
use getset::{Getters, Setters};
use rand::prelude::IteratorRandom;
use tokio::net::{TcpListener, TcpStream};
use crate::io::serializable_trait::SerializableTrait;
use crate::neo_system::NeoSystem;
use crate::network::payloads::IInventory;
use crate::network::{Message, MessageCommand, RemoteNode};
use crate::network::network_error::NetworkError;
use crate::network::connection::peer::{Peer, PeerMessage, TcpConfig};

#[derive(Getters, Setters)]
pub struct LocalNode {
    #[getset(get = "pub")]
    system: Arc<NeoSystem>,
    seed_list: Vec<SocketAddr>,
    #[getset(get = "pub")]
    remote_nodes: Arc<RwLock<HashMap<Addr<RemoteNode>, RemoteNode>>>,
    tcp_listener: Option<Addr<TcpListener>>,
    timer: Option<SpawnHandle>,
    connected_addresses: Arc<RwLock<HashMap<IpAddr, usize>>>,
    connected_peers: Arc<RwLock<HashMap<Addr<RemoteNode>, SocketAddr>>>,
    unconnected_peers: Arc<RwLock<HashSet<SocketAddr>>>,
    connecting_peers: Arc<RwLock<HashSet<SocketAddr>>>,
    trusted_ip_addresses: Arc<RwLock<HashSet<IpAddr>>>,
    listener_tcp_port: u16,
    max_connections_per_address: usize,
    min_desired_connections: usize,
    max_connections: usize,
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum LocalNodeMessage {
    RelayDirectly(Box<dyn IInventory<Error=NetworkError>>),
    SendDirectly(Box<dyn IInventory<Error=NetworkError>>),
    GetInstance(oneshot::Sender<Addr<LocalNode>>),
}

impl Actor for LocalNode {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        println!("LocalNode actor started");
        self.connect_to_seed_nodes(ctx);
        self.start_listening(ctx);
    }
}

impl Peer for LocalNode {
    fn tcp_listener(&self) -> Option<Addr<TcpListener>> {
        self.tcp_listener.clone()
    }

    fn set_tcp_listener(&mut self, listener: Option<Addr<TcpListener>>) {
        self.tcp_listener = listener;
    }

    fn timer(&self) -> Option<SpawnHandle> {
        self.timer
    }

    fn set_timer(&mut self, timer: Option<SpawnHandle>) {
        self.timer = timer;
    }

    fn connected_addresses(&self) -> Arc<RwLock<HashMap<IpAddr, usize>>> {
        self.connected_addresses.clone()
    }

    fn connected_peers(&self) -> Arc<RwLock<HashMap<Addr<RemoteNode>, SocketAddr>>> {
        self.connected_peers.clone()
    }

    fn unconnected_peers(&self) -> Arc<RwLock<HashSet<SocketAddr>>> {
        self.unconnected_peers.clone()
    }

    fn connecting_peers(&self) -> Arc<RwLock<HashSet<SocketAddr>>> {
        self.connecting_peers.clone()
    }

    fn trusted_ip_addresses(&self) -> Arc<RwLock<HashSet<IpAddr>>> {
        self.trusted_ip_addresses.clone()
    }

    fn listener_tcp_port(&self) -> u16 {
        self.listener_tcp_port
    }

    fn set_listener_tcp_port(&mut self, port: u16) {
        self.listener_tcp_port = port;
    }

    fn max_connections_per_address(&self) -> usize {
        self.max_connections_per_address
    }

    fn set_max_connections_per_address(&mut self, max: usize) {
        self.max_connections_per_address = max;
    }

    fn min_desired_connections(&self) -> usize {
        self.min_desired_connections
    }

    fn set_min_desired_connections(&mut self, min: usize) {
        self.min_desired_connections = min;
    }

    fn max_connections(&self) -> usize {
        self.max_connections
    }

    fn set_max_connections(&mut self, max: usize) {
        self.max_connections = max;
    }

    fn unconnected_max(&self) -> usize {
        self.max_connections * 2
    }
    
    fn need_more_peers(&self, count: usize) {
        let connected_count = self.connected_peers().read().await.len();
        if connected_count >= count {
            return;
        }

        let mut rng = rand::thread_rng();
        let endpoints: Vec<SocketAddr> = self.unconnected_peers().write().await
            .drain()
            .choose_multiple(&mut rng, count - connected_count);

        for endpoint in endpoints {
            self.connect_to_peer(endpoint, false);
        }
    }
    
    fn initiate_tcp_connection(&self, endpoint: SocketAddr) {
        let addr = self.addr.clone();
        tokio::spawn(async move {
            match TcpStream::connect(endpoint).await {
                Ok(stream) => {
                    let (local_addr, remote_addr) = (stream.local_addr().unwrap(), stream.peer_addr().unwrap());
                    addr.do_send(PeerMessage::TcpConnected { remote: remote_addr, local: local_addr });
                },
                Err(_) => {
                    addr.do_send(PeerMessage::TcpCommandFailed(endpoint));
                }
            }
        });
    }
    
    fn bind_tcp_listener(&self, config: TcpConfig) {
        let addr = self.addr.clone();
        tokio::spawn(async move {
            match TcpListener::bind(config.address).await {
                Ok(listener) => {
                    addr.do_send(PeerMessage::TcpBound);
                    loop {
                        match listener.accept().await {
                            Ok((stream, remote_addr)) => {
                                let local_addr = stream.local_addr().unwrap();
                                addr.do_send(PeerMessage::TcpConnected { remote: remote_addr, local: local_addr });
                            },
                            Err(_) => break,
                        }
                    }
                },
                Err(_) => {
                    addr.do_send(PeerMessage::TcpCommandFailed(config.address));
                }
            }
        });
    }
}

impl LocalNode {
    pub const PROTOCOL_VERSION: u32 = 0;
    const MAX_COUNT_FROM_SEED_LIST: usize = 5;

    pub fn new(system: Arc<NeoSystem>) -> Addr<Self> {
        let seed_list = system.settings.seed_list.iter()
            .filter_map(|s| Self::get_ip_endpoint(s))
            .collect();

        let local_node = LocalNode {
            system,
            seed_list,
            remote_nodes: Arc::new(RwLock::new(HashMap::new())),
            tcp_listener: None,
            timer: None,
            connected_addresses: Arc::new(RwLock::new(HashMap::new())),
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
            unconnected_peers: Arc::new(RwLock::new(HashSet::new())),
            connecting_peers: Arc::new(RwLock::new(HashSet::new())),
            trusted_ip_addresses: Arc::new(RwLock::new(HashSet::new())),
            listener_tcp_port: 0,
            max_connections_per_address: 3,
            min_desired_connections: Self::DEFAULT_MIN_DESIRED_CONNECTIONS,
            max_connections: Self::DEFAULT_MAX_CONNECTIONS,
        };

        local_node.start()
    }

    pub async fn connected_count(&self) -> usize {
        self.remote_nodes.read().await.len()
    }

    pub fn nonce() -> u32 {
        static NONCE: once_cell::sync::Lazy<u32> = once_cell::sync::Lazy::new(|| {
            rand::random()
        });
        *NONCE
    }

    pub fn user_agent() -> &'static str {
        static USER_AGENT: once_cell::sync::Lazy<String> = once_cell::sync::Lazy::new(|| {
            let version = env!("CARGO_PKG_VERSION");
            format!("/{}-{}/", env!("CARGO_PKG_NAME"), version)
        });
        &USER_AGENT
    }

    async fn broadcast_message(&self, command: MessageCommand, payload: Option<&dyn SerializableTrait>) {
        let message = Message::create(command, payload);
        self.send_to_remote_nodes(message).await;
    }

    async fn send_to_remote_nodes(&self, message: Message) {
        let nodes = self.remote_nodes.read().await;
        for node in nodes.keys() {
            node.do_send(message.clone());
        }
    }

    fn get_ip_endpoint(host_and_port: &str) -> Option<SocketAddr> {
        let mut parts = host_and_port.split(':');
        let host = parts.next()?;
        let port = parts.next()?.parse().ok()?;

        if let Ok(ip) = host.parse::<IpAddr>() {
            Some(SocketAddr::new(ip, port))
        } else {
            // DNS resolution would go here
            None
        }
    }

    pub async fn allow_new_connection(&self, node: Addr<RemoteNode>, remote_node: &RemoteNode) -> bool {
        if remote_node.version.network != self.system.settings.network {
            return false;
        }
        if remote_node.version.nonce == Self::nonce() {
            return false;
        }

        let mut nodes = self.remote_nodes.write().await;
        for other in nodes.values() {
            if other.remote.ip() == remote_node.remote.ip() && other.version.nonce == remote_node.version.nonce {
                return false;
            }
        }

        if remote_node.remote.port() != remote_node.listener_tcp_port && remote_node.listener_tcp_port != 0 {
            nodes.insert(node, remote_node.clone());
        }

        true
    }

    pub async fn get_remote_nodes(&self) -> Vec<RemoteNode> {
        self.remote_nodes.read().await.values().cloned().collect()
    }

    async fn need_more_peers(&self, count: usize) {
        let count = count.max(Self::MAX_COUNT_FROM_SEED_LIST);
        if !self.remote_nodes.read().await.is_empty() {
            self.broadcast_message(MessageCommand::GetAddr, None).await;
        } else {
            let mut rng = rand::thread_rng();
            let peers: Vec<_> = self.seed_list.choose_multiple(&mut rng, count).cloned().collect();
            self.add_peers(peers).await;
        }
    }

    async fn on_relay_directly(&self, inventory: Box<dyn IInventory<Error=NetworkError>>) {
        let nodes = self.remote_nodes.read().await;
        if let Some(block) = inventory.as_any().downcast_ref::<Block>() {
            for (node, remote_node) in nodes.iter() {
                if block.index > remote_node.last_block_index {
                    node.do_send(NodeMessage::Relay { inventory: inventory.clone() });
                }
            }
        } else {
            for node in nodes.keys() {
                node.do_send(NodeMessage::Relay { inventory: inventory.clone() });
            }
        }
    }

    async fn on_send_directly(&self, inventory: Box<dyn IInventory<Error=NetworkError>>) {
        self.send_to_remote_nodes(inventory).await;
    }

    async fn on_tcp_connected(&self, node: Addr<RemoteNode>) {
        node.do_send(NodeMessage::StartProtocol);
    }
}

impl Handler<LocalNodeMessage> for LocalNode {
    type Result = ();

    fn handle(&mut self, msg: LocalNodeMessage, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            LocalNodeMessage::RelayDirectly(inventory) => {
                ctx.spawn(self.on_relay_directly(inventory));
            }
            LocalNodeMessage::SendDirectly(inventory) => {
                ctx.spawn(self.on_send_directly(inventory));
            }
            LocalNodeMessage::GetInstance(respond_to) => {
                let _ = respond_to.send(ctx.address());
            }
        }
    }
}

impl Handler<PeerMessage> for LocalNode {
    type Result = ();

    fn handle(&mut self, msg: PeerMessage, ctx: &mut Self::Context) -> Self::Result {
        match msg {
            PeerMessage::ChannelsConfig(config) => {
                // TODO: Implement channels configuration handling
            }
            PeerMessage::Timer => {
                ctx.spawn(async move {
                    self.connect_to_new_peers().await;
                }.into_actor(self));
            }
            PeerMessage::Peers { endpoints } => {
                ctx.spawn(async move {
                    self.add_peers(endpoints).await;
                }.into_actor(self));
            }
            PeerMessage::Connect { endpoint, is_trusted } => {
                self.connect_to_peer(endpoint, is_trusted);
            }
            PeerMessage::TcpConnected { remote, local } => {
                ctx.spawn(async move {
                    if let Err(e) = self.on_tcp_connected(remote, local).await {
                        println!("Error handling TCP connection: {:?}", e);
                    }
                }.into_actor(self));
            }
            PeerMessage::TcpBound => {
                println!("TCP listener bound successfully");
            }
            PeerMessage::TcpCommandFailed(addr) => {
                ctx.spawn(async move {
                    self.on_tcp_command_failed(addr).await;
                }.into_actor(self));
            }
            PeerMessage::Terminated(addr) => {
                ctx.spawn(async move {
                    self.on_remote_node_terminated(addr).await;
                }.into_actor(self));
            }
        }
    }
}
