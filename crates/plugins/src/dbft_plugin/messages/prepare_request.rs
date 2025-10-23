// Copyright (C) 2015-2025 The Neo Project.
//
// prepare_request.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::messages::consensus_message::{
    ConsensusMessageError, ConsensusMessageHeader, ConsensusMessageResult,
};
use crate::dbft_plugin::types::consensus_message_type::ConsensusMessageType;
use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_core::neo_system::ProtocolSettings;
use neo_core::UInt256;
use std::collections::HashSet;

/// PrepareRequest message (matches Neo C# PrepareRequest exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareRequest {
    header: ConsensusMessageHeader,
    version: u32,
    prev_hash: UInt256,
    timestamp: u64,
    nonce: u64,
    transaction_hashes: Vec<UInt256>,
}

impl PrepareRequest {
    /// Creates a new prepare request message.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        version: u32,
        prev_hash: UInt256,
        timestamp: u64,
        nonce: u64,
        transaction_hashes: Vec<UInt256>,
    ) -> Self {
        Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::PrepareRequest,
                block_index,
                validator_index,
                view_number,
            ),
            version,
            prev_hash,
            timestamp,
            nonce,
            transaction_hashes,
        }
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        version: u32,
        prev_hash: UInt256,
        timestamp: u64,
        nonce: u64,
        transaction_hashes: Vec<UInt256>,
    ) -> Self {
        Self::new(
            block_index,
            validator_index,
            view_number,
            version,
            prev_hash,
            timestamp,
            nonce,
            transaction_hashes,
        )
    }

    /// Returns the message header.
    pub fn header(&self) -> &ConsensusMessageHeader {
        &self.header
    }

    /// Returns a mutable reference to the message header.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        &mut self.header
    }

    /// Gets the block index carried by the request.
    pub fn block_index(&self) -> u32 {
        self.header.block_index
    }

    /// Gets the validator index of the primary that produced the request.
    pub fn validator_index(&self) -> u8 {
        self.header.validator_index
    }

    /// Gets the view number associated with the request.
    pub fn view_number(&self) -> u8 {
        self.header.view_number
    }

    /// Gets the block version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Gets the hash of the previous block.
    pub fn prev_hash(&self) -> &UInt256 {
        &self.prev_hash
    }

    /// Gets the timestamp proposed for the new block.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Gets the nonce proposed for the new block.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Gets the transaction hashes included in this prepare request.
    pub fn transaction_hashes(&self) -> &[UInt256] {
        &self.transaction_hashes
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        let hash_size = UInt256::default().size();
        4 + hash_size + 8 + 8
            + var_int_size(self.transaction_hashes.len())
            + self.transaction_hashes.len() * hash_size
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        writer.write_u32(self.version)?;
        self.prev_hash.serialize(writer)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_var_int(self.transaction_hashes.len() as u64)?;
        for hash in &self.transaction_hashes {
            hash.serialize(writer)?;
        }
        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a prepare request using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::PrepareRequest {
            return Err(ConsensusMessageError::invalid_data(
                "PrepareRequest payload received for non-PrepareRequest header",
            ));
        }

        let version = reader.read_u32()?;
        let prev_hash = UInt256::deserialize(reader)?;
        let timestamp = reader.read_u64()?;
        let nonce = reader.read_u64()?;

        let count = reader.read_var_int(u16::MAX as u64)? as usize;
        let mut transaction_hashes = Vec::with_capacity(count);
        let mut uniqueness = HashSet::with_capacity(count);
        for _ in 0..count {
            let hash = UInt256::deserialize(reader)?;
            if !uniqueness.insert(hash) {
                return Err(ConsensusMessageError::invalid_data(
                    "PrepareRequest contains duplicate transaction hashes",
                ));
            }
            transaction_hashes.push(hash);
        }

        Ok(Self {
            header,
            version,
            prev_hash,
            timestamp,
            nonce,
            transaction_hashes,
        })
    }

    /// Deserializes a prepare request from a reader, including its header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }

    /// Verifies the message against the provided protocol settings.
    pub fn verify(&self, settings: &ProtocolSettings) -> bool {
        let validator_count = settings.validators_count.max(0) as u32;
        if self.header.validator_index as u32 >= validator_count {
            return false;
        }

        (self.transaction_hashes.len() as u32) <= settings.max_transactions_per_block
    }
}

fn var_int_size(value: usize) -> usize {
    if value < 0xFD {
        1
    } else if value <= 0xFFFF {
        3
    } else if value <= 0xFFFF_FFFF {
        5
    } else {
        9
    }
}
