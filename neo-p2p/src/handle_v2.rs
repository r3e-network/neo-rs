// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;

use neo_base::encoding::bin::*;
use neo_base::time::{unix_seconds_now, Tick, UnixTime};
use neo_core::payload::{Capability::*, *};
use neo_core::{block::Block, tx::Tx};
use tokio::sync::mpsc;

use crate::{PeerStage::*, *};

#[derive(Debug, Clone, errors::Error)]
pub enum HandleError {
    #[error("handle: identical nonce '{0}'")]
    IdenticalNonce(u32),

    #[error("handle: network mismatch '{0}'")]
    NetworkMismatch(u32),

    #[error("handle: no such net-handle in NetHandles")]
    NoSuchNetHandle,

    #[error("handle: peer already connected")]
    AlreadyConnected,

    #[error("handle: no such address in Discovery")]
    NoSuchAddress,

    #[error("handle: invalid message '{0}' with '{1}'")]
    InvalidMessage(&'static str, &'static str),

    #[error("hande: send message '{0}' with '{1}'")]
    SendError(&'static str, SendError),

    #[error("handle: '{0}' timeout")]
    Timeout(&'static str),
}

#[derive(Clone)]
pub struct MessageHandleV2 {
    net_handles: NetHandles,
    port: u16,
    config: P2pConfig,
}

impl MessageHandleV2 {
    #[inline]
    pub fn new(port: u16, config: P2pConfig, net_handles: NetHandles) -> Self {
        Self { net_handles, port, config }
    }

    #[inline]
    fn remove_net_handle(&self, peer: &SocketAddr) { self.net_handles.remove(peer); }

    #[inline]
    fn net_handle(&self, peer: &SocketAddr) -> Option<NetHandle> {
        self.net_handles.get(peer).map(|kv| kv.value().clone())
    }
}

// impl MessageHandle for MessageHandleV2 {
impl MessageHandleV2 {
    pub fn on_received(self, mut net_rx: mpsc::Receiver<NetMessage>, discovery: Discovery) {
        use NetEvent::*;
        while let Some(net) = net_rx.blocking_recv() {
            match &net.event {
                Message(message) => {
                    let mut buf = RefBuffer::from(message.as_bytes());
                    let _ = BinDecoder::decode_bin(&mut buf)
                        .map(|message| {
                            let _ = self.on_message(&discovery, &net.peer, message)
                                .map_err(|err| { self.on_handle_err(&discovery, &net, err); });
                        })
                        .map_err(|err| { self.on_handle_err(&discovery, &net, err); });
                }
                Connected | Accepted => { self.on_incoming(&discovery, &net.event, &net.peer); }
                Disconnected => { discovery.lock().unwrap().on_disconnected(&net.peer); }
                NotConnected => { discovery.lock().unwrap().on_failure(&net.peer); }
                Timeout => { self.on_handle_err(&discovery, &net, HandleError::Timeout("net_rx")); }
            }
        }
        log::warn!("`on_received` exited");
    }

    pub fn on_protocol_tick(&self, tick: Arc<Tick>, discovery: Discovery) {
        let min_peers = self.config.min_peers;
        let ping_millis = self.config.ping_interval.as_millis() as i64;

        let mut ping_at = UnixTime::now();
        while tick.wait() {
            let now = UnixTime::now();

            // tick_interval should less then ping_interval
            if (now - ping_at).num_milliseconds() >= ping_millis {
                // heartbeat
                self.on_heartbeat(&discovery);
                ping_at = now;
            }

            let stats = { discovery.lock().unwrap().discoveries() };
            let peers = stats.goods; // stats.connected ?
            let optimal = core::cmp::min(self.config.min_peers, stats.fan_out);
            if peers < min_peers || optimal > peers {
                // request more connection
                let n = core::cmp::min(
                    self.config.attempt_peers,
                    optimal - peers, /* overflow is ok */
                );
                discovery.lock().unwrap().request_remotes(n);
            }

            // TODO: move GetAddress routine to there
        }
        log::warn!("`on_protocol_tick` exited");
    }

    fn on_heartbeat(&self, discovery: &Discovery) {
        let ping_timeout = self.config.ping_timeout;
        let handshake_timeout = self.config.ping_timeout;

        let now = UnixTime::now();
        let should_ping = |x: &crate::Connected| {
            let ping = x.ping_timeout(now, ping_timeout);
            if !ping {
                x.ping_sent.store(now);
            }
            (x.addr, Timeouts { ping, handshake: x.handshake_timeout(now, handshake_timeout) })
        };

        //let dsc = discovery.lock().unwrap();
        let peers: Vec<_> =
            { discovery.lock().unwrap().connected_peers().map(should_ping).collect() };

        for (peer, timeouts) in peers.iter().filter(|(_, x)| x.ping || x.handshake) {
            self.on_handle_err(
                &discovery,
                &NetEvent::Timeout.with_peer(*peer),
                HandleError::Timeout(if timeouts.ping { "Ping" } else { "Handshake" }),
            );
        }

        self.broadcast_ping(&peers, &discovery);
    }

    fn broadcast_ping(&self, peers: &[(SocketAddr, Timeouts)], discovery: &Discovery) {
        let ping = P2pMessage::Ping(Ping {
            last_block_index: 0, // TODO: get last block index
            unix_seconds: unix_seconds_now() as u32,
            nonce: self.config.nonce,
        });

        use NetEvent::Disconnected;
        let message: Bytes = ping.to_bin_encoded().into();
        for (peer, _) in peers.iter().filter(|(_, x)| !x.ping && !x.handshake) {
            let _ = self
                .net_handle(peer)
                .ok_or(HandleError::NoSuchNetHandle)
                .and_then(|handle| {
                    handle.try_seed(message.clone())
                        .map(|()| { log::info!("send {:?} to {}", &ping, peer); })
                        .map_err(|err| HandleError::SendError("Ping", err))
                })
                .map_err(|err| { self.on_handle_err(discovery, &Disconnected.with_peer(*peer), err); });
        }
    }
}

impl MessageHandleV2 {
    fn on_incoming(&self, discovery: &Discovery, event: &NetEvent, peer: &SocketAddr) {
        let capabilities = if self.config.relay {
            vec![TcpServer { port: self.port }, FullNode { start_height: 0 }] // TODO: set start_height
        } else {
            vec![TcpServer { port: self.port }]
        };
        let version = P2pMessage::Version(Version {
            network: self.config.network,
            version: 0,
            unix_seconds: unix_seconds_now() as u32,
            nonce: self.config.nonce,
            user_agent: neo_base::VERSION.into(),
            capabilities,
        });

        let message = version.to_bin_encoded().into();
        let Some(handle) = self.net_handle(peer) else {
            return;
        };
        if let Err(err) = handle.try_seed(message) {
            log::error!("`on_incoming` try send `Version` err: {:?}", err);
            self.remove_net_handle(peer);
            return;
        }

        let stage = if matches!(event, NetEvent::Accepted) { Accepted } else { Connected };
        let mut disc = discovery.lock().unwrap();

        disc.on_incoming(peer.clone(), stage.as_u32() | VersionSent.as_u32());
        let stats = disc.discoveries();
        drop(disc);

        log::info!("`on_incoming` from {},{:?}, net-stats: {:?}", peer, stage, &stats);
    }

    fn on_handle_err<T: Error>(&self, discovery: &Discovery, net: &NetMessage, err: T) {
        if let NetEvent::Message(_data) = &net.event {
            //
        }

        self.remove_net_handle(&net.peer);
        {
            discovery.lock().unwrap().on_disconnected(&net.peer);
        }

        log::error!("handle NetEvent from {} err: {}", net.peer, &err);
    }

    fn on_message(
        &self,
        discovery: &Discovery,
        peer: &SocketAddr,
        message: P2pMessage,
    ) -> Result<(), HandleError> {
        use P2pMessage::*;
        if matches!(message, Version(_) | VersionAck) {
            return match message {
                Version(version) => self.on_version(discovery, peer, version),
                VersionAck => self.on_version_ack(discovery, &peer),
                _ => unreachable!("unexpected message"),
            };
        }

        let stages = { discovery.lock().unwrap().connected(&peer).map(|x| x.stages()) }
            .ok_or(HandleError::NoSuchAddress)?;
        if !VersionReceived.belongs(stages) || !VersionAckReceived.belongs(stages) {
            return Err(HandleError::InvalidMessage("Message", "handshake uncompleted"));
        }

        match message {
            GetAddress => self.on_get_address(discovery, &peer),
            Address(nodes) => self.on_address(discovery, &peer, nodes),
            Ping(ping) => self.on_ping(discovery, &peer, ping),
            Pong(pong) => self.on_pong(discovery, &peer, pong),
            GetHeaders(range) => self.on_get_headers(&peer, range),
            Headers(headers) => self.on_headers(&peer, headers),
            GetBlocks(range) => self.on_get_blocks(&peer, range),
            TxPool => self.on_tx_pool(&peer),
            Inventory(inventory) => self.on_inventory(&peer, inventory),
            GetData(inventory) => self.on_get_data(&peer, inventory),
            GetBlockByIndex(range) => self.on_get_block_by_index(&peer, range),
            NotFound(_inventory) => Ok(()), // just ignore
            Tx(tx) => self.on_tx(&peer, tx),
            Block(block) => self.on_block(&peer, block),
            Extensible(extensible) => self.on_extensible(&peer, extensible),
            Reject => Ok(()),                     // just ignore
            FilterLoad(_filter_load) => Ok(()),   // just ignore
            FilterAdd(_filter_add) => Ok(()),     // just ignore
            FilterClear => Ok(()),                // just ignore
            MerkleBlock(_merkle_block) => Ok(()), // just ignore
            Alert => Ok(()),                      // just ignore
            Version(_) => {
                unreachable!("unexpected Version message");
            }
            VersionAck => {
                unreachable!("unexpected VersionAck message");
            }
        }
    }

    fn on_version(
        &self,
        discovery: &Discovery,
        addr: &SocketAddr,
        version: Version,
    ) -> Result<(), HandleError> {
        let nonce = version.nonce;
        if nonce == self.config.nonce {
            version.port()
                .map(|port| SocketAddr::new(addr.ip(), port))
                .map(|service| {
                    { discovery.lock().unwrap().on_failure_always(service); }
                    log::error!("`on_failure_always` for {},{}, nonce {}", addr, &service, nonce);
                });
            return Err(HandleError::IdenticalNonce(nonce));
        }

        if version.network != self.config.network {
            return Err(HandleError::NetworkMismatch(version.network));
        }

        let start_height = version.start_height().unwrap_or(0);
        let peer = TcpPeer::new(addr.clone(), version);
        let Some(service) = peer.service_addr() else {
            // must have service address
            return Err(HandleError::InvalidMessage("Version", "no service address"));
        };

        let message = P2pMessage::VersionAck.to_bin_encoded();
        let handle = self.net_handle(addr).ok_or(HandleError::NoSuchNetHandle)?;
        handle.try_seed(message.into()).map_err(|err| HandleError::SendError("VersionAck", err))?;

        handle.states.set_last_block_index(start_height);
        drop(handle);

        let mut discovery = discovery.lock().unwrap();
        if discovery.has_peer(&service, peer.version.nonce) {
            return Err(HandleError::AlreadyConnected);
        }

        let conn = discovery.connected(&addr).ok_or(HandleError::NoSuchAddress)?;
        conn.add_stages(VersionReceived.as_u32() | VersionAckSent.as_u32());
        discovery.on_good(peer);

        Ok(())
    }

    fn on_version_ack(&self, discovery: &Discovery, peer: &SocketAddr) -> Result<(), HandleError> {
        let discovery = discovery.lock().unwrap();
        let conn = discovery.connected(peer).ok_or(HandleError::NoSuchAddress)?;
        if !VersionSent.belongs(conn.stages()) {
            return Err(HandleError::InvalidMessage("VersionAck", "no Version has received"));
        }

        conn.add_stages(VersionAckReceived.as_u32());
        Ok(())
    }

    fn on_get_address(&self, discovery: &Discovery, peer: &SocketAddr) -> Result<(), HandleError> {
        let now = unix_seconds_now() as u32;
        let nodes = {
            discovery
                .lock()
                .unwrap()
                .good_peers()
                .map(|x| NodeAddr {
                    unix_seconds: now,
                    ip: x.addr.ip().into(),
                    capabilities: x.version.capabilities.clone(),
                })
                .take(MAX_COUNT_TO_SEND)
                .collect()
        };

        let message = P2pMessage::Address(NodeList { nodes });
        let message = message.to_bin_encoded(); // for less critical section

        let handle = self.net_handle(peer)
            .ok_or(HandleError::NoSuchNetHandle)?;

        handle.try_seed(message.into())
            .map_err(|err| HandleError::SendError("Address", err))?;

        handle.states.on_sent_get_address();
        Ok(())
    }

    fn on_address(
        &self,
        discovery: &Discovery,
        peer: &SocketAddr,
        nodes: NodeList,
    ) -> Result<(), HandleError> {
        let handle = self.net_handle(peer)
            .ok_or(HandleError::NoSuchNetHandle)?;
        if !handle.states.on_recv_address() {
            return Err(HandleError::InvalidMessage("Address", "no GetAddress for this node"));
        }

        let nodes: Vec<_> = nodes.nodes.iter().filter_map(|node| node.service_addr()).collect();
        discovery.lock().unwrap().back_fill(&nodes);
        Ok(())
    }

    fn on_ping(
        &self,
        discovery: &Discovery,
        peer: &SocketAddr,
        ping: Ping,
    ) -> Result<(), HandleError> {
        {
            let now = UnixTime::now();
            discovery
                .lock()
                .unwrap()
                .connected(peer)
                .map(|x| x.ping_recv.store(now))
                .ok_or(HandleError::NoSuchAddress)?;
        }
        log::info!("recv {:?} from {}", &ping, peer);

        let pong = P2pMessage::Pong(Pong {
            last_block_index: 0, // TODO: get last lock index
            unix_seconds: unix_seconds_now() as u32,
            nonce: self.config.nonce,
        });

        let handle = self.net_handle(peer)
            .ok_or(HandleError::NoSuchNetHandle)?;

        handle.try_seed(pong.to_bin_encoded().into())
            .map_err(|err| HandleError::SendError("Pong", err))?;

        handle.states.set_last_block_index(ping.last_block_index);
        Ok(())
    }

    fn on_pong(
        &self,
        discovery: &Discovery,
        peer: &SocketAddr,
        pong: Pong,
    ) -> Result<(), HandleError> {
        {
            let now = UnixTime::now();
            discovery
                .lock()
                .unwrap()
                .connected(peer)
                .map(|x| x.pong_recv.store(now))
                .ok_or(HandleError::NoSuchAddress)?;
        }

        let handle = self.net_handle(peer)
            .ok_or(HandleError::NoSuchNetHandle)?;

        handle.states.set_last_block_index(pong.last_block_index);
        Ok(())
    }

    fn on_get_headers(&self,
                      _peer: &SocketAddr,
                      _range: BlockIndexRange,
    ) -> Result<(), HandleError> {
        Ok(())
    }

    fn on_headers(&self, _peer: &SocketAddr, _headers: Headers) -> Result<(), HandleError> {
        Ok(())
    }

    fn on_get_blocks(&self, _peer: &SocketAddr, _range: BlockHashRange) -> Result<(), HandleError> {
        Ok(())
    }

    fn on_tx_pool(&self, _peer: &SocketAddr) -> Result<(), HandleError> { Ok(()) }

    fn on_tx(&self, _peer: &SocketAddr, _tx: Tx) -> Result<(), HandleError> { Ok(()) }

    fn on_block(&self, _peer: &SocketAddr, _block: Block) -> Result<(), HandleError> { Ok(()) }

    fn on_extensible(
        &self,
        _peer: &SocketAddr,
        _extensible: Extensible,
    ) -> Result<(), HandleError> {
        Ok(())
    }

    fn on_inventory(&self, _peer: &SocketAddr, inv: Inventory) -> Result<(), HandleError> {
        match inv {
            Inventory::Tx(_hashes) => {}
            Inventory::Block(_hashes) => {}
            Inventory::Extensible(_hashes) => {}
        }
        Ok(())
    }

    fn on_get_data(&self, _peer: &SocketAddr, _inv: Inventory) -> Result<(), HandleError> { Ok(()) }

    fn on_get_block_by_index(
        &self,
        _peer: &SocketAddr,
        _range: BlockIndexRange,
    ) -> Result<(), HandleError> {
        Ok(())
    }
}
