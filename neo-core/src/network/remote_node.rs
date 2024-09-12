use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, IpAddr};
use std::sync::Arc;
use std::time::{ Instant};
use crate::io::caching::HashSetCache;
use crate::neo_system::NeoSystem;
use crate::network::{Message, MessageCommand};
use crate::network::payloads::VersionPayload;
use crate::network::peer::Peer;
use crate::uint256::UInt256;

pub struct RemoteNode {
    system: Arc<NeoSystem>,
    local_node: Arc<Peer>,
    message_queue_high: VecDeque<Message>,
    message_queue_low: VecDeque<Message>,
    last_sent: Instant,
    sent_commands: [bool; 256],
    msg_buffer: Vec<u8>,
    ack: bool,
    last_height_sent: u32,
    listener: SocketAddr,
    listener_tcp_port: u16,
    pub(crate) version: Option<VersionPayload>,
    pub(crate) last_block_index: u32,
    is_full_node: bool,
    known_hashes: HashSetCache<UInt256>,
    sent_hashes: HashSetCache<UInt256>,
}

impl RemoteNode {
    pub fn new(
        system: Arc<NeoSystem>,
        local_node: Arc<Peer>,
        connection: impl Connection,
        remote: SocketAddr,
        local: SocketAddr,
    ) -> Self {
        let capacity = system.mem_pool.capacity() * 2 / 5;
        let known_hashes = HashSetCache::new(capacity);
        let sent_hashes = HashSetCache::new(capacity);
        local_node.remote_nodes.insert(connection.id(), Arc::new(Self {
            system,
            local_node,
            message_queue_high: VecDeque::new(),
            message_queue_low: VecDeque::new(),
            last_sent: Instant::now(),
            sent_commands: [false; 256],
            msg_buffer: Vec::new(),
            ack: true,
            last_height_sent: 0,
            listener: remote,
            listener_tcp_port: 0,
            version: None,
            last_block_index: 0,
            is_full_node: false,
            known_hashes,
            sent_hashes,
        }));
        Self
    }

    pub fn listener(&self) -> SocketAddr {
        SocketAddr::new(self.listener.ip(), self.listener_tcp_port)
    }

    pub fn version(&self) -> Option<&VersionPayload> {
        self.version.as_ref()
    }

    pub fn last_block_index(&self) -> u32 {
        self.last_block_index
    }

    pub fn is_full_node(&self) -> bool {
        self.is_full_node
    }

    fn check_message_queue(&mut self) {
        if !self.ack {
            return;
        }
        if let Some(message) = self.message_queue_high.pop_front() {
            self.send_message(message);
        } else if let Some(message) = self.message_queue_low.pop_front() {
            self.send_message(message);
        }
    }

    fn enqueue_message(&mut self, message: Message) {
        let is_single = match message.command {
            MessageCommand::Addr
            | MessageCommand::GetAddr
            | MessageCommand::GetBlocks
            | MessageCommand::GetHeaders
            | MessageCommand::Mempool
            | MessageCommand::Ping
            | MessageCommand::Pong => true,
            _ => false,
        };
        let message_queue = match message.command {
            MessageCommand::Alert
            | MessageCommand::Extensible
            | MessageCommand::FilterAdd
            | MessageCommand::FilterClear
            | MessageCommand::FilterLoad
            | MessageCommand::GetAddr
            | MessageCommand::Mempool => &mut self.message_queue_high,
            _ => &mut self.message_queue_low,
        };
        if !is_single || !message_queue.iter().any(|m| m.command == message.command) {
            message_queue.push_back(message);
            self.last_sent = Instant::now();
        }
        self.check_message_queue();
    }

    fn on_ack(&mut self) {
        self.ack = true;
        self.check_message_queue();
    }

    fn on_data(&mut self, data: &[u8]) {
        self.msg_buffer.extend_from_slice(data);
        while let Some(message) = self.try_parse_message() {
            self.on_message(message);
        }
    }

    fn on_receive(&mut self, message: impl Message) {
        match message {
            Timer(_) => self.on_timer(),
            Message(msg) => {
                if let Some(payload) = msg.payload.as_ping() {
                    if payload.last_block_index > self.last_height_sent {
                        self.last_height_sent = payload.last_block_index;
                    } else if msg.command == MessageCommand::Ping {
                        return;
                    }
                }
                self.enqueue_message(msg);
            }
            Inventory(inventory) => self.on_send(inventory),
            Relay(relay) => self.on_relay(relay.inventory),
            StartProtocol(_) => self.on_start_protocol(),
        }
    }

    fn on_relay(&mut self, inventory: impl Inventory) {
        if !self.is_full_node {
            return;
        }
        if let InventoryType::TX = inventory.inventory_type() {
            if let Some(ref bloom_filter) = self.bloom_filter {
                if !bloom_filter.test(inventory.as_transaction().unwrap()) {
                    return;
                }
            }
        }
        self.enqueue_message(Message::new(
            MessageCommand::Inv,
            InvPayload::create(inventory.inventory_type(), &[inventory.hash()]),
        ));
    }

    fn on_send(&mut self, inventory: impl Inventory) {
        if !self.is_full_node {
            return;
        }
        if let InventoryType::TX = inventory.inventory_type() {
            if let Some(ref bloom_filter) = self.bloom_filter {
                if !bloom_filter.test(inventory.as_transaction().unwrap()) {
                    return;
                }
            }
        }
        self.enqueue_message(Message::new(inventory.inventory_type().into(), inventory));
    }

    fn on_start_protocol(&mut self) {
        let capabilities = vec![FullNodeCapability::new(
            NativeContract::Ledger::current_index(&self.system.store_view),
        )];
        if self.local_node.listener_tcp_port > 0 {
            capabilities.push(ServerCapability::new(
                NodeCapabilityType::TcpServer,
                self.local_node.listener_tcp_port as u16,
            ));
        }
        self.send_message(Message::new(
            MessageCommand::Version,
            VersionPayload::create(
                self.system.settings.network,
                self.local_node.nonce,
                self.local_node.user_agent.to_owned(),
                &capabilities,
            ),
        ));
    }

    fn post_stop(&mut self) {
        self.timer.cancel();
        self.local_node.remote_nodes.remove(&self.remote_addr);
    }

    fn send_message(&mut self, message: Message) {
        self.ack = false;
        self.send_data(&message.to_bytes());
        self.sent_commands[message.command as usize] = true;
    }

    fn try_parse_message(&mut self) -> Option<Message> {
        let (message, length) = Message::try_deserialize(&self.msg_buffer)?;
        self.msg_buffer.drain(..length);
        Some(message)
    }
}