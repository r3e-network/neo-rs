use std::collections::{ VecDeque};
use std::net::{SocketAddr};
use std::sync::Arc;
use std::time::{ Instant};
use actix::dev::Envelope;
use actix::prelude::*;
use tokio::io::{ AsyncWriteExt};
use tokio::net::TcpStream;
use bytes::{BytesMut};
use chrono::Local;
use getset::{Getters, Setters};
use crate::neo_system::NeoSystem;
use crate::network::{Connection, LocalNode, Message, MessageCommand};
use crate::network::capabilities::{FullNodeCapability, NodeCapabilityType, ServerCapability};
use crate::network::payloads::{VersionPayload, PingPayload, InvPayload, IInventory, InventoryType};
use crate::network::connection::peer::Peer;
use neo_io::{CacheInterface, HashSetCache};
use neo_type::H256;

#[derive(Message)]
#[rtype(result = "()")]
struct StartProtocol;

#[derive(Message)]
#[rtype(result = "()")]
struct Relay {
    inventory: Box<dyn IInventory<Error=()>>,
}

#[derive(Getters, Setters)]
pub struct RemoteNode {
    system: Arc<NeoSystem>,
    local_node: Arc<LocalNode>,
    message_queue_high: VecDeque<Message>,
    message_queue_low: VecDeque<Message>,
    last_sent: Instant,
    sent_commands: [bool; 256],
    msg_buffer: BytesMut,
    ack: bool,
    last_height_sent: u32,
    listener: SocketAddr,
    listener_tcp_port: u16,
    #[getset(get = "pub")]
    pub(crate) version: Option<VersionPayload>,
    #[getset(get = "pub")]
    last_block_index: u32,
    #[getset(get = "pub")]
    is_full_node: bool,
    known_hashes: HashSetCache<H256>,
    sent_hashes: HashSetCache<H256>,
    stream: TcpStream,
}

impl RemoteNode {
    pub fn new(
        system: Arc<NeoSystem>,
        local_node: Arc<LocalNode>,
        stream: TcpStream,
        remote: SocketAddr,
        local: SocketAddr,
    ) -> Self {
        let capacity = system.mem_pool.capacity() * 2 / 5;
        let known_hashes = HashSetCache::new(capacity);
        let sent_hashes = HashSetCache::new(capacity);
        Self {
            system,
            local_node,
            message_queue_high: VecDeque::new(),
            message_queue_low: VecDeque::new(),
            last_sent: Instant::now(),
            sent_commands: [false; 256],
            msg_buffer: BytesMut::with_capacity(1024),
            ack: true,
            last_height_sent: 0,
            listener: remote,
            listener_tcp_port: 0,
            version: None,
            last_block_index: 0,
            is_full_node: false,
            known_hashes,
            sent_hashes,
            stream,
        }
    }

    pub fn listener(&self) -> SocketAddr {
        SocketAddr::new(self.listener.ip(), self.listener_tcp_port)
    }

    fn check_message_queue(&mut self) {
        if !self.ack {
            return;
        }
        if let Some(message) = self.message_queue_high.pop_front().or_else(|| self.message_queue_low.pop_front()) {
            self.send_message(message);
        }
    }

    fn enqueue_message(&mut self, message: Message) {
        let is_single = matches!(message.command,
            MessageCommand::Addr | MessageCommand::GetAddr | MessageCommand::GetBlocks |
            MessageCommand::GetHeaders | MessageCommand::Mempool | MessageCommand::Ping |
            MessageCommand::Pong
        );
        let message_queue = match message.command {
            MessageCommand::Alert | MessageCommand::Extensible | MessageCommand::FilterAdd |
            MessageCommand::FilterClear | MessageCommand::FilterLoad | MessageCommand::GetAddr |
            MessageCommand::Mempool => &mut self.message_queue_high,
            _ => &mut self.message_queue_low,
        };
        if !is_single || !message_queue.iter().any(|m| m.command == message.command) {
            message_queue.push_back(message);
            self.last_sent = Instant::now();
        }
        self.check_message_queue();
    }

    async fn on_ack(&mut self) {
        self.ack = true;
        self.check_message_queue();
    }

    async fn on_data(&mut self, data: BytesMut) {
        self.msg_buffer.extend_from_slice(&data);
        while let Some(message) = self.try_parse_message() {
            self.on_message(message).await;
        }
    }

    async fn on_message(&mut self, message: Message) {
        match message.command {
            MessageCommand::Ping => {
                if let Some(payload) = message.payload.as_ping() {
                    if payload.last_block_index > self.last_height_sent {
                        self.last_height_sent = payload.last_block_index;
                    } else {
                        return;
                    }
                }
            }
            _ => {}
        }
        self.enqueue_message(message);
    }

    async fn on_relay(&mut self, inventory: Box<dyn IInventory<Error=()>>) {
        if !self.is_full_node {
            return;
        }
        if inventory.inventory_type() == InventoryType::TX {
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

    async fn on_send(&mut self, inventory: Box<dyn IInventory<Error=()>>) {
        if !self.is_full_node {
            return;
        }
        if inventory.inventory_type() == InventoryType::TX {
            if let Some(ref bloom_filter) = self.bloom_filter {
                if !bloom_filter.test(inventory.as_transaction().unwrap()) {
                    return;
                }
            }
        }
        self.enqueue_message(Message::new(inventory.inventory_type().into(), inventory));
    }

    async fn on_start_protocol(&mut self) {
        let mut capabilities = vec![FullNodeCapability::new(
            NativeContract::Ledger::current_index(&self.system.store_view()),
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

    fn send_message(&mut self, message: Message) {
        self.ack = false;
        let _ = self.stream.write_all(&message.to_bytes());
        self.sent_commands[message.command as usize] = true;
    }

    fn try_parse_message(&mut self) -> Option<Message> {
        let (message, length) = Message::try_deserialize(&self.msg_buffer)?;
        self.msg_buffer.advance(length);
        Some(message)
    }
}

impl Actor for RemoteNode {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);
    }
}

impl Handler<StartProtocol> for RemoteNode {
    type Result = ();

    fn handle(&mut self, _msg: StartProtocol, _ctx: &mut Self::Context) -> Self::Result {
        self.on_start_protocol();
    }
}

impl Handler<Relay> for RemoteNode {
    type Result = ();

    fn handle(&mut self, msg: Relay, _ctx: &mut Self::Context) -> Self::Result {
        self.on_relay(msg.inventory);
    }
}

impl Connection for RemoteNode {
    fn remote(&self) -> SocketAddr {
        self.listener
    }

    fn local(&self) -> SocketAddr {
        self.stream.local_addr().unwrap()
    }

    fn stream(&self) -> &TcpStream {
        &self.stream
    }

    fn stream_mut(&mut self) -> &mut TcpStream {
        &mut self.stream
    }

    fn is_disconnected(&self) -> bool {
        // Implement based on connection state
        self.version.is_none() || self.stream.peer_addr().is_err()
    }

    fn set_disconnected(&mut self, value: bool) {
        if value {
            self.version = None;
        }
    }

    fn new(stream: TcpStream, remote: SocketAddr, local: SocketAddr) -> Self where Self: Sized {
        unimplemented!("Use RemoteNode::new instead")
    }

    async fn disconnect(&mut self, abort: bool) {
        self.set_disconnected(true);
        if abort {
            self.stream.abort().await.ok();
        } else {
            self.stream.shutdown().await.ok();
        }
        // Additional cleanup if needed
        self.message_queue_high.clear();
        self.message_queue_low.clear();
        self.known_hashes.clear();
        self.sent_hashes.clear();
        self.version = None;
        self.last_block_index = 0;
        self.is_full_node = false;
        self.ack = false;
        self.last_sent = Instant::now();
        self.sent_commands = [false; 256];
        self.msg_buffer.clear();
    }

    async fn on_ack(&mut self) {
        self.ack = true;
        self.process_message_queue().await;
    }

    async fn on_data(&mut self, data: BytesMut) {
        self.msg_buffer.extend_from_slice(&data);
        while let Some(message) = self.try_parse_message() {
            self.handle_message(message).await;
        }
    }
}

pub struct RemoteNodeMailbox {
    inner: VecDeque<Envelope<RemoteNode>>,
}

impl PriorityMailbox for RemoteNodeMailbox {
    fn new() -> Self {
        RemoteNodeMailbox {
            inner: VecDeque::new(),
        }
    }

    fn enqueue(&mut self, msg: Envelope<RemoteNode>) {
        match msg.message() {
            Message(msg) if matches!(msg.command,
                MessageCommand::Extensible | MessageCommand::FilterAdd |
                MessageCommand::FilterClear | MessageCommand::FilterLoad |
                MessageCommand::VerAck | MessageCommand::Version |
                MessageCommand::Alert
            ) => self.inner.push_front(msg),
            Tcp::ConnectionClosed(_) | Connection::Close(_) | Connection::Ack(_) => self.inner.push_front(msg),
            _ => self.inner.push_back(msg),
        }
    }

    fn dequeue(&mut self) -> Option<Envelope<RemoteNode>> {
        self.inner.pop_front()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn filter(&mut self, f: impl Fn(&Envelope<RemoteNode>) -> bool) {
        self.inner.retain(f);
    }
}

impl RemoteNodeMailbox {
    fn shall_drop(&self, msg: &Envelope<RemoteNode>) -> bool {
        if let Message(msg) = msg.message() {
            matches!(msg.command,
                MessageCommand::GetAddr | MessageCommand::GetBlocks |
                MessageCommand::GetHeaders | MessageCommand::Mempool
            ) && self.inner.iter().any(|e| {
                if let Message(existing_msg) = e.message() {
                    existing_msg.command == msg.command
                } else {
                    false
                }
            })
        } else {
            false
        }
    }
}