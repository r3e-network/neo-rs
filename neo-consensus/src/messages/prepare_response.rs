//! `PrepareResponse` message - sent by validators to acknowledge a proposal.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// `PrepareResponse` message sent by validators to acknowledge a `PrepareRequest`.
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
    /// Creates a new `PrepareResponse` message
    #[must_use]
    pub const fn new(
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
    #[must_use]
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::PrepareResponse
    }

    /// Serializes the message to bytes
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        self.preparation_hash.as_bytes().to_vec()
    }

    /// Deserializes the message body (excluding the common header) from bytes.
    pub fn deserialize_body(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        if data.len() < 32 {
            return Err(crate::ConsensusError::invalid_proposal(
                "PrepareResponse invalid hash length",
            ));
        }
        let preparation_hash = UInt256::from_bytes(&data[..32]).map_err(|_| {
            crate::ConsensusError::invalid_proposal("PrepareResponse invalid hash bytes")
        })?;
        Ok(Self {
            block_index,
            view_number,
            validator_index,
            preparation_hash,
        })
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
#[path = "../tests/messages/prepare_response.rs"]
mod tests;
