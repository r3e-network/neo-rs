//! Message header structure.
//!
//! This module provides message header functionality exactly matching C# Neo MessageHeader.

use crate::{Error, Result};
use super::{commands::MessageCommand, MAX_MESSAGE_SIZE};
use serde::{Deserialize, Serialize};

/// Network message header (Neo N3 compatible format)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageHeader {
    /// Network magic number (4 bytes)
    pub magic: u32,
    /// Command string (12 bytes, zero-padded)
    pub command: MessageCommand,
    /// Payload length (4 bytes)
    pub length: u32,
    /// Payload checksum (4 bytes, SHA256(SHA256(payload)))
    pub checksum: u32,
}

impl MessageHeader {
    /// Creates a new message header
    pub fn new(magic: u32, command: MessageCommand, payload: &[u8]) -> Self {
        let length = payload.len() as u32;
        let checksum = Self::calculate_checksum(payload);
        
        Self {
            magic,
            command,
            length,
            checksum,
        }
    }
    
    /// Calculates checksum for payload (SHA256(SHA256(payload)))
    fn calculate_checksum(payload: &[u8]) -> u32 {
        use sha2::{Digest, Sha256};
        let first_hash = Sha256::digest(payload);
        let second_hash = Sha256::digest(&first_hash);
        u32::from_le_bytes([second_hash[0], second_hash[1], second_hash[2], second_hash[3]])
    }
    
    /// Validates the header
    pub fn validate(&self, payload: &[u8]) -> Result<()> {
        if self.length as usize != payload.len() {
            return Err(Error::Protocol(format!(
                "Invalid payload length: expected {}, got {}", 
                self.length, payload.len()
            )));
        }
        
        if self.length as usize > MAX_MESSAGE_SIZE {
            return Err(Error::Protocol(format!(
                "Message too large: {} bytes (max: {})", 
                self.length, MAX_MESSAGE_SIZE
            )));
        }
        
        let expected_checksum = Self::calculate_checksum(payload);
        if self.checksum != expected_checksum {
            return Err(Error::Protocol(format!(
                "Invalid checksum: expected 0x{:08x}, got 0x{:08x}", 
                expected_checksum, self.checksum
            )));
        }
        
        Ok(())
    }

    /// Serializes header to bytes (Neo N3 format)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(24);
        bytes.extend_from_slice(&self.magic.to_le_bytes());
        bytes.extend_from_slice(self.command.as_bytes());
        bytes.extend_from_slice(&self.length.to_le_bytes());
        bytes.extend_from_slice(&self.checksum.to_le_bytes());
        bytes
    }

    /// Deserializes header from bytes (Neo N3 format)
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 24 {
            return Err(Error::Protocol("Header too short".to_string()));
        }

        let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        
        let mut command_bytes = [0u8; 12];
        command_bytes.copy_from_slice(&bytes[4..16]);
        let command = MessageCommand::new(
            std::str::from_utf8(&command_bytes)
                .unwrap_or("invalid")
                .trim_end_matches('\0')
        );
        
        let length = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let checksum = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);

        Ok(Self {
            magic,
            command,
            length,
            checksum,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_header() {
        let magic = 0x334f454e; // Neo N3 MainNet magic
        let command = MessageCommand::new("version");
        let payload = b"test payload";
        
        let header = MessageHeader::new(magic, command.clone(), payload);
        
        assert_eq!(header.magic, magic);
        assert_eq!(header.command, command);
        assert_eq!(header.length, payload.len() as u32);
        
        // Test validation
        assert!(header.validate(payload).is_ok());
        
        // Test serialization roundtrip
        let header_bytes = header.to_bytes();
        assert_eq!(header_bytes.len(), 24);
        
        let deserialized = MessageHeader::from_bytes(&header_bytes).unwrap();
        assert_eq!(header.magic, deserialized.magic);
        assert_eq!(header.command.to_string(), deserialized.command.to_string());
        assert_eq!(header.length, deserialized.length);
        assert_eq!(header.checksum, deserialized.checksum);
    }
}
