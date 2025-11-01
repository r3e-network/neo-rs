// Copyright (C) 2015-2025 The Neo Project.
//
// prepare_response.rs file belongs to the neo project and is free
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
use neo_core::UInt256;

/// PrepareResponse message (matches Neo C# PrepareResponse exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareResponse {
    header: ConsensusMessageHeader,
    preparation_hash: UInt256,
}

impl PrepareResponse {
    /// Creates a new prepare response message.
    pub fn new(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        preparation_hash: UInt256,
    ) -> Self {
        Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::PrepareResponse,
                block_index,
                validator_index,
                view_number,
            ),
            preparation_hash,
        }
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        preparation_hash: UInt256,
    ) -> Self {
        Self::new(block_index, validator_index, view_number, preparation_hash)
    }

    /// Returns the message header.
    pub fn header(&self) -> &ConsensusMessageHeader {
        &self.header
    }

    /// Returns a mutable reference to the message header.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        &mut self.header
    }

    /// Gets the block index carried by the response.
    pub fn block_index(&self) -> u32 {
        self.header.block_index
    }

    /// Gets the validator index of the responder.
    pub fn validator_index(&self) -> u8 {
        self.header.validator_index
    }

    /// Gets the view number for which the response was produced.
    pub fn view_number(&self) -> u8 {
        self.header.view_number
    }

    /// Gets the preparation hash referenced by this response.
    pub fn preparation_hash(&self) -> &UInt256 {
        &self.preparation_hash
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        self.preparation_hash.size()
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.preparation_hash.serialize(writer)?;
        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(
        &self,
        writer: &mut BinaryWriter,
    ) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a prepare response using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::PrepareResponse {
            return Err(ConsensusMessageError::invalid_data(
                "PrepareResponse payload received for non-PrepareResponse header",
            ));
        }

        let preparation_hash = UInt256::deserialize(reader)?;
        Ok(Self {
            header,
            preparation_hash,
        })
    }

    /// Deserializes a prepare response from the reader, including header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }
}
