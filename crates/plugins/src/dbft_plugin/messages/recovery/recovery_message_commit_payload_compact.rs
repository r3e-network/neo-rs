// Copyright (C) 2015-2025 The Neo Project.
//
// recovery_message_commit_payload_compact.rs file belongs to the neo project and is free
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
const SIGNATURE_LENGTH: usize = 64;

/// Compact representation of a Commit payload used inside RecoveryMessage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPayloadCompact {
    pub view_number: u8,
    pub validator_index: u8,
    pub signature: Vec<u8>,
    pub invocation_script: Vec<u8>,
}

impl CommitPayloadCompact {
    /// Creates a new compact commit payload.
    pub fn new(
        view_number: u8,
        validator_index: u8,
        signature: Vec<u8>,
        invocation_script: Vec<u8>,
    ) -> Result<Self, ConsensusMessageError> {
        if signature.len() != SIGNATURE_LENGTH {
            return Err(ConsensusMessageError::invalid_data(format!(
                "CommitPayloadCompact signature must be {SIGNATURE_LENGTH} bytes, got {}",
                signature.len()
            )));
        }
        if invocation_script.len() > MAX_INVOCATION_SCRIPT {
            return Err(ConsensusMessageError::invalid_data(
                "Invocation script in CommitPayloadCompact exceeds maximum length",
            ));
        }

        Ok(Self {
            view_number,
            validator_index,
            signature,
            invocation_script,
        })
    }

    /// Returns the serialized size of the compact payload.
    pub fn size(&self) -> usize {
        1 + 1 + self.signature.len() + var_bytes_size(self.invocation_script.len())
    }

    /// Serializes the compact payload.
    pub fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), ConsensusMessageError> {
        if self.signature.len() != SIGNATURE_LENGTH {
            return Err(ConsensusMessageError::invalid_data(
                "CommitPayloadCompact signature length mismatch",
            ));
        }
        if self.invocation_script.len() > MAX_INVOCATION_SCRIPT {
            return Err(ConsensusMessageError::invalid_data(
                "Invocation script in CommitPayloadCompact exceeds maximum length",
            ));
        }

        writer.write_u8(self.view_number)?;
        writer.write_u8(self.validator_index)?;
        writer.write_bytes(&self.signature)?;
        writer.write_var_bytes(&self.invocation_script)?;
        Ok(())
    }

    /// Deserializes a compact payload from the reader.
    pub fn deserialize(reader: &mut MemoryReader) -> Result<Self, ConsensusMessageError> {
        let view_number = reader.read_u8()?;
        let validator_index = reader.read_u8()?;
        let signature = reader.read_bytes(SIGNATURE_LENGTH)?;
        let invocation_script = reader.read_var_bytes_max(MAX_INVOCATION_SCRIPT)?;
        Self::new(view_number, validator_index, signature, invocation_script)
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
