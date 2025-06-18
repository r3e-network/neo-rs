//! Complete network message wrapper.
//!
//! This module provides the NetworkMessage type exactly matching C# Neo NetworkMessage.

use crate::{Error, Result};
use super::{header::MessageHeader, protocol::ProtocolMessage};

/// Complete network message
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkMessage {
    /// Message header
    pub header: MessageHeader,
    /// Message payload
    pub payload: ProtocolMessage,
}

impl NetworkMessage {
    /// Creates a new network message
    pub fn new(magic: u32, payload: ProtocolMessage) -> Self {
        let serialized_payload = payload.to_bytes().unwrap_or_default();
        let command = payload.command();
        let header = MessageHeader::new(magic, command, &serialized_payload);
        
        Self { header, payload }
    }
    
    /// Serializes the message to bytes (Neo N3 compatible)
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let payload_bytes = self.payload.to_bytes()?;
        let mut bytes = Vec::with_capacity(24 + payload_bytes.len());
        
        // Header (24 bytes)
        bytes.extend_from_slice(&self.header.to_bytes());
        
        // Payload
        bytes.extend_from_slice(&payload_bytes);
        
        Ok(bytes)
    }
    
    /// Deserializes a message from bytes (Neo N3 compatible)
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 24 {
            return Err(Error::Protocol("Message too short".to_string()));
        }
        
        // Parse header
        let header = MessageHeader::from_bytes(&bytes[..24])?;
        
        // Validate length
        if bytes.len() < 24 + header.length as usize {
            return Err(Error::Protocol("Incomplete message".to_string()));
        }
        
        let payload_bytes = &bytes[24..24 + header.length as usize];
        
        // Validate header
        header.validate(payload_bytes)?;
        
        // Deserialize payload based on command
        let payload = ProtocolMessage::from_bytes(&header.command, payload_bytes)?;
        
        Ok(Self { header, payload })
    }
    
    /// Gets the serialized size of the message
    pub fn serialized_size(&self) -> usize {
        24 + self.header.length as usize // Header (24 bytes) + payload
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::messages::commands::MessageCommand;

    #[test]
    fn test_network_message_serialization() {
        let magic = 0x334f454e; // Neo N3 MainNet magic
        let payload = ProtocolMessage::Ping { nonce: 12345 };
        
        let message = NetworkMessage::new(magic, payload.clone());
        
        assert_eq!(message.header.magic, magic);
        assert_eq!(message.header.command, MessageCommand::PING);
        assert_eq!(message.payload, payload);
        
        // Test serialization roundtrip
        let message_bytes = message.to_bytes().unwrap();
        let deserialized = NetworkMessage::from_bytes(&message_bytes).unwrap();
        
        assert_eq!(message.header.magic, deserialized.header.magic);
        assert_eq!(message.header.command.to_string(), deserialized.header.command.to_string());
        assert_eq!(message.payload, deserialized.payload);
    }

    #[test]
    fn test_network_message_verack() {
        let magic = 0x334f454e;
        let payload = ProtocolMessage::Verack;
        
        let message = NetworkMessage::new(magic, payload);
        
        assert_eq!(message.header.command, MessageCommand::VERACK);
        assert_eq!(message.header.length, 0); // Verack has empty payload
        
        // Test serialization
        let bytes = message.to_bytes().unwrap();
        assert_eq!(bytes.len(), 24); // Only header, no payload
    }
}
