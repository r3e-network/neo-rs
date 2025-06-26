//! Complete network message wrapper.
//!
//! This module provides the NetworkMessage type for Neo 3 protocol.
//! Neo 3 uses a 2-byte header (flags + command) with variable-length payload.

use super::{
    commands::{MessageCommand, MessageFlags},
    header::Neo3Message,
    protocol::ProtocolMessage,
};
use crate::{NetworkError, NetworkResult as Result};

/// Complete network message (Neo 3 format)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMessage {
    /// Neo 3 message structure
    pub neo3_message: Neo3Message,
    /// Parsed payload (for convenience)
    pub payload: ProtocolMessage,
    /// Compatibility header (for legacy code)
    pub header: HeaderCompat,
}

impl NetworkMessage {
    /// Creates a new network message (Neo 3 format)
    pub fn new(payload: ProtocolMessage) -> Self {
        let serialized_payload = payload.to_bytes().unwrap_or_default();
        let command = payload.command();
        let payload_length = serialized_payload.len() as u32;
        let neo3_message = Neo3Message::new(command, serialized_payload);
        let header = HeaderCompat {
            command,
            magic: 0x3554334e, // N3T5 TestNet magic
            length: payload_length,
            checksum: 0, // Simplified for compatibility
        };

        Self {
            neo3_message,
            payload,
            header,
        }
    }

    /// Serializes the message to bytes (Neo 3 format)
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(self.neo3_message.to_bytes())
    }

    /// Deserializes a message from bytes (Neo 3 format)
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Parse Neo3 message structure
        let neo3_message = Neo3Message::from_bytes(bytes)?;

        // Get the actual payload data (decompress if needed)
        let payload_bytes = neo3_message.get_payload()?;

        // Deserialize payload based on command
        let payload = ProtocolMessage::from_bytes(&neo3_message.command, &payload_bytes)?;

        // Create compatibility header
        let header = HeaderCompat {
            command: neo3_message.command,
            magic: 0x3554334e, // N3T5 TestNet magic
            length: payload_bytes.len() as u32,
            checksum: 0, // Simplified for compatibility
        };

        Ok(Self {
            neo3_message,
            payload,
            header,
        })
    }

    /// Gets the serialized size of the message
    pub fn serialized_size(&self) -> usize {
        self.neo3_message.serialized_size()
    }

    /// Gets the message command
    pub fn command(&self) -> MessageCommand {
        self.neo3_message.command
    }

    /// Gets the message flags
    pub fn flags(&self) -> MessageFlags {
        self.neo3_message.flags
    }
}

/// Compatibility header structure for legacy code
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderCompat {
    pub command: MessageCommand,
    pub magic: u32,
    pub length: u32,
    pub checksum: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neo3_network_message_serialization() {
        use super::super::commands::MessageCommand;
        let payload = ProtocolMessage::Ping { nonce: 12345 };

        let message = NetworkMessage::new(payload.clone());

        assert_eq!(message.command(), MessageCommand::Ping);
        assert_eq!(message.payload, payload);

        // Test serialization roundtrip
        let message_bytes = message.to_bytes().unwrap();
        let deserialized = NetworkMessage::from_bytes(&message_bytes).unwrap();

        assert_eq!(message.header.command, deserialized.header.command);
        assert_eq!(message.header.length, deserialized.header.length);
        assert_eq!(message.payload, deserialized.payload);
    }

    #[test]
    fn test_neo3_network_message_verack() {
        use super::super::commands::MessageCommand;
        let payload = ProtocolMessage::Verack;

        let message = NetworkMessage::new(payload);

        assert_eq!(message.header.command, MessageCommand::VERACK);
        assert_eq!(message.header.length, 0); // Verack has empty payload

        // Test serialization
        let bytes = message.to_bytes().unwrap();
        assert_eq!(bytes.len(), 24); // 24-byte header + 0-byte payload
    }
}
