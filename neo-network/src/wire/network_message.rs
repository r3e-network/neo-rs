//! Top-level [`NetworkMessage`] envelope.
//!
//! Combines [`MessageHeader`] (command + flags) and a typed
//! [`ProtocolMessage`] payload. This is the type that the framed
//! codec produces and that the peer event loop dispatches on.

use super::error::WireResult;
use super::message::Message;
use super::protocol_message::ProtocolMessage;
use crate::{MessageCommand, MessageFlags};

/// Header metadata attached to every P2P message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageHeader {
    /// Command opcode that identifies the payload type.
    pub command: MessageCommand,
}

impl MessageHeader {
    /// Constructs a new header from a command.
    pub fn new(command: MessageCommand) -> Self {
        Self { command }
    }
}

/// Fully-decoded network message: header + flags + typed payload.
#[derive(Debug, Clone)]
pub struct NetworkMessage {
    /// Header metadata (command).
    pub header: MessageHeader,
    /// Message flags (e.g. compression bit).
    pub flags: MessageFlags,
    /// Strongly-typed payload.
    pub payload: ProtocolMessage,
}

impl NetworkMessage {
    /// Constructs a new network message from a typed payload. The
    /// command is derived from the payload variant.
    pub fn new(payload: ProtocolMessage) -> Self {
        let command = payload.command();
        Self {
            header: MessageHeader { command },
            flags: MessageFlags::NONE,
            payload,
        }
    }

    /// Constructs a network message with explicit flags.
    pub fn with_flags(payload: ProtocolMessage, flags: MessageFlags) -> Self {
        let command = payload.command();
        Self {
            header: MessageHeader { command },
            flags,
            payload,
        }
    }

    /// Convenience accessor for the command associated with this message.
    pub fn command(&self) -> MessageCommand {
        self.header.command
    }

    /// Encodes the message into its on-the-wire byte sequence.
    ///
    /// `allow_compression` mirrors the C# `Message.ToArray(bool)` flag:
    /// when `false`, the payload is always emitted uncompressed even
    /// if the heuristics would normally trigger compression.
    pub fn to_bytes(&self, allow_compression: bool) -> WireResult<Vec<u8>> {
        let payload_bytes = self.payload.serialize_payload()?;
        let enable_compression = allow_compression && self.payload.allows_compression();
        let message =
            Message::from_payload_bytes(self.header.command, payload_bytes, enable_compression)?;
        message.to_bytes()
    }

    /// Decodes a network message that was previously produced by
    /// [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> WireResult<Self> {
        let message = Message::from_bytes(bytes)?;
        let payload = ProtocolMessage::deserialize_payload(message.command, &message.payload_raw)?;
        Ok(Self {
            header: MessageHeader {
                command: message.command,
            },
            flags: message.flags,
            payload,
        })
    }
}

impl From<ProtocolMessage> for NetworkMessage {
    fn from(payload: ProtocolMessage) -> Self {
        Self::new(payload)
    }
}

#[cfg(test)]
#[path = "../tests/wire/network_message.rs"]
mod tests;
