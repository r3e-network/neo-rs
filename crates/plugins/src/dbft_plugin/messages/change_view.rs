// Copyright (C) 2015-2025 The Neo Project.
//
// change_view.rs file belongs to the neo project and is free
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
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;
use crate::dbft_plugin::types::consensus_message_type::ConsensusMessageType;
use neo_core::neo_io::{BinaryWriter, MemoryReader};

/// ChangeView message (matches Neo C# ChangeView exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeView {
    header: ConsensusMessageHeader,
    timestamp: u64,
    reason: ChangeViewReason,
}

impl ChangeView {
    /// Creates a new ChangeView message with explicit parameters.
    pub fn new(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> Self {
        Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::ChangeView,
                block_index,
                validator_index,
                view_number,
            ),
            timestamp,
            reason,
        }
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> Self {
        Self::new(block_index, validator_index, view_number, timestamp, reason)
    }

    /// Returns the message header.
    pub fn header(&self) -> &ConsensusMessageHeader {
        &self.header
    }

    /// Returns a mutable reference to the message header.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        &mut self.header
    }

    /// Gets the block index carried by the change-view message.
    pub fn block_index(&self) -> u32 {
        self.header.block_index
    }

    /// Gets the validator index that issued the change view.
    pub fn validator_index(&self) -> u8 {
        self.header.validator_index
    }

    /// Gets the current view number reported by the sender.
    pub fn view_number(&self) -> u8 {
        self.header.view_number
    }

    /// Gets the timestamp of the change view message.
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Gets the reason supplied for the view change.
    pub fn reason(&self) -> ChangeViewReason {
        self.reason
    }

    /// Gets the new view number the validator is requesting.
    pub fn new_view_number(&self) -> u8 {
        self.header.view_number.wrapping_add(1)
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        8 + 1
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        writer.write_u64(self.timestamp)?;
        writer.write_u8(self.reason.to_byte())?;
        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a ChangeView message using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::ChangeView {
            return Err(ConsensusMessageError::invalid_data(
                "ChangeView payload received for non-ChangeView header",
            ));
        }

        let timestamp = reader.read_u64()?;
        let reason_byte = reader.read_u8()?;
        let reason = ChangeViewReason::from_byte(reason_byte)
            .ok_or_else(|| ConsensusMessageError::invalid_data(format!(
                "Invalid ChangeView reason byte {reason_byte:#x}"
            )))?;

        Ok(Self {
            header,
            timestamp,
            reason,
        })
    }

    /// Deserializes a ChangeView message from the reader, including header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }
}
