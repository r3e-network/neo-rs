//! Top-level [`NetworkMessage`] envelope.
//!
//! Combines [`MessageHeader`] (command + flags) and a typed
//! [`ProtocolMessage`] payload. This is the type that the framed
//! codec produces and that the peer event loop dispatches on.

use super::error::WireResult;
use super::message::Message;
use super::protocol_message::ProtocolMessage;
use neo_p2p::{MessageCommand, MessageFlags};

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
mod tests {
    use super::*;
    use neo_p2p::payloads::PingPayload;

    #[test]
    fn network_message_round_trip_ping() {
        let msg = NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create(11)));
        let bytes = msg.to_bytes(true).expect("encode");
        let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
        assert_eq!(decoded.command(), MessageCommand::Ping);
        match decoded.payload {
            ProtocolMessage::Ping(p) => assert_eq!(p.last_block_index, 11),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn network_message_round_trip_verack() {
        let msg = NetworkMessage::new(ProtocolMessage::Verack);
        let bytes = msg.to_bytes(false).expect("encode");
        let decoded = NetworkMessage::from_bytes(&bytes).expect("decode");
        assert!(matches!(decoded.payload, ProtocolMessage::Verack));
    }
}
