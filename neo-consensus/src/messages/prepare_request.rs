//! PrepareRequest message - sent by the primary to propose a block.

use crate::messages::serialize_consensus_message_header;
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
    /// Block version (must be 0 on Neo N3)
    pub version: u32,
    /// Previous block hash
    pub prev_hash: UInt256,
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
        version: u32,
        prev_hash: UInt256,
        timestamp: u64,
        nonce: u64,
        transaction_hashes: Vec<UInt256>,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            version,
            prev_hash,
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
    ///
    /// Matches C# `DBFTPlugin.Messages.PrepareRequest`.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = serialize_consensus_message_header(
            ConsensusMessageType::PrepareRequest,
            self.block_index,
            self.validator_index,
            self.view_number,
        );
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.prev_hash.as_bytes());
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out.extend_from_slice(&self.nonce.to_le_bytes());

        let mut writer = neo_io::BinaryWriter::new();
        let _ = writer.write_var_int(self.transaction_hashes.len() as u64);
        for hash in &self.transaction_hashes {
            let _ = writer.write_bytes(&hash.as_bytes());
        }
        out.extend_from_slice(&writer.into_bytes());
        out
    }

    /// Deserializes a PrepareRequest message from bytes.
    pub fn deserialize(data: &[u8]) -> ConsensusResult<Self> {
        let mut reader = neo_io::MemoryReader::new(data);
        Self::deserialize_from_reader(&mut reader)
    }

    pub fn deserialize_from_reader(reader: &mut neo_io::MemoryReader) -> ConsensusResult<Self> {
        let msg_type = reader
            .read_byte()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest header missing"))?;
        if ConsensusMessageType::from_byte(msg_type) != Some(ConsensusMessageType::PrepareRequest) {
            return Err(crate::ConsensusError::invalid_proposal(
                "invalid PrepareRequest message type",
            ));
        }

        let block_index = reader
            .read_u32()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest block index missing"))?;
        let validator_index = reader
            .read_byte()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest validator index missing"))?;
        let view_number = reader
            .read_byte()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest view number missing"))?;

        let version = reader
            .read_u32()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest version missing"))?;

        let prev_hash_bytes = reader
            .read_memory(32)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest prev_hash missing"))?;
        let prev_hash = UInt256::from_bytes(prev_hash_bytes).map_err(|_| {
            crate::ConsensusError::invalid_proposal("PrepareRequest prev_hash invalid")
        })?;

        let timestamp = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest timestamp missing"))?;
        let nonce = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest nonce missing"))?;

        let count = reader
            .read_var_int(u16::MAX as u64)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest tx count invalid"))?
            as usize;

        let mut transaction_hashes = Vec::with_capacity(count);
        let mut seen = std::collections::HashSet::with_capacity(count);
        for _ in 0..count {
            let bytes = reader
                .read_memory(32)
                .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest tx hash missing"))?;
            let h = UInt256::from_bytes(bytes).map_err(|_| {
                crate::ConsensusError::invalid_proposal("PrepareRequest tx hash invalid")
            })?;
            if !seen.insert(h) {
                return Err(crate::ConsensusError::invalid_proposal(
                    "PrepareRequest contains duplicate transaction hashes",
                ));
            }
            transaction_hashes.push(h);
        }

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            version,
            prev_hash,
            timestamp,
            nonce,
            transaction_hashes,
        })
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
        let msg = PrepareRequestMessage::new(
            100,
            0,
            0,
            0,
            UInt256::zero(),
            1234567890,
            42,
            vec![UInt256::zero()],
        );

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 0);
        assert_eq!(msg.version, 0);
        assert_eq!(msg.prev_hash, UInt256::zero());
        assert_eq!(msg.timestamp, 1234567890);
        assert_eq!(msg.nonce, 42);
        assert_eq!(msg.transaction_hashes.len(), 1);
    }

    #[test]
    fn test_prepare_request_serialize() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, vec![]);
        let data = msg.serialize();

        // 7 byte header + 4 version + 32 prev_hash + 8 timestamp + 8 nonce + 1 tx count
        assert_eq!(data.len(), 60);
    }

    #[test]
    fn test_prepare_request_validate() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, vec![]);

        assert!(msg.validate(0).is_ok());
        assert!(msg.validate(1).is_err());
    }
}
