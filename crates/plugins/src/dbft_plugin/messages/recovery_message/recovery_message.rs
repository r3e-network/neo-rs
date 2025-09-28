// Copyright (C) 2015-2025 The Neo Project.
//
// recovery_message.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_context::{ConsensusContext, ExtensiblePayload};
use crate::dbft_plugin::messages::change_view::ChangeView;
use crate::dbft_plugin::messages::commit::Commit;
use crate::dbft_plugin::messages::consensus_message::{
    ConsensusMessageError, ConsensusMessageHeader, ConsensusMessageResult,
};
use crate::dbft_plugin::messages::prepare_request::PrepareRequest;
use crate::dbft_plugin::messages::prepare_response::PrepareResponse;
use crate::dbft_plugin::messages::recovery_message::recovery_message_change_view_payload_compact::ChangeViewPayloadCompact;
use crate::dbft_plugin::messages::recovery_message::recovery_message_commit_payload_compact::CommitPayloadCompact;
use crate::dbft_plugin::messages::recovery_message::recovery_message_preparation_payload_compact::PreparationPayloadCompact;
use crate::dbft_plugin::messages::recovery_message::recovery_request::RecoveryRequest;
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;
use crate::dbft_plugin::types::consensus_message_type::ConsensusMessageType;
use neo_core::neo_io::{BinaryWriter, MemoryReader};
use neo_core::neo_system::ProtocolSettings;
use neo_core::UInt256;
use std::collections::HashMap;

/// RecoveryMessage (matches Neo C# RecoveryMessage exactly).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryMessage {
    header: ConsensusMessageHeader,
    change_view_messages: HashMap<u8, ChangeViewPayloadCompact>,
    prepare_request_message: Option<PrepareRequest>,
    preparation_hash: Option<UInt256>,
    preparation_messages: HashMap<u8, PreparationPayloadCompact>,
    commit_messages: HashMap<u8, CommitPayloadCompact>,
}

impl RecoveryMessage {
    /// Creates a new recovery message.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        change_view_messages: HashMap<u8, ChangeViewPayloadCompact>,
        prepare_request_message: Option<PrepareRequest>,
        preparation_hash: Option<UInt256>,
        preparation_messages: HashMap<u8, PreparationPayloadCompact>,
        commit_messages: HashMap<u8, CommitPayloadCompact>,
    ) -> Self {
        Self {
            header: ConsensusMessageHeader::with_values(
                ConsensusMessageType::RecoveryMessage,
                block_index,
                validator_index,
                view_number,
            ),
            change_view_messages,
            prepare_request_message,
            preparation_hash,
            preparation_messages,
            commit_messages,
        }
    }

    /// Convenience constructor matching the C# helper.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn with_params(
        block_index: u32,
        validator_index: u8,
        view_number: u8,
        change_view_messages: HashMap<u8, ChangeViewPayloadCompact>,
        prepare_request_message: Option<PrepareRequest>,
        preparation_hash: Option<UInt256>,
        preparation_messages: HashMap<u8, PreparationPayloadCompact>,
        commit_messages: HashMap<u8, CommitPayloadCompact>,
    ) -> Self {
        Self::new(
            block_index,
            validator_index,
            view_number,
            change_view_messages,
            prepare_request_message,
            preparation_hash,
            preparation_messages,
            commit_messages,
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

    /// Gets the block index carried by the recovery message.
    pub fn block_index(&self) -> u32 {
        self.header.block_index
    }

    /// Gets the validator index of the sender.
    pub fn validator_index(&self) -> u8 {
        self.header.validator_index
    }

    /// Gets the view number for which the recovery message was generated.
    pub fn view_number(&self) -> u8 {
        self.header.view_number
    }

    /// Gets the compact change view payloads indexed by validator.
    pub fn change_view_messages(&self) -> &HashMap<u8, ChangeViewPayloadCompact> {
        &self.change_view_messages
    }

    /// Gets the optional prepare request message included in the recovery payload.
    pub fn prepare_request_message(&self) -> Option<&PrepareRequest> {
        self.prepare_request_message.as_ref()
    }

    /// Gets the optional preparation hash included in the recovery payload.
    pub fn preparation_hash(&self) -> Option<&UInt256> {
        self.preparation_hash.as_ref()
    }

    /// Gets the compact preparation payloads indexed by validator.
    pub fn preparation_messages(&self) -> &HashMap<u8, PreparationPayloadCompact> {
        &self.preparation_messages
    }

    /// Gets the compact commit payloads indexed by validator.
    pub fn commit_messages(&self) -> &HashMap<u8, CommitPayloadCompact> {
        &self.commit_messages
    }

    /// Serialized size of the message body (excluding header).
    pub(crate) fn body_size(&self) -> usize {
        let change_view_size = serialized_compact_map_size(&self.change_view_messages, |payload| {
            payload.size()
        });

        let prepare_section = if let Some(request) = &self.prepare_request_message {
            1 + request.header().size() + request.body_size()
        } else {
            let hash_size = UInt256::default().size();
            1 + var_int_size(if self.preparation_hash.is_some() { hash_size } else { 0 })
                + self.preparation_hash.as_ref().map(|_| hash_size).unwrap_or(0)
        };

        let preparation_messages_size = serialized_compact_map_size(&self.preparation_messages, |payload| {
            payload.size()
        });

        let commit_messages_size = serialized_compact_map_size(&self.commit_messages, |payload| {
            payload.size()
        });

        change_view_size + prepare_section + preparation_messages_size + commit_messages_size
    }

    /// Serializes the message body (excluding header).
    pub(crate) fn write_body(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        write_compact_map(writer, &self.change_view_messages, |payload, writer| {
            payload.serialize(writer)
        })?;

        writer.write_bool(self.prepare_request_message.is_some())?;
        if let Some(request) = &self.prepare_request_message {
            request.write_with_header(writer)?;
        } else {
            let hash_size = UInt256::default().size();
            if let Some(hash) = &self.preparation_hash {
                writer.write_var_int(hash_size as u64)?;
                hash.serialize(writer)?;
            } else {
                writer.write_var_int(0)?;
            }
        }

        write_compact_map(writer, &self.preparation_messages, |payload, writer| {
            payload.serialize(writer)
        })?;

        write_compact_map(writer, &self.commit_messages, |payload, writer| {
            payload.serialize(writer)
        })?;

        Ok(())
    }

    /// Serializes the full message including header.
    pub(crate) fn write_with_header(&self, writer: &mut BinaryWriter) -> ConsensusMessageResult<()> {
        self.header.serialize(writer)?;
        self.write_body(writer)
    }

    /// Deserializes a recovery message using an already-read header.
    pub(crate) fn deserialize_with_header(
        header: ConsensusMessageHeader,
        reader: &mut MemoryReader,
    ) -> ConsensusMessageResult<Self> {
        if header.message_type != ConsensusMessageType::RecoveryMessage {
            return Err(ConsensusMessageError::invalid_data(
                "RecoveryMessage payload received for non-RecoveryMessage header",
            ));
        }

        let change_view_messages = read_compact_map(reader, u8::MAX as u64, |reader| {
            ChangeViewPayloadCompact::deserialize(reader)
        }, |payload| payload.validator_index)?;

        let prepare_request_message = if reader.read_bool()? {
            Some(PrepareRequest::deserialize(reader)?)
        } else {
            None
        };

        let preparation_hash = if prepare_request_message.is_some() {
            None
        } else {
            let expected_size = UInt256::default().size() as u64;
            let length = reader.read_var_int(expected_size)? as usize;
            if length == 0 {
                None
            } else if length == expected_size as usize {
                let bytes = reader.read_bytes(length)?;
                let hash = UInt256::try_from(bytes.as_slice()).map_err(|err| {
                    ConsensusMessageError::invalid_data(format!(
                        "Failed to deserialize preparation hash: {err}"
                    ))
                })?;
                Some(hash)
            } else {
                return Err(ConsensusMessageError::invalid_data(
                    "RecoveryMessage PreparationHash length mismatch",
                ));
            }
        };

        let preparation_messages = read_compact_map(reader, u8::MAX as u64, |reader| {
            PreparationPayloadCompact::deserialize(reader)
        }, |payload| payload.validator_index)?;

        let commit_messages = read_compact_map(reader, u8::MAX as u64, |reader| {
            CommitPayloadCompact::deserialize(reader)
        }, |payload| payload.validator_index)?;

        Ok(Self {
            header,
            change_view_messages,
            prepare_request_message,
            preparation_hash,
            preparation_messages,
            commit_messages,
        })
    }

    /// Deserializes a recovery message from the reader, including header.
    pub fn deserialize(reader: &mut MemoryReader) -> ConsensusMessageResult<Self> {
        let header = ConsensusMessageHeader::deserialize(reader)?;
        Self::deserialize_with_header(header, reader)
    }

    /// Converts the compact change view payloads into full payloads using the provided context.
    pub fn get_change_view_payloads(&self, context: &ConsensusContext) -> ConsensusMessageResult<Vec<ExtensiblePayload>> {
        self.change_view_messages
            .values()
            .map(|compact| {
                let mut message = ChangeView::new(
                    self.block_index(),
                    compact.validator_index,
                    compact.original_view_number,
                    compact.timestamp,
                    ChangeViewReason::Timeout,
                );
                message.header_mut().validator_index = compact.validator_index;
                context.create_payload(message, Some(compact.invocation_script.clone()))
            })
            .collect()
    }

    /// Converts the compact commit payloads into full payloads using the provided context.
    pub fn get_commit_payloads_from_recovery_message(
        &self,
        context: &ConsensusContext,
    ) -> ConsensusMessageResult<Vec<ExtensiblePayload>> {
        self.commit_messages
            .values()
            .map(|compact| {
                let message = Commit::new(
                    self.block_index(),
                    compact.validator_index,
                    compact.view_number,
                    compact.signature.clone(),
                )?;
                context.create_payload(message, Some(compact.invocation_script.clone()))
            })
            .collect()
    }

    /// Gets the prepare request payload from the recovery message, if any.
    pub fn get_prepare_request_payload(
        &self,
        context: &ConsensusContext,
    ) -> ConsensusMessageResult<Option<ExtensiblePayload>> {
        match &self.prepare_request_message {
            Some(request) => {
                let invocation = self
                    .preparation_messages
                    .get(&context.block.primary_index())
                    .map(|compact| compact.invocation_script.clone())
                    .unwrap_or_default();
                context
                    .create_payload(request.clone(), Some(invocation))
                    .map(Some)
            }
            None => Ok(None),
        }
    }

    /// Produces prepare response payloads derived from the recovery contents.
    pub fn get_prepare_response_payloads(
        &self,
        context: &ConsensusContext,
    ) -> ConsensusMessageResult<Vec<ExtensiblePayload>> {
        let preparation_hash = match self.preparation_hash {
            Some(hash) => hash,
            None => {
                if let Some(payload) = context.preparation_payloads()[context.block.primary_index() as usize].as_ref() {
                    let mut p = payload.clone();
                    ConsensusContext::payload_hash(&mut p)
                } else {
                    return Ok(Vec::new());
                }
            }
        };

        self.preparation_messages
            .values()
            .filter(|compact| compact.validator_index != context.block.primary_index() as u8)
            .map(|compact| {
                let message = PrepareResponse::new(
                    self.block_index(),
                    compact.validator_index,
                    self.view_number(),
                    preparation_hash,
                );
                context.create_payload(message, Some(compact.invocation_script.clone()))
            })
            .collect()
    }

    /// Verifies the recovery message against protocol settings.
    pub fn verify(&self, protocol_settings: &ProtocolSettings) -> bool {
        if self.header.validator_index as u32 >= protocol_settings.validators_count {
            return false;
        }

        if let Some(request) = &self.prepare_request_message {
            if !request.verify(protocol_settings) {
                return false;
            }
        }

        self.change_view_messages
            .values()
            .all(|payload| (payload.validator_index as u32) < protocol_settings.validators_count)
            && self
                .preparation_messages
                .values()
                .all(|payload| (payload.validator_index as u32) < protocol_settings.validators_count)
            && self
                .commit_messages
                .values()
                .all(|payload| (payload.validator_index as u32) < protocol_settings.validators_count)
    }
}

fn serialized_compact_map_size<T>(
    map: &HashMap<u8, T>,
    element_size: impl Fn(&T) -> usize,
) -> usize {
    var_int_size(map.len()) + map.values().map(element_size).sum::<usize>()
}

fn write_compact_map<T>(
    writer: &mut BinaryWriter,
    map: &HashMap<u8, T>,
    serialize: impl Fn(&T, &mut BinaryWriter) -> ConsensusMessageResult<()>,
) -> ConsensusMessageResult<()> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(validator, _)| *validator);
    writer.write_var_int(entries.len() as u64)?;
    for (_, payload) in entries {
        serialize(payload, writer)?;
    }
    Ok(())
}

fn read_compact_map<T>(
    reader: &mut MemoryReader,
    max_count: u64,
    deserialize: impl Fn(&mut MemoryReader) -> ConsensusMessageResult<T>,
    key_selector: impl Fn(&T) -> u8,
) -> ConsensusMessageResult<HashMap<u8, T>> {
    let count = reader.read_var_int(max_count)? as usize;
    let mut map = HashMap::with_capacity(count);
    for _ in 0..count {
        let payload = deserialize(reader)?;
        let key = key_selector(&payload);
        if map.insert(key, payload).is_some() {
            return Err(ConsensusMessageError::invalid_data(
                "RecoveryMessage contains duplicate payloads for a validator",
            ));
        }
    }
    Ok(map)
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
