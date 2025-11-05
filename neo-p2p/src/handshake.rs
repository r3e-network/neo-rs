use std::time::SystemTime;

use rand::Rng;

use crate::message::{Endpoint, Message, VersionPayload};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeRole {
    Outbound,
    Inbound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HandshakeState {
    AwaitingRemoteVersion,
    AwaitingRemoteVerack,
    Completed,
}

#[derive(Debug, Clone)]
pub struct HandshakeMachine {
    role: HandshakeRole,
    state: HandshakeState,
    local_version: VersionPayload,
    remote_version: Option<VersionPayload>,
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("handshake: unexpected message {0}")]
    Unexpected(&'static str),

    #[error("handshake: already completed")]
    AlreadyCompleted,
}

impl HandshakeMachine {
    pub fn new(role: HandshakeRole, local_version: VersionPayload) -> Self {
        Self {
            role,
            state: HandshakeState::AwaitingRemoteVersion,
            local_version,
            remote_version: None,
        }
    }

    pub fn start(&mut self) -> Option<Message> {
        match self.role {
            HandshakeRole::Outbound => Some(Message::Version(self.local_version.clone())),
            HandshakeRole::Inbound => None,
        }
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.state, HandshakeState::Completed)
    }

    pub fn remote_version(&self) -> Option<&VersionPayload> {
        self.remote_version.as_ref()
    }

    pub fn on_message(&mut self, message: &Message) -> Result<Vec<Message>, HandshakeError> {
        if self.is_complete() {
            return Err(HandshakeError::AlreadyCompleted);
        }

        match (&self.state, message) {
            (HandshakeState::AwaitingRemoteVersion, Message::Version(payload)) => {
                self.remote_version = Some(payload.clone());
                self.state = HandshakeState::AwaitingRemoteVerack;
                let mut replies = Vec::new();
                if matches!(self.role, HandshakeRole::Inbound) {
                    replies.push(Message::Version(self.local_version.clone()));
                }
                replies.push(Message::Verack);
                Ok(replies)
            }
            (HandshakeState::AwaitingRemoteVerack, Message::Verack) => {
                self.state = HandshakeState::Completed;
                Ok(Vec::new())
            }
            _ => Err(HandshakeError::Unexpected(message.command())),
        }
    }
}

/// Utility for constructing a standard version payload for local node.
pub fn build_version_payload(
    protocol: u32,
    services: u64,
    receiver: Endpoint,
    sender: Endpoint,
    start_height: u32,
) -> VersionPayload {
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let nonce = rand::thread_rng().gen::<u64>();
    VersionPayload::new(
        protocol,
        services,
        timestamp,
        receiver,
        sender,
        nonce,
        format!("/neo-rs:{}", env!("CARGO_PKG_VERSION")),
        start_height,
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn sample_endpoint(port: u16) -> Endpoint {
        Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
    }

    fn sample_version(port: u16) -> VersionPayload {
        build_version_payload(
            0x03,
            1,
            sample_endpoint(port),
            sample_endpoint(port + 1),
            100,
        )
    }

    #[test]
    fn outbound_handshake_flow() {
        let mut machine = HandshakeMachine::new(HandshakeRole::Outbound, sample_version(2000));
        let initial = machine.start().unwrap();
        assert!(matches!(initial, Message::Version(_)));

        let replies = machine
            .on_message(&Message::Version(sample_version(3000)))
            .unwrap();
        assert_eq!(replies, vec![Message::Verack]);
        assert!(!machine.is_complete());

        let replies = machine.on_message(&Message::Verack).unwrap();
        assert!(replies.is_empty());
        assert!(machine.is_complete());
    }

    #[test]
    fn inbound_handshake_flow() {
        let mut machine = HandshakeMachine::new(HandshakeRole::Inbound, sample_version(4000));
        assert!(machine.start().is_none());

        let replies = machine
            .on_message(&Message::Version(sample_version(5000)))
            .unwrap();
        assert_eq!(replies.len(), 2);
        assert!(matches!(replies[0], Message::Version(_)));
        assert_eq!(replies[1], Message::Verack);
        assert!(!machine.is_complete());

        machine.on_message(&Message::Verack).unwrap();
        assert!(machine.is_complete());
    }
}
