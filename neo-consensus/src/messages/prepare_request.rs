//! `PrepareRequest` message - sent by the primary to propose a block.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// `PrepareRequest` message sent by the primary (speaker) to propose a new block.
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
    /// Creates a new `PrepareRequest` message
    // Rationale: dBFT prepare-request fields map one-to-one to the wire
    // message; keeping them explicit avoids hidden consensus defaults.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
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
    #[must_use]
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::PrepareRequest
    }

    /// Serializes the message to bytes
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        // Matches C# DBFTPlugin PrepareRequest.Serialize (after the common message header):
        // `Version:u32, PrevHash:UInt256, Timestamp:u64, Nonce:u64, TransactionHashes: UInt256[] (varint count)`.
        let mut writer = BinaryWriter::new();
        writer
            .write_u32(self.version)
            .expect("infallible: in-memory write");
        writer
            .write_serializable(&self.prev_hash)
            .expect("infallible: in-memory write");
        writer
            .write_u64(self.timestamp)
            .expect("infallible: in-memory write");
        writer
            .write_u64(self.nonce)
            .expect("infallible: in-memory write");
        writer
            .write_serializable_vec(&self.transaction_hashes)
            .expect("infallible: in-memory write");
        writer.into_bytes()
    }

    /// Deserializes the message body (excluding the common header) from bytes.
    pub fn deserialize_body(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        use neo_io::serializable::helper::SerializeHelper;

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

        let transaction_hashes =
            SerializeHelper::deserialize_array::<UInt256>(&mut reader, u16::MAX as usize)
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

    /// Deserializes a full `PrepareRequest` message from a `MemoryReader`, including the common header.
    ///
    /// This is used by `RecoveryMessage` which embeds an entire `PrepareRequest` message.
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
        let validator_index = reader.read_u8().map_err(|_| {
            crate::ConsensusError::invalid_proposal("PrepareRequest validator_index")
        })?;
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

        use neo_io::serializable::helper::SerializeHelper;
        let transaction_hashes =
            SerializeHelper::deserialize_array::<UInt256>(reader, u16::MAX as usize)
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
    pub fn validate(
        &self,
        expected_primary: u8,
        max_transactions_per_block: u32,
    ) -> ConsensusResult<()> {
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

        if self.transaction_hashes.len() > max_transactions_per_block as usize {
            return Err(crate::ConsensusError::invalid_proposal(
                "PrepareRequest exceeds MaxTransactionsPerBlock",
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
#[path = "../tests/messages/prepare_request.rs"]
mod tests;
