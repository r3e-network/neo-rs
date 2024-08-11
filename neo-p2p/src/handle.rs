// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::net::SocketAddr;

use tokio::sync::mpsc;

use neo_base::{encoding::bin::*};
use neo_core::payload::{Capability::*, NodeList, P2pMessage, Version};
use crate::*;


#[derive(Debug, Clone)]
pub struct HandleSettings {
    pub network: u32,
    pub nonce: u32,
    pub port: u16,
    pub relay: bool,
}


// #[derive(Debug, Clone, errors::Error)]
// pub struct HandleError {}


pub struct MessageHandle {
    net_handles: SharedHandles,
    net_rx: mpsc::Receiver<NetMessage>,
    settings: HandleSettings,
}


impl MessageHandle {
    #[inline]
    pub fn new(settings: HandleSettings, net_handles: SharedHandles, net_rx: mpsc::Receiver<NetMessage>) -> Self {
        Self { net_handles, net_rx, settings }
    }

    pub fn on_received(mut self, discovery: SharedDiscovery) {
        use NetEvent::*;
        while let Some(net) = self.net_rx.blocking_recv() {
            let peer = net.peer;
            match &net.event {
                Received(message) => {
                    let mut buf = RefBuffer::from(message.as_bytes());
                    match BinDecoder::decode_bin(&mut buf) {
                        Ok(message) => { self.on_message(&discovery, peer, message); }
                        Err(err) => { self.on_decode_err(&net, err); }
                    }
                }
                Connected | Accepted => { self.on_incoming(&discovery, &net.event, &peer); }
                Disconnected | NotConnected => { self.on_outgoing(); }
            }
        }
        // TODO: log exit action
    }

    fn on_incoming(&self, discovery: &SharedDiscovery, event: &NetEvent, peer: &SocketAddr) {
        let port = self.settings.port;
        let capabilities = if self.settings.relay {
            vec![TcpServer { port }, FullNode { start_height: 0 }] // TODO: set start_height
        } else {
            vec![TcpServer { port }]
        };
        let version = P2pMessage::Version(Version {
            network: self.settings.network,
            version: 0,
            unix_seconds: local_now().timestamp() as u32,
            nonce: self.settings.nonce,
            user_agent: neo_base::VERSION.into(),
            capabilities,
        });

        let message = version.to_bin_encoded().into();
        let Some(handle) = self.net_handle(peer) else { return; };
        if let Err(_err) = handle.try_seed(message) { // TODO: log error
            self.remove_net_handle(peer);
            return;
        }

        use PeerStage::*;
        let stage = if matches!(event, NetEvent::Accepted) { Accepted } else { Connected };
        discovery.lock().unwrap().on_incoming(peer.clone(), stage.as_u32() | VersionSent.as_u32());
    }

    fn on_outgoing(&self) {
        //
    }

    fn on_decode_err(&self, _message: &NetMessage, _err: BinDecodeError) {
        // TODO
    }

    fn on_message(&self, discovery: &SharedDiscovery, peer: SocketAddr, message: P2pMessage) {
        use P2pMessage::*;
        match message {
            Version(version) => { self.on_version(discovery, peer, version); }
            VersionAck => { self.on_version_ack(discovery, &peer); }
            GetAddress => { self.on_get_address(); } // TODO
            Address(nodes) => { self.on_address(nodes); }
            Ping(_ping) => {}
            Pong(_pong) => {}
            GetHeaders(_index_range) => {}
            Headers(_headers) => {}
            GetBlocks(_hash_range) => {}
            TxPool => {}
            Inventory(_inventory) => {}
            GetData(_inventory) => {}
            GetBlockByIndex(_index_range) => {}
            NotFound => {}
            Tx(_tx) => {}
            Block(_block) => {}
            Extensible(_extensible) => {}
            Reject => {}
            FilterLoad(_filter_load) => {}
            FilterAdd(_filter_add) => {}
            FilterClear => {}
            MerkleBlock(_merkle_block) => {}
            Alert => {}
        }
    }

    fn on_version(&self, discovery: &SharedDiscovery, addr: SocketAddr, version: Version) {
        let peer = TcpPeer::new(addr, version);
        let Some(handle) = self.net_handle(&addr) else { return; };
        let Some(service) = peer.service_addr() else { // must hava service address
            self.remove_net_handle(&addr);
            return;
        };

        let message = P2pMessage::VersionAck.to_bin_encoded();
        if let Err(_err) = handle.try_seed(message.into()) {
            // TODO: log error
            self.remove_net_handle(&addr);
            return;
        }

        {
            let mut dsc = discovery.lock().unwrap();
            dsc.on_good(service, peer);
            if let Some(conn) = dsc.get_connected(&addr) {
                conn.add_stage(PeerStage::VersionReceived.as_u32() | PeerStage::VersionAckSent.as_u32());
            }
        }
    }

    fn on_version_ack(&self, discovery: &SharedDiscovery, addr: &SocketAddr) {
        let dsc = discovery.lock().unwrap();
        if let Some(connected) = dsc.get_connected(addr) {
            connected.add_stage(PeerStage::VersionAckReceived.as_u32());
        } else {
            self.remove_net_handle(addr);
        }
    }

    fn on_get_address(&self) {
        //
    }

    fn on_address(&self, _nodes: NodeList) {
        //
    }

    #[inline]
    fn remove_net_handle(&self, peer: &SocketAddr) {
        self.net_handles.lock()
            .unwrap()
            .remove(peer);
    }

    #[inline]
    fn net_handle(&self, peer: &SocketAddr) -> Option<NetHandle> {
        self.net_handles.lock()
            .unwrap()
            .get(peer)
            .cloned()
    }
}


#[cfg(test)]
mod test {
    #[test]
    fn test_message_handle() {
        //
    }
}