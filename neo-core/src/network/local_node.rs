use tokio::sync::{mpsc, oneshot, RwLock};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use neo_crypto::rand;
use crate::io::iserializable::ISerializable;
use crate::neo_system::NeoSystem;
use crate::network::Payloads::IInventory;
use crate::network::{Message, MessageCommand, RemoteNode};

pub struct LocalNode {
    system: Arc<NeoSystem>,
    seed_list: Vec<SocketAddr>,
    remote_nodes: Arc<RwLock<HashMap<mpsc::Sender<NodeMessage>, RemoteNode>>>,
    tx: mpsc::Sender<LocalNodeMessage>,
    rx: mpsc::Receiver<LocalNodeMessage>,
}

pub enum LocalNodeMessage {
    Relay(Box<dyn IInventory>),
    Send(Box<dyn IInventory>),
    GetInstance(oneshot::Sender<Arc<LocalNode>>),
    TcpConnected(mpsc::Sender<NodeMessage>),
}

pub enum NodeMessage {
    StartProtocol,
    Relay { inventory: Box<dyn IInventory> },
}

impl LocalNode {
    pub const PROTOCOL_VERSION: u32 = 0;
    const MAX_COUNT_FROM_SEED_LIST: usize = 5;

    pub fn new(system: Arc<NeoSystem>) -> Arc<Self> {
        let seed_list = system.settings.seed_list.iter()
            .filter_map(|s| Self::get_ip_endpoint(s))
            .collect();

        let (tx, rx) = mpsc::channel(100);

        let node = Arc::new(Self {
            system,
            seed_list,
            remote_nodes: Arc::new(RwLock::new(HashMap::new())),
            tx,
            rx,
        });

        let node_clone = Arc::clone(&node);
        tokio::spawn(async move {
            node_clone.run().await;
        });

        node
    }

    async fn run(&self) {
        while let Some(msg) = self.rx.recv().await {
            match msg {
                LocalNodeMessage::Relay(inventory) => self.on_relay_directly(inventory).await,
                LocalNodeMessage::Send(inventory) => self.on_send_directly(inventory).await,
                LocalNodeMessage::GetInstance(respond_to) => {
                    let _ = respond_to.send(Arc::clone(self));
                },
                LocalNodeMessage::TcpConnected(node_tx) => self.on_tcp_connected(node_tx).await,
            }
        }
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

    async fn broadcast_message(&self, command: MessageCommand, payload: Option<&dyn ISerializable>) {
        let message = Message::create(command, payload);
        self.send_to_remote_nodes(message).await;
    }

    async fn send_to_remote_nodes(&self, message: impl Message) {
        let nodes = self.remote_nodes.read().await;
        for node_tx in nodes.keys() {
            let _ = node_tx.send(NodeMessage::Relay { inventory: message.clone() }).await;
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

    pub async fn allow_new_connection(&self, node_tx: mpsc::Sender<NodeMessage>, node: &RemoteNode) -> bool {
        if node.version.network != self.system.settings.network {
            return false;
        }
        if node.version.nonce == Self::nonce() {
            return false;
        }

        let mut nodes = self.remote_nodes.write().await;
        for other in nodes.values() {
            if other.remote.ip() == node.remote.ip() && other.version.nonce == node.version.nonce {
                return false;
            }
        }

        if node.remote.port() != node.listener_tcp_port && node.listener_tcp_port != 0 {
            nodes.insert(node_tx, node.clone());
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
            let peers = self.seed_list.choose_multiple(&mut rng, count);
            self.add_peers(peers).await;
        }
    }

    async fn on_relay_directly(&self, inventory: Box<dyn IInventory>) {
        let nodes = self.remote_nodes.read().await;
        if let Some(block) = inventory.as_any().downcast_ref::<Block>() {
            for (node_tx, node) in nodes.iter() {
                if block.index > node.last_block_index {
                    let _ = node_tx.send(NodeMessage::Relay { inventory: inventory.clone() }).await;
                }
            }
        } else {
            for node_tx in nodes.keys() {
                let _ = node_tx.send(NodeMessage::Relay { inventory: inventory.clone() }).await;
            }
        }
    }

    async fn on_send_directly(&self, inventory: Box<dyn IInventory>) {
        self.send_to_remote_nodes(inventory).await;
    }

    async fn on_tcp_connected(&self, node_tx: mpsc::Sender<NodeMessage>) {
        let _ = node_tx.send(NodeMessage::StartProtocol).await;
    }
}
