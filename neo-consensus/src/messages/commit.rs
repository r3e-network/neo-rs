//! Commit message - sent when a validator is ready to commit the block.

use crate::messages::{parse_consensus_message_header, serialize_consensus_message_header};
use crate::{ConsensusMessageType, ConsensusResult};
use serde::{Deserialize, Serialize};

/// Commit message sent when a validator has received enough PrepareResponses
/// and is ready to commit the block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitMessage {
    /// Block index
    pub block_index: u32,
    /// View number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Signature over the block hash
    pub signature: Vec<u8>,
}

impl CommitMessage {
    /// Creates a new Commit message
    pub fn new(block_index: u32, view_number: u8, validator_index: u8, signature: Vec<u8>) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            signature,
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::Commit
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = serialize_consensus_message_header(
            ConsensusMessageType::Commit,
            self.block_index,
            self.validator_index,
            self.view_number,
        );
        out.extend_from_slice(&self.signature);
        out
    }

    /// Deserializes a Commit message from bytes.
    pub fn deserialize(data: &[u8]) -> ConsensusResult<Self> {
        let (msg_type, block_index, validator_index, view_number, body) =
            parse_consensus_message_header(data)?;
        if msg_type != ConsensusMessageType::Commit {
            return Err(crate::ConsensusError::invalid_proposal(
                "invalid Commit message type",
            ));
        }
        if body.len() != 64 {
            return Err(crate::ConsensusError::InvalidSignatureLength {
                expected: 64,
                got: body.len(),
            });
        }
        Ok(Self {
            block_index,
            view_number,
            validator_index,
            signature: body.to_vec(),
        })
    }

    /// Validates the signature length
    pub fn validate(&self) -> ConsensusResult<()> {
        // ECDSA signature should be 64 bytes (r + s)
        if self.signature.len() != 64 {
            return Err(crate::ConsensusError::InvalidSignatureLength {
                expected: 64,
                got: self.signature.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_new() {
        let sig = vec![0u8; 64];
        let msg = CommitMessage::new(100, 0, 1, sig.clone());

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.signature, sig);
    }

    #[test]
    fn test_commit_serialize() {
        let sig = vec![0u8; 64];
        let msg = CommitMessage::new(100, 0, 1, sig);
        let data = msg.serialize();

        assert_eq!(data.len(), 71);
    }

    #[test]
    fn test_commit_validate() {
        let valid_sig = vec![0u8; 64];
        let msg = CommitMessage::new(100, 0, 1, valid_sig);
        assert!(msg.validate().is_ok());

        let invalid_sig = vec![0u8; 32];
        let msg = CommitMessage::new(100, 0, 1, invalid_sig);
        assert!(msg.validate().is_err());
    }
}
