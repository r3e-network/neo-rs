// Copyright (C) 2015-2025 The Neo Project.
//
// commit.rs file belongs to the neo project and is free
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

/// Commit message (matches Neo C# Commit exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    header: ConsensusMessageHeader,
    signature: Vec<u8>,
}

impl Commit {
    /// Expected length of the commit signature in bytes.
    pub const SIGNATURE_LENGTH: usize = 64;

    /// Creates a new commit message with the provided signature.
    pub fn new(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        signature: Vec<u8>,
    ) -> ConsensusMessageResult<Self> {
        if signature.len() != Self::SIGNATURE_LENGTH {
            return Err(ConsensusMessageError::invalid_data(format!(
                "Commit signature must be {} bytes, got {}",
                Self::SIGNATURE_LENGTH,
                signature.len()
            )));
        }

        Ok(Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::Commit,
                block_index,
                validator_index,
                view_number,
            ),
            signature,
        })
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        signature: Vec<u8>,
    ) -> Self {
        Self::new(block_index, validator_index, view_number, signature)
            .expect("commit signature must be 64 bytes")
    }

    /// Returns the message header.
    pub fn header(&self) -> &ConsensusMessageHeader {
        &self.header
    }

    /// Returns a mutable reference to the message header.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        &mut self.header
    }

    /// Gets the commit signature.
    pub fn signature(&self) -> &[u8] {
        &self.signature
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        self.signature.len()
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        if self.signature.len() != Self::SIGNATURE_LENGTH {
            return Err(ConsensusMessageError::invalid_data(
                "Commit signature length mismatch",
            ));
        }
        writer.write_bytes(&self.signature)?;
        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a commit message using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::Commit {
            return Err(ConsensusMessageError::invalid_data(
                "Commit payload received for non-Commit header",
            ));
        }

        let signature = reader.read_bytes(Self::SIGNATURE_LENGTH)?;
        let mut commit = Self::new(
            header.block_index,
            header.validator_index,
            header.view_number,
            signature,
        )?;
        commit.header = header;
        Ok(commit)
    }

    /// Deserializes a commit message from the reader, including header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }
}
