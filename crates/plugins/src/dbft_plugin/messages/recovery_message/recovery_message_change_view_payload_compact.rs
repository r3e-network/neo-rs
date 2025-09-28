// Copyright (C) 2015-2025 The Neo Project.
//
// recovery_message_change_view_payload_compact.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::messages::consensus_message::ConsensusMessageError;
use neo_core::neo_io::{BinaryWriter, MemoryReader};

const MAX_INVOCATION_SCRIPT: usize = 1024;

/// Compact representation of a ChangeView payload used inside RecoveryMessage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeViewPayloadCompact {
    pub validator_index: u8,
    pub original_view_number: u8,
    pub timestamp: u64,
    pub invocation_script: Vec<u8>,
}

impl ChangeViewPayloadCompact {
    /// Creates a new compact payload.
    pub fn new(
        validator_index: u8,
        original_view_number: u8,
        timestamp: u64,
        invocation_script: Vec<u8>,
    ) -> Self {
        Self {
            validator_index,
            original_view_number,
            timestamp,
            invocation_script,
        }
    }

    /// Returns the serialized size of the compact payload.
    pub fn size(&self) -> usize {
        1 + 1 + 8 + var_bytes_size(self.invocation_script.len())
    }

    /// Serializes the compact payload.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), ConsensusMessageError> {
        if self.invocation_script.len() > MAX_INVOCATION_SCRIPT {
            return Err(ConsensusMessageError::invalid_data(
                "Invocation script in ChangeViewPayloadCompact exceeds maximum length",
            ));
        }

        writer.write_u8(self.validator_index)?;
        writer.write_u8(self.original_view_number)?;
        writer.write_u64(self.timestamp)?;
        writer.write_var_bytes(&self.invocation_script)?;
        Ok(())
    }

    /// Deserializes a compact payload from the reader.
    pub fn deserialize(reader: &mut MemoryReader) -> Result<Self, ConsensusMessageError> {
        let validator_index = reader.read_u8()?;
        let original_view_number = reader.read_u8()?;
        let timestamp = reader.read_u64()?;
        let invocation_script = reader.read_var_bytes_max(MAX_INVOCATION_SCRIPT)?;
        Ok(Self {
            validator_index,
            original_view_number,
            timestamp,
            invocation_script,
        })
    }
}

fn var_bytes_size(length: usize) -> usize {
    var_int_size(length) + length
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
