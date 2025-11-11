use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::handshake::{HandshakeError, HandshakeMachine, HandshakeRole};
use crate::message::{Endpoint, Message, VersionPayload};

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: Uuid,
    pub endpoint: Endpoint,
    handshake: HandshakeMachine,
    last_message: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerEvent {
    Messages(Vec<Message>),
    HandshakeCompleted,
    None,
}

impl Peer {
    pub fn outbound(endpoint: Endpoint, local_version: VersionPayload) -> Self {
        Self::new(endpoint, HandshakeRole::Outbound, local_version)
    }

    pub fn inbound(endpoint: Endpoint, local_version: VersionPayload) -> Self {
        Self::new(endpoint, HandshakeRole::Inbound, local_version)
    }

    fn new(endpoint: Endpoint, role: HandshakeRole, local_version: VersionPayload) -> Self {
        Self {
            id: Uuid::new_v4(),
            endpoint,
            handshake: HandshakeMachine::new(role, local_version),
            last_message: None,
        }
    }

    pub fn bootstrap(&mut self) -> Vec<Message> {
        self.handshake.start().into_iter().collect::<Vec<_>>()
    }

    pub fn on_message(&mut self, message: Message) -> Result<PeerEvent, HandshakeError> {
        self.last_message = Some(Instant::now());
        let replies = self.handshake.on_message(&message)?;
        if self.handshake.is_complete() {
            if replies.is_empty() {
                Ok(PeerEvent::HandshakeCompleted)
            } else {
                Ok(PeerEvent::Messages(replies))
            }
        } else if replies.is_empty() {
            Ok(PeerEvent::None)
        } else {
            Ok(PeerEvent::Messages(replies))
        }
    }

    pub fn is_ready(&self) -> bool {
        self.handshake.is_complete()
    }

    pub fn last_message_since(&self) -> Option<Duration> {
        self.last_message.map(|inst| inst.elapsed())
    }

    pub fn remote_version(&self) -> Option<&VersionPayload> {
        self.handshake.remote_version()
    }

    pub fn compression_allowed(&self) -> bool {
        if let Some(remote) = self.remote_version() {
            remote.allows_compression() && self.handshake.local_version().allows_compression()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{handshake::build_version_payload, message::Capability};
    use std::net::{IpAddr, Ipv4Addr};

    fn endpoint(port: u16) -> Endpoint {
        Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    #[test]
    fn peer_outbound_sequence() {
        let local_version = build_version_payload(
            860_833_102,
            0x03,
            "/test-peer".to_string(),
            vec![
                Capability::tcp_server(2000),
                Capability::full_node(10),
                Capability::ArchivalNode,
            ],
        );
        let mut peer = Peer::outbound(endpoint(2002), local_version.clone());
        let bootstrap = peer.bootstrap();
        assert_eq!(bootstrap.len(), 1);
        assert!(matches!(&bootstrap[0], Message::Version(_)));

        let mut remote_version = local_version.clone();
        remote_version.nonce = local_version.nonce.wrapping_add(1);
        peer.on_message(Message::Version(remote_version)).unwrap();
        let event = peer.on_message(Message::Verack).unwrap();
        assert!(matches!(event, PeerEvent::HandshakeCompleted));
        assert!(peer.is_ready());
    }
}
