// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use neo_base::errors;
use neo_core::types::{Bytes, Network, DEFAULT_PER_BLOCK_MILLIS, SEED_LIST_DEV_NET};

pub use {codec::*, discovery::*, driver_v2::*, handle_v2::*, node::*, peer::*};

pub mod codec;
pub mod discovery;
pub mod driver_v2;
pub mod handle_v2;
pub mod node;
pub mod peer;

#[cfg(test)]
mod handle_v2_test;

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum NetEvent {
    Connected,
    NotConnected,
    Accepted,
    Message(Bytes),
    Disconnected,
    Timeout,
}

impl NetEvent {
    #[inline]
    pub fn with_peer(self, peer: SocketAddr) -> NetMessage {
        NetMessage { peer, event: self }
    }
}

#[derive(Debug, Clone)]
pub struct NetMessage {
    pub peer: SocketAddr,
    pub event: NetEvent,
}

// pub trait MessageHandle: Send + 'static {
//     fn on_received(self, net_rx: mpsc::Receiver<NetMessage>, discovery: Discovery);
// }

#[derive(Debug, Clone, errors::Error)]
pub enum SendError {
    #[error("send: timeout after {0:?}")]
    Timeout(Duration),

    #[error("send: channel has fulled")]
    Fulled,

    #[error("send: channel has closed")]
    Closed,
}

#[derive(Debug, errors::Error)]
pub enum DialError {
    #[error("dial: too many dials")]
    TooManyDials,
}

pub trait Dial {
    fn dial(&self, peer: SocketAddr) -> Result<(), DialError>;
}

impl Dial for mpsc::Sender<SocketAddr> {
    #[inline]
    fn dial(&self, peer: SocketAddr) -> Result<(), DialError> {
        self.try_send(peer).map_err(|_err| DialError::TooManyDials)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct P2pConfig {
    pub nonce: u32,
    pub min_peers: u32,
    pub max_peers: u32,
    pub attempt_peers: u32,

    /// Broadcast interval is  discovery_factor * per_block_millis
    pub discovery_factor: u32,
    pub broadcast_factor: u32,

    /// Listen local socket-addr, like "0.0.0.0:10234"
    pub listen: String,
    // pub announced_port: u16,
    pub network: u32,

    /// FullNode is enabled if relay is true
    pub relay: bool,
    pub seeds: Vec<String>,

    // i.e. protocol_tick_interval
    pub tick_interval: Duration,
    pub ping_interval: Duration,
    pub ping_timeout: Duration,
    pub dial_timeout: Duration,

    pub per_block_millis: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        let nonce = neo_crypto::rand::read_u64().expect("`rand::read_u64()` should be ok");
        Self {
            nonce: nonce as u32,
            min_peers: 3,
            max_peers: 128,
            attempt_peers: 20,
            discovery_factor: 1000,
            broadcast_factor: 0,
            listen: "127.0.0.1:10234".into(),
            network: Network::DevNet.as_magic(),
            relay: true,
            seeds: SEED_LIST_DEV_NET.iter().map(|&x| x.into()).collect(),
            tick_interval: Duration::from_secs(3),
            ping_interval: Duration::from_secs(30),
            ping_timeout: Duration::from_secs(90),
            dial_timeout: Duration::from_secs(5),
            per_block_millis: DEFAULT_PER_BLOCK_MILLIS,
        }
    }
}

// #[cfg(test)]
// #[ctor::ctor]
// fn init_log() { env_logger::init(); }
