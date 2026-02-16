//! Recovery messages - for consensus state recovery.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_io::serializable::helper::{deserialize_array, get_var_size_bytes, serialize_array};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Maximum invocation script size accepted by `DBFTPlugin` compact payloads (bytes).
const MAX_INVOCATION_SCRIPT: usize = 1024;

/// `RecoveryRequest` message sent when a validator needs to recover consensus state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequestMessage {
    /// Block index
    pub block_index: u32,
    /// View number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Timestamp of the request
    pub timestamp: u64,
}

impl RecoveryRequestMessage {
    /// Creates a new `RecoveryRequest` message.
    #[must_use]
    pub const fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        timestamp: u64,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
        }
    }

    /// Returns the message type.
    #[must_use]
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::RecoveryRequest
    }

    /// Serializes the message body to bytes (excluding the common header).
    ///
    /// Neo N3 `DBFTPlugin` format: `timestamp (8)`.
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        self.timestamp.to_le_bytes().to_vec()
    }
}

/// Compact representation of a `ChangeView` payload (`RecoveryMessage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeViewPayloadCompact {
    /// Index of the validator that sent this change view.
    pub validator_index: u8,
    /// Original view number before the change.
    pub original_view_number: u8,
    /// Timestamp of the change view request.
    pub timestamp: u64,
    /// Invocation script for verification.
    pub invocation_script: Vec<u8>,
}

impl Serializable for ChangeViewPayloadCompact {
    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        let validator_index = reader.read_u8()?;
        let original_view_number = reader.read_u8()?;
        let timestamp = reader.read_u64()?;
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;
        Ok(Self {
            validator_index,
            original_view_number,
            timestamp,
            invocation_script,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_u8(self.validator_index)?;
        writer.write_u8(self.original_view_number)?;
        writer.write_u64(self.timestamp)?;
        writer.write_var_bytes(&self.invocation_script)?;
        Ok(())
    }

    fn size(&self) -> usize {
        1 + 1 + 8 + get_var_size_bytes(&self.invocation_script)
    }
}

/// Compact representation of a `PrepareResponse` payload (`RecoveryMessage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparationPayloadCompact {
    /// Index of the validator that sent this preparation.
    pub validator_index: u8,
    /// Invocation script for verification.
    pub invocation_script: Vec<u8>,
}

impl Serializable for PreparationPayloadCompact {
    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        let validator_index = reader.read_u8()?;
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;
        Ok(Self {
            validator_index,
            invocation_script,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_u8(self.validator_index)?;
        writer.write_var_bytes(&self.invocation_script)?;
        Ok(())
    }

    fn size(&self) -> usize {
        1 + get_var_size_bytes(&self.invocation_script)
    }
}

/// Compact representation of a Commit payload (`RecoveryMessage`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitPayloadCompact {
    /// View number for this commit.
    pub view_number: u8,
    /// Index of the validator that sent this commit.
    pub validator_index: u8,
    /// Signature of the commit.
    pub signature: Vec<u8>,
    /// Invocation script for verification.
    pub invocation_script: Vec<u8>,
}

impl Serializable for CommitPayloadCompact {
    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        let view_number = reader.read_u8()?;
        let validator_index = reader.read_u8()?;
        let signature = reader.read_bytes(64)?;
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;
        Ok(Self {
            view_number,
            validator_index,
            signature,
            invocation_script,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_u8(self.view_number)?;
        writer.write_u8(self.validator_index)?;
        // Commit.Signature is fixed 64 bytes (secp256r1 r||s).
        if self.signature.len() != 64 {
            return Err(neo_io::IoError::invalid_data(
                "CommitPayloadCompact signature must be 64 bytes",
            ));
        }
        writer.write_bytes(&self.signature)?;
        writer.write_var_bytes(&self.invocation_script)?;
        Ok(())
    }

    fn size(&self) -> usize {
        1 + 1 + 64 + get_var_size_bytes(&self.invocation_script)
    }
}

/// `RecoveryMessage` sent in response to a `RecoveryRequest`.
///
/// This struct models the Neo N3 `DBFTPlugin` on-wire format (message body only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMessage {
    /// Block index for this recovery.
    pub block_index: u32,
    /// Current view number.
    pub view_number: u8,
    /// Index of the validator sending recovery.
    pub validator_index: u8,

    /// Change view messages collected.
    pub change_view_messages: Vec<ChangeViewPayloadCompact>,

    /// Embedded `PrepareRequest` message (including its common header) when available.
    pub prepare_request_message: Option<super::PrepareRequestMessage>,

    /// `PreparationHash` (ExtensiblePayload.Hash of the primary `PrepareRequest`) when `PrepareRequest` is missing.
    pub preparation_hash: Option<UInt256>,

    /// Preparation messages collected from validators.
    pub preparation_messages: Vec<PreparationPayloadCompact>,
    /// Commit messages collected from validators.
    pub commit_messages: Vec<CommitPayloadCompact>,
}

impl RecoveryMessage {
    /// Creates a new empty `RecoveryMessage`.
    #[must_use]
    pub const fn new(block_index: u32, view_number: u8, validator_index: u8) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            change_view_messages: Vec::new(),
            prepare_request_message: None,
            preparation_hash: None,
            preparation_messages: Vec::new(),
            commit_messages: Vec::new(),
        }
    }

    /// Returns the message type.
    #[must_use]
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::RecoveryMessage
    }

    /// Serializes the message body to bytes (excluding the common header).
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();

        // ChangeViewMessages (serializable array)
        let mut cvs = self.change_view_messages.clone();
        cvs.sort_by_key(|p| p.validator_index);
        let _ = serialize_array(&cvs, &mut writer);

        // PrepareRequestMessage presence flag + value OR PreparationHash var-bytes.
        let has_prepare_request = self.prepare_request_message.is_some();
        let _ = writer.write_bool(has_prepare_request);
        if let Some(ref req) = self.prepare_request_message {
            // Embedded message includes its own common header.
            let bytes = super::consensus_message_bytes(
                ConsensusMessageType::PrepareRequest,
                req.block_index,
                req.validator_index,
                req.view_number,
                &req.serialize(),
            );
            let _ = writer.write_bytes(&bytes);
        } else if let Some(hash) = self.preparation_hash {
            let _ = writer.write_var_bytes(&hash.to_bytes());
        } else {
            let _ = writer.write_var_int(0);
        }

        // PreparationMessages (serializable array)
        let mut preps = self.preparation_messages.clone();
        preps.sort_by_key(|p| p.validator_index);
        let _ = serialize_array(&preps, &mut writer);

        // CommitMessages (serializable array)
        let mut commits = self.commit_messages.clone();
        commits.sort_by_key(|p| p.validator_index);
        let _ = serialize_array(&commits, &mut writer);

        writer.into_bytes()
    }

    /// Deserializes a `RecoveryMessage` from bytes (body only, excluding the common header).
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        let mut reader = MemoryReader::new(data);

        let change_view_messages =
            deserialize_array::<ChangeViewPayloadCompact>(&mut reader, u8::MAX as usize).map_err(
                |_| crate::ConsensusError::invalid_proposal("RecoveryMessage change views"),
            )?;

        let has_prepare_request = reader
            .read_bool()
            .map_err(|_| crate::ConsensusError::invalid_proposal("RecoveryMessage flag"))?;

        let (prepare_request_message, preparation_hash) = if has_prepare_request {
            let req = super::PrepareRequestMessage::deserialize_from_reader(&mut reader)?;
            (Some(req), None)
        } else {
            let len = reader.read_var_int(UInt256::LENGTH as u64).map_err(|_| {
                crate::ConsensusError::invalid_proposal("RecoveryMessage PreparationHash length")
            })? as usize;
            if len == 0 {
                (None, None)
            } else if len == UInt256::LENGTH {
                let hash = <UInt256 as Serializable>::deserialize(&mut reader).map_err(|_| {
                    crate::ConsensusError::invalid_proposal("RecoveryMessage PreparationHash")
                })?;
                (None, Some(hash))
            } else {
                return Err(crate::ConsensusError::invalid_proposal(
                    "RecoveryMessage PreparationHash length invalid",
                ));
            }
        };

        let preparation_messages =
            deserialize_array::<PreparationPayloadCompact>(&mut reader, u8::MAX as usize).map_err(
                |_| crate::ConsensusError::invalid_proposal("RecoveryMessage preparations"),
            )?;
        let commit_messages =
            deserialize_array::<CommitPayloadCompact>(&mut reader, u8::MAX as usize)
                .map_err(|_| crate::ConsensusError::invalid_proposal("RecoveryMessage commits"))?;

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            change_view_messages,
            prepare_request_message,
            preparation_hash,
            preparation_messages,
            commit_messages,
        })
    }

    /// Basic validation: ensures no duplicate validator indices in preparation messages.
    pub fn validate(&self) -> ConsensusResult<()> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.preparation_messages {
            if !seen.insert(p.validator_index) {
                return Err(crate::ConsensusError::DuplicateValidator(p.validator_index));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_request_serializes_timestamp_only() {
        let msg = RecoveryRequestMessage::new(1, 0, 0, 1234);
        let bytes = msg.serialize();
        assert_eq!(bytes.len(), 8);
        assert_eq!(u64::from_le_bytes(bytes.try_into().unwrap()), 1234);
    }

    #[test]
    fn recovery_message_roundtrip_minimal_without_prepare_request() {
        let mut msg = RecoveryMessage::new(100, 0, 1);
        msg.preparation_hash = Some(UInt256::from([0xAB; 32]));
        msg.preparation_messages.push(PreparationPayloadCompact {
            validator_index: 0,
            invocation_script: vec![0x0C, 0x40, 0xAA],
        });
        msg.commit_messages.push(CommitPayloadCompact {
            view_number: 0,
            validator_index: 0,
            signature: vec![0x11; 64],
            invocation_script: vec![0x0C, 0x40, 0xBB],
        });

        let bytes = msg.serialize();
        let parsed = RecoveryMessage::deserialize(&bytes, 100, 0, 1).unwrap();
        assert!(parsed.prepare_request_message.is_none());
        assert_eq!(parsed.preparation_hash, msg.preparation_hash);
        assert_eq!(parsed.preparation_messages.len(), 1);
        assert_eq!(parsed.commit_messages.len(), 1);
    }

    #[test]
    fn recovery_message_wire_format_bytes_without_prepare_request() {
        let mut msg = RecoveryMessage::new(100, 0, 1);
        msg.change_view_messages.push(ChangeViewPayloadCompact {
            validator_index: 2,
            original_view_number: 1,
            timestamp: 0x0102_0304_0506_0708u64,
            invocation_script: vec![0xAA, 0xBB],
        });
        let prep_hash = UInt256::from([0xCC; 32]);
        msg.preparation_hash = Some(prep_hash);
        msg.preparation_messages.push(PreparationPayloadCompact {
            validator_index: 3,
            invocation_script: vec![0xDD],
        });
        msg.commit_messages.push(CommitPayloadCompact {
            view_number: 0,
            validator_index: 4,
            signature: vec![0xEE; 64],
            invocation_script: vec![0xFF, 0x00],
        });

        let bytes = msg.serialize();

        // Build expected bytes by following the C# RecoveryMessage serialization layout.
        let mut expected = Vec::new();
        let prep_hash_bytes = prep_hash.to_array();

        // ChangeViewMessages array (count=1)
        expected.push(0x01);
        expected.push(2); // validator_index
        expected.push(1); // original_view_number
        expected.extend_from_slice(&0x0102_0304_0506_0708u64.to_le_bytes());
        expected.push(0x02); // varbytes len
        expected.extend_from_slice(&[0xAA, 0xBB]);

        // hasPrepareRequestMessage = false
        expected.push(0x00);

        // PreparationHash as varbytes (len=32)
        expected.push(0x20);
        expected.extend_from_slice(&prep_hash_bytes);

        // PreparationMessages array (count=1)
        expected.push(0x01);
        expected.push(3); // validator_index
        expected.push(0x01); // invocation_script len
        expected.push(0xDD);

        // CommitMessages array (count=1)
        expected.push(0x01);
        expected.push(0x00); // view_number
        expected.push(4); // validator_index
        expected.extend(std::iter::repeat_n(0xEE, 64));
        expected.push(0x02); // invocation_script len
        expected.extend_from_slice(&[0xFF, 0x00]);

        assert_eq!(bytes, expected);
    }
}
