use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use async_trait::async_trait;
use crate::network::payload::{self, Version, Ping};
use crate::network::message::Message;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub address: String,
    pub user_agent: String,
    pub height: u32,
}

#[async_trait]
pub trait AddressablePeer: Send + Sync {
    // ConnectionAddr returns an address-like identifier of this connection
    // before we have a proper one (after the handshake). It's either the
    // address from discoverer (if initiated from node) or one from socket
    // (if connected to node from outside).
    fn connection_addr(&self) -> String;

    // PeerAddr returns the remote address that should be used to establish
    // a new connection to the node. It can differ from the RemoteAddr
    // address in case the remote node is a client and its current
    // connection port is different from the one the other node should use
    // to connect to it. It's only valid after the handshake is completed.
    // Before that, it returns the same address as RemoteAddr.
    fn peer_addr(&self) -> SocketAddr;

    // Version returns peer's version message if the peer has handshaked
    // already.
    fn version(&self) -> Option<Arc<Version>>;
}

#[async_trait]
pub trait Peer: AddressablePeer {
    // RemoteAddr returns the remote address that we're connected to now.
    fn remote_addr(&self) -> SocketAddr;

    fn disconnect(&self, err: Box<dyn Error + Send + Sync>);

    // BroadcastPacket is a context-bound packet enqueuer, it either puts the
    // given packet into the queue or exits with errors if the context expires
    // or peer disconnects. It accepts a slice of bytes that
    // can be shared with other queues (so that message marshalling can be
    // done once for all peers). It returns an error if the peer has not yet
    // completed handshaking.
    async fn broadcast_packet(&self, ctx: Arc<Mutex<()>>, packet: Vec<u8>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // BroadcastHPPacket is the same as BroadcastPacket, but uses a high-priority
    // queue.
    async fn broadcast_hp_packet(&self, ctx: Arc<Mutex<()>>, packet: Vec<u8>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // EnqueueP2PMessage is a blocking packet enqueuer, it doesn't return until
    // it puts the given message into the queue. It returns an error if the peer
    // has not yet completed handshaking. This queue is intended to be used for
    // unicast peer to peer communication that is more important than broadcasts
    // (handled by BroadcastPacket) but less important than high-priority
    // messages (handled by EnqueueHPMessage).
    async fn enqueue_p2p_message(&self, msg: Arc<Message>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // EnqueueP2PPacket is similar to EnqueueP2PMessage, but accepts a slice of
    // message(s) bytes.
    async fn enqueue_p2p_packet(&self, packet: Vec<u8>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // EnqueueHPMessage is similar to EnqueueP2PMessage, but uses a high-priority
    // queue.
    async fn enqueue_hp_message(&self, msg: Arc<Message>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // EnqueueHPPacket is similar to EnqueueHPMessage, but accepts a slice of
    // message(s) bytes.
    async fn enqueue_hp_packet(&self, packet: Vec<u8>) -> Result<(), Box<dyn Error + Send + Sync>>;

    fn last_block_index(&self) -> u32;

    fn handshaked(&self) -> bool;

    fn is_full_node(&self) -> bool;

    // SetPingTimer adds an outgoing ping to the counter and sets a PingTimeout
    // timer that will shut the connection down in case of no response.
    fn set_ping_timer(&self);

    // SendVersion checks handshake status and sends a version message to
    // the peer.
    async fn send_version(&self) -> Result<(), Box<dyn Error + Send + Sync>>;

    async fn send_version_ack(&self, msg: Arc<Message>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // StartProtocol is a goroutine to be run after the handshake. It
    // implements basic peer-related protocol handling.
    async fn start_protocol(&self);

    async fn handle_version(&self, version: Arc<Version>) -> Result<(), Box<dyn Error + Send + Sync>>;

    async fn handle_version_ack(&self) -> Result<(), Box<dyn Error + Send + Sync>>;

    // HandlePing checks ping contents against Peer's state and updates it.
    async fn handle_ping(&self, ping: Arc<Ping>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // HandlePong checks pong contents against Peer's state and updates it.
    async fn handle_pong(&self, pong: Arc<Ping>) -> Result<(), Box<dyn Error + Send + Sync>>;

    // AddGetAddrSent is to inform local peer context that a getaddr command
    // is sent. The decision to send getaddr is server-wide, but it needs to be
    // accounted for in peer's context, thus this method.
    fn add_get_addr_sent(&self);

    // CanProcessAddr checks whether an addr command is expected to come from
    // this peer and can be processed.
    fn can_process_addr(&self) -> bool;
}
