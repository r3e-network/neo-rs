//! PrepareResponse message - sent by validators to acknowledge a proposal.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// PrepareResponse message sent by validators to acknowledge a PrepareRequest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareResponseMessage {
    /// Block index
    pub block_index: u32,
    /// View number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Hash of the proposed block (for verification)
    pub preparation_hash: UInt256,
}

impl PrepareResponseMessage {
    /// Creates a new PrepareResponse message
    pub fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        preparation_hash: UInt256,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            preparation_hash,
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::PrepareResponse
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        self.preparation_hash.as_bytes().to_vec()
    }

    /// Validates the message
    pub fn validate(&self, expected_hash: &UInt256) -> ConsensusResult<()> {
        if &self.preparation_hash != expected_hash {
            return Err(crate::ConsensusError::HashMismatch {
                expected: *expected_hash,
                got: self.preparation_hash,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_response_new() {
        let hash = UInt256::zero();
        let msg = PrepareResponseMessage::new(100, 0, 1, hash);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.preparation_hash, hash);
    }

    #[test]
    fn test_prepare_response_serialize() {
        let msg = PrepareResponseMessage::new(100, 0, 1, UInt256::zero());
        let data = msg.serialize();

        assert_eq!(data.len(), 32); // UInt256 is 32 bytes
    }

    #[test]
    fn test_prepare_response_validate() {
        let hash = UInt256::zero();
        let msg = PrepareResponseMessage::new(100, 0, 1, hash);

        assert!(msg.validate(&hash).is_ok());

        let different_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
        assert!(msg.validate(&different_hash).is_err());
    }
}
