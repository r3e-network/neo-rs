// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_message.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    ChangeView, Commit, PrepareRequest, PrepareResponse, RecoveryMessage, RecoveryRequest,
};
use crate::dbft_plugin::types::consensus_message_type::ConsensusMessageType;
use neo_core::neo_io::{BinaryWriter, IoError, MemoryReader};
use neo_core::neo_system::ProtocolSettings;
use thiserror::Error;

/// Result alias for consensus message operations.
pub type ConsensusMessageResult<T> = Result<T, ConsensusMessageError>;

/// Errors that can occur when working with consensus messages.
#[derive(Debug, Error)]
pub enum ConsensusMessageError {
    /// Underlying I/O error while reading or writing message payloads.
    #[error("{0}")]
    Io(#[from] IoError),
    /// Encountered an unknown consensus message type byte.
    #[error("Unknown consensus message type {0:#x}")]
    UnknownMessageType(u8),
    /// Message payload contained invalid data.
    #[error("{0}")]
    InvalidFormat(String),
}

impl ConsensusMessageError {
    pub(crate) fn invalid_data(message: impl Into<String>) -> Self {
        Self::InvalidFormat(message.into())
    }
}

/// Header shared by all consensus messages (matches C# ConsensusMessage fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsensusMessageHeader {
    pub message_type: ConsensusMessageType,
    pub block_index: u32,
    pub validator_index: u8,
    pub view_number: u8,
}

impl ConsensusMessageHeader {
    /// Size of the serialized header in bytes.
    pub const SIZE: usize = 1 + 4 + 1 + 1;

    /// Creates a new header with zeroed numeric fields.
    pub fn new(message_type: ConsensusMessageType) -> Self {
        Self {
            message_type,
            block_index: 0,
            validator_index: 0,
            view_number: 0,
        }
    }

    /// Creates a header from explicit values.
    pub fn with_values(
        message_type: ConsensusMessageType,
        block_index: u32,
        validator_index: u8,
        view_number: u8,
    ) -> Self {
        Self {
            message_type,
            block_index,
            validator_index,
            view_number,
        }
    }

    /// Deserializes a header from the supplied reader.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let message_type_byte = reader.read_u8()?;
        let message_type = ConsensusMessageType::from_byte(message_type_byte)
            .ok_or(ConsensusMessageError::UnknownMessageType(message_type_byte))?;
        let block_index = reader.read_u32()?;
        let validator_index = reader.read_u8()?;
        let view_number = reader.read_u8()?;

        Ok(Self {
            message_type,
            block_index,
            validator_index,
            view_number,
        })
    }

    /// Serializes the header into the supplied writer.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        writer.write_u8(self.message_type.to_byte())?;
        writer.write_u32(self.block_index)?;
        writer.write_u8(self.validator_index)?;
        writer.write_u8(self.view_number)?;
        Ok(())
    }

    /// Returns the size of the header when serialized.
    pub fn size(&self) -> usize {
        Self::SIZE
    }
}

/// Discriminated union for every consensus message type.
#[derive(Debug, Clone)]
pub enum ConsensusMessagePayload {
    ChangeView(ChangeView),
    PrepareRequest(PrepareRequest),
    PrepareResponse(PrepareResponse),
    Commit(Commit),
    RecoveryRequest(RecoveryRequest),
    RecoveryMessage(RecoveryMessage),
}

impl ConsensusMessagePayload {
    /// Deserializes a consensus message from raw bytes.
    pub fn deserialize_from(data: &[u8]) -> ConsensusMessageResult<Self> {
        let mut reader = MemoryReader::new(data);
        let header = ConsensusMessageHeader::deserialize(&mut reader)?;
        let payload = match header.message_type {
            ConsensusMessageType::ChangeView => ConsensusMessagePayload::ChangeView(
                ChangeView::deserialize_with_header(header, &mut reader)?,
            ),
            ConsensusMessageType::PrepareRequest => ConsensusMessagePayload::PrepareRequest(
                PrepareRequest::deserialize_with_header(header, &mut reader)?,
            ),
            ConsensusMessageType::PrepareResponse => ConsensusMessagePayload::PrepareResponse(
                PrepareResponse::deserialize_with_header(header, &mut reader)?,
            ),
            ConsensusMessageType::Commit => ConsensusMessagePayload::Commit(
                Commit::deserialize_with_header(header, &mut reader)?,
            ),
            ConsensusMessageType::RecoveryRequest => ConsensusMessagePayload::RecoveryRequest(
                RecoveryRequest::deserialize_with_header(header, &mut reader)?,
            ),
            ConsensusMessageType::RecoveryMessage => ConsensusMessagePayload::RecoveryMessage(
                RecoveryMessage::deserialize_with_header(header, &mut reader)?,
            ),
        };

        if reader.remaining() != 0 {
            return Err(ConsensusMessageError::invalid_data(
                "Unexpected trailing bytes in consensus message",
            ));
        }

        Ok(payload)
    }

    /// Serializes the message to a byte vector.
    pub fn to_bytes(&self) -> ConsensusMessageResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();
        match self {
            ConsensusMessagePayload::ChangeView(message) => {
                message.write_with_header(&mut writer)?
            }
            ConsensusMessagePayload::PrepareRequest(message) => {
                message.write_with_header(&mut writer)?
            }
            ConsensusMessagePayload::PrepareResponse(message) => {
                message.write_with_header(&mut writer)?
            }
            ConsensusMessagePayload::Commit(message) => message.write_with_header(&mut writer)?,
            ConsensusMessagePayload::RecoveryRequest(message) => {
                message.write_with_header(&mut writer)?
            }
            ConsensusMessagePayload::RecoveryMessage(message) => {
                message.write_with_header(&mut writer)?
            }
        }
        Ok(writer.into_bytes())
    }

    /// Gets a shared reference to the header of the message.
    pub fn header(&self) -> &ConsensusMessageHeader {
        match self {
            ConsensusMessagePayload::ChangeView(message) => message.header(),
            ConsensusMessagePayload::PrepareRequest(message) => message.header(),
            ConsensusMessagePayload::PrepareResponse(message) => message.header(),
            ConsensusMessagePayload::Commit(message) => message.header(),
            ConsensusMessagePayload::RecoveryRequest(message) => message.header(),
            ConsensusMessagePayload::RecoveryMessage(message) => message.header(),
        }
    }

    /// Gets a mutable reference to the header of the message.
    pub fn header_mut(&mut self) -> &mut ConsensusMessageHeader {
        match self {
            ConsensusMessagePayload::ChangeView(message) => message.header_mut(),
            ConsensusMessagePayload::PrepareRequest(message) => message.header_mut(),
            ConsensusMessagePayload::PrepareResponse(message) => message.header_mut(),
            ConsensusMessagePayload::Commit(message) => message.header_mut(),
            ConsensusMessagePayload::RecoveryRequest(message) => message.header_mut(),
            ConsensusMessagePayload::RecoveryMessage(message) => message.header_mut(),
        }
    }

    /// Returns the message type discriminant.
    pub fn message_type(&self) -> ConsensusMessageType {
        self.header().message_type
    }

    /// Gets the block index carried by the message.
    pub fn block_index(&self) -> u32 {
        self.header().block_index
    }

    /// Gets the validator index carried by the message.
    pub fn validator_index(&self) -> u8 {
        self.header().validator_index
    }

    /// Gets the view number carried by the message.
    pub fn view_number(&self) -> u8 {
        self.header().view_number
    }

    /// Verifies the message against the provided protocol settings.
    pub fn verify(&self, settings: &ProtocolSettings) -> bool {
        let validator_count = settings.validators_count.max(0) as u32;
        if self.validator_index() as u32 >= validator_count {
            return false;
        }

        match self {
            ConsensusMessagePayload::PrepareRequest(message) => message.verify(settings),
            ConsensusMessagePayload::RecoveryMessage(message) => message.verify(settings),
            _ => true,
        }
    }

    /// Returns the size of the serialized message.
    pub fn size(&self) -> usize {
        self.header().size()
            + match self {
                ConsensusMessagePayload::ChangeView(message) => message.body_size(),
                ConsensusMessagePayload::PrepareRequest(message) => message.body_size(),
                ConsensusMessagePayload::PrepareResponse(message) => message.body_size(),
                ConsensusMessagePayload::Commit(message) => message.body_size(),
                ConsensusMessagePayload::RecoveryRequest(message) => message.body_size(),
                ConsensusMessagePayload::RecoveryMessage(message) => message.body_size(),
            }
    }

    /// Attempts to borrow the payload as a change view message.
    pub fn as_change_view(&self) -> Option<&ChangeView> {
        match self {
            ConsensusMessagePayload::ChangeView(message) => Some(message),
            _ => None,
        }
    }

    /// Attempts to borrow the payload as a commit message.
    pub fn as_commit(&self) -> Option<&Commit> {
        match self {
            ConsensusMessagePayload::Commit(message) => Some(message),
            _ => None,
        }
    }

    /// Attempts to borrow the payload as a prepare request message.
    pub fn as_prepare_request(&self) -> Option<&PrepareRequest> {
        match self {
            ConsensusMessagePayload::PrepareRequest(message) => Some(message),
            _ => None,
        }
    }

    /// Attempts to borrow the payload as a prepare response message.
    pub fn as_prepare_response(&self) -> Option<&PrepareResponse> {
        match self {
            ConsensusMessagePayload::PrepareResponse(message) => Some(message),
            _ => None,
        }
    }

    /// Attempts to borrow the payload as a recovery request message.
    pub fn as_recovery_request(&self) -> Option<&RecoveryRequest> {
        match self {
            ConsensusMessagePayload::RecoveryRequest(message) => Some(message),
            _ => None,
        }
    }

    /// Attempts to borrow the payload as a recovery message.
    pub fn as_recovery_message(&self) -> Option<&RecoveryMessage> {
        match self {
            ConsensusMessagePayload::RecoveryMessage(message) => Some(message),
            _ => None,
        }
    }
}

impl From<ChangeView> for ConsensusMessagePayload {
    fn from(message: ChangeView) -> Self {
        ConsensusMessagePayload::ChangeView(message)
    }
}

impl From<PrepareRequest> for ConsensusMessagePayload {
    fn from(message: PrepareRequest) -> Self {
        ConsensusMessagePayload::PrepareRequest(message)
    }
}

impl From<PrepareResponse> for ConsensusMessagePayload {
    fn from(message: PrepareResponse) -> Self {
        ConsensusMessagePayload::PrepareResponse(message)
    }
}

impl From<Commit> for ConsensusMessagePayload {
    fn from(message: Commit) -> Self {
        ConsensusMessagePayload::Commit(message)
    }
}

impl From<RecoveryRequest> for ConsensusMessagePayload {
    fn from(message: RecoveryRequest) -> Self {
        ConsensusMessagePayload::RecoveryRequest(message)
    }
}

impl From<RecoveryMessage> for ConsensusMessagePayload {
    fn from(message: RecoveryMessage) -> Self {
        ConsensusMessagePayload::RecoveryMessage(message)
    }
}
