//! PrepareRequest message - sent by the primary to propose a block.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// PrepareRequest message sent by the primary (speaker) to propose a new block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareRequestMessage {
    /// Block index being proposed
    pub block_index: u32,
    /// View number
    pub view_number: u8,
    /// Validator index (should be primary)
    pub validator_index: u8,
    /// Proposed block timestamp
    pub timestamp: u64,
    /// Nonce for the block
    pub nonce: u64,
    /// Transaction hashes to include in the block
    pub transaction_hashes: Vec<UInt256>,
}

impl PrepareRequestMessage {
    /// Creates a new PrepareRequest message
    pub fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        timestamp: u64,
        nonce: u64,
        transaction_hashes: Vec<UInt256>,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            nonce,
            transaction_hashes,
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::PrepareRequest
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        // Transaction count as varint
        let tx_count = self.transaction_hashes.len();
        if tx_count < 0xFD {
            data.push(tx_count as u8);
        } else if tx_count <= 0xFFFF {
            data.push(0xFD);
            data.extend_from_slice(&(tx_count as u16).to_le_bytes());
        } else {
            data.push(0xFE);
            data.extend_from_slice(&(tx_count as u32).to_le_bytes());
        }
        // Transaction hashes
        for hash in &self.transaction_hashes {
            data.extend_from_slice(&hash.as_bytes());
        }
        data
    }

    /// Validates the message
    pub fn validate(&self, expected_primary: u8) -> ConsensusResult<()> {
        if self.validator_index != expected_primary {
            return Err(crate::ConsensusError::InvalidPrimary {
                expected: expected_primary,
                got: self.validator_index,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_request_new() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 1234567890, 42, vec![UInt256::zero()]);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 0);
        assert_eq!(msg.timestamp, 1234567890);
        assert_eq!(msg.nonce, 42);
        assert_eq!(msg.transaction_hashes.len(), 1);
    }

    #[test]
    fn test_prepare_request_serialize() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 1000, 1, vec![]);
        let data = msg.serialize();

        // 8 bytes timestamp + 8 bytes nonce + 1 byte tx count
        assert_eq!(data.len(), 17);
    }

    #[test]
    fn test_prepare_request_validate() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 1000, 1, vec![]);

        assert!(msg.validate(0).is_ok());
        assert!(msg.validate(1).is_err());
    }
}
