use std::vec::Vec;

use crate::message::{Message, MessageCommand, VersionPayload};

use super::state::HandshakeState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeRole {
    Outbound,
    Inbound,
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
    #[error("handshake: unexpected message {0:?}")]
    Unexpected(MessageCommand),

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
