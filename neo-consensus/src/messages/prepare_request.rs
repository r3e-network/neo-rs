//! PrepareRequest message - sent by the primary to propose a block.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
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
    /// Block version (must be 0 for Neo N3)
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
    pub fn serialize(&self) -> Vec<u8> {
        // Matches C# DBFTPlugin PrepareRequest.Serialize (after the common message header):
        // `Version:u32, PrevHash:UInt256, Timestamp:u64, Nonce:u64, TransactionHashes: UInt256[] (varint count)`.
        let mut writer = BinaryWriter::new();
        let _ = writer.write_u32(self.version);
        let _ = writer.write_serializable(&self.prev_hash);
        let _ = writer.write_u64(self.timestamp);
        let _ = writer.write_u64(self.nonce);
        let _ = writer.write_serializable_vec(&self.transaction_hashes);
        writer.into_bytes()
    }

    /// Deserializes the message body (excluding the common header) from bytes.
    pub fn deserialize_body(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        use neo_io::serializable::helper::deserialize_array;

        let mut reader = MemoryReader::new(data);
        let version = reader
            .read_u32()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest version"))?;
        if version != 0 {
            return Err(crate::ConsensusError::invalid_proposal(
                "PrepareRequest version must be 0",
            ));
        }

        let prev_hash = <UInt256 as Serializable>::deserialize(&mut reader)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest prev_hash"))?;
        let timestamp = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest timestamp"))?;
        let nonce = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest nonce"))?;

        let transaction_hashes = deserialize_array::<UInt256>(&mut reader, u16::MAX as usize)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest tx hashes"))?;

        // C# checks for duplicates.
        let mut uniq = std::collections::HashSet::with_capacity(transaction_hashes.len());
        for h in &transaction_hashes {
            if !uniq.insert(*h) {
                return Err(crate::ConsensusError::invalid_proposal(
                    "PrepareRequest transaction hashes are duplicate",
                ));
            }
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

    /// Deserializes a full PrepareRequest message from a `MemoryReader`, including the common header.
    ///
    /// This is used by RecoveryMessage which embeds an entire PrepareRequest message.
    pub fn deserialize_from_reader(reader: &mut MemoryReader) -> ConsensusResult<Self> {
        let ty = reader
            .read_u8()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest type"))?;
        if ty != ConsensusMessageType::PrepareRequest.to_byte() {
            return Err(crate::ConsensusError::invalid_proposal(
                "Invalid embedded PrepareRequest type",
            ));
        }

        let block_index = reader
            .read_u32()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest block_index"))?;
        let validator_index = reader
            .read_u8()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest validator_index"))?;
        let view_number = reader
            .read_u8()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest view_number"))?;

        // Remaining fields are the message body.
        let version = reader
            .read_u32()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest version"))?;
        if version != 0 {
            return Err(crate::ConsensusError::invalid_proposal(
                "PrepareRequest version must be 0",
            ));
        }

        let prev_hash = <UInt256 as Serializable>::deserialize(reader)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest prev_hash"))?;
        let timestamp = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest timestamp"))?;
        let nonce = reader
            .read_u64()
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest nonce"))?;

        use neo_io::serializable::helper::deserialize_array;
        let transaction_hashes = deserialize_array::<UInt256>(reader, u16::MAX as usize)
            .map_err(|_| crate::ConsensusError::invalid_proposal("PrepareRequest tx hashes"))?;

        let mut uniq = std::collections::HashSet::with_capacity(transaction_hashes.len());
        for h in &transaction_hashes {
            if !uniq.insert(*h) {
                return Err(crate::ConsensusError::invalid_proposal(
                    "PrepareRequest transaction hashes are duplicate",
                ));
            }
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
        if self.version != 0 {
            return Err(crate::ConsensusError::invalid_proposal(
                "PrepareRequest version must be 0",
            ));
        }

        // C# enforces distinct transaction hashes.
        let mut uniq = std::collections::HashSet::with_capacity(self.transaction_hashes.len());
        for h in &self.transaction_hashes {
            if !uniq.insert(*h) {
                return Err(crate::ConsensusError::invalid_proposal(
                    "PrepareRequest transaction hashes are duplicate",
                ));
            }
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

        // version (4) + prev_hash (32) + timestamp (8) + nonce (8) + tx count (1)
        assert_eq!(data.len(), 53);
    }

    #[test]
    fn test_prepare_request_wire_format_bytes() {
        let prev_hash = UInt256::from([0xAAu8; 32]);
        let tx1 = UInt256::from([0x01u8; 32]);
        let tx2 = UInt256::from([0x02u8; 32]);
        let timestamp = 0x0A0B_0C0D_0102_0304u64;
        let nonce = 0x1122_3344_5566_7788u64;

        let msg = PrepareRequestMessage::new(
            100,
            0,
            0,
            0,
            prev_hash,
            timestamp,
            nonce,
            vec![tx1, tx2],
        );
        let data = msg.serialize();

        let mut expected = Vec::new();
        expected.extend_from_slice(&0u32.to_le_bytes());
        expected.extend_from_slice(&prev_hash.to_array());
        expected.extend_from_slice(&timestamp.to_le_bytes());
        expected.extend_from_slice(&nonce.to_le_bytes());
        expected.push(0x02); // varint count
        expected.extend_from_slice(&tx1.to_array());
        expected.extend_from_slice(&tx2.to_array());

        assert_eq!(data, expected);
    }

    #[test]
    fn test_prepare_request_validate() {
        let msg = PrepareRequestMessage::new(100, 0, 0, 0, UInt256::zero(), 1000, 1, vec![]);

        assert!(msg.validate(0).is_ok());
        assert!(msg.validate(1).is_err());
    }
}
