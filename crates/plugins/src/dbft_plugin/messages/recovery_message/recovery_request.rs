// Copyright (C) 2015-2025 The Neo Project.
//
// recovery_request.rs file belongs to the neo project and is free
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
use neo_core::neo_io::{BinaryWriter, MemoryReader};

/// RecoveryRequest message (matches Neo C# RecoveryRequest exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRequest {
    header: ConsensusMessageHeader,
    timestamp: u64,
}

impl RecoveryRequest {
    /// Creates a new recovery request message.
    pub fn new(block_index: u32, validator_index: u8, view_number: u8, timestamp: u64) -> Self {
        Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::RecoveryRequest,
                block_index,
                validator_index,
                view_number,
            ),
            timestamp,
        }
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        timestamp: u64,
    ) -> Self {
        Self::new(block_index, validator_index, view_number, timestamp)
    }

    /// Returns the message header.
    pub fn header(&self) -> &ConsensusMessageHeader {
        &self.header
    }

    /// Gets the block index carried by the request.
    pub fn block_index(&self) -> u32 {
        self.header.block_index
    }

    /// Gets the validator index carried by the request.
    pub fn validator_index(&self) -> u8 {
        self.header.validator_index
    }

    /// Gets the view number carried by the request.
    pub fn view_number(&self) -> u8 {
        self.header.view_number
    }

    /// Returns a mutable reference to the message header.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        &mut self.header
    }

    /// Gets the timestamp carried by the recovery request.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        8
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        writer.write_u64(self.timestamp)?;
        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a recovery request using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::RecoveryRequest {
            return Err(ConsensusMessageError::invalid_data(
                "RecoveryRequest payload received for non-RecoveryRequest header",
            ));
        }

        let timestamp = reader.read_u64()?;
        Ok(Self { header, timestamp })
    }

    /// Deserializes a recovery request from the reader, including header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }
}
