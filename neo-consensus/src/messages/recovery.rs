//! Recovery messages - for consensus state recovery.

use crate::{ConsensusMessageType, ConsensusResult};
use neo_io::serializable::helper::SerializeHelper;
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
        1 + 1 + 8 + SerializeHelper::get_var_size_bytes(&self.invocation_script)
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
        1 + SerializeHelper::get_var_size_bytes(&self.invocation_script)
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
        1 + 1 + 64 + SerializeHelper::get_var_size_bytes(&self.invocation_script)
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
    ///
    /// Returns `Err` if the underlying `BinaryWriter` fails. The in-memory
    /// `BinaryWriter` cannot fail in practice, but the `?` propagation here
    /// ensures that any future writer change (e.g. a streaming sink) surfaces
    /// an `IoError` instead of silently producing a truncated/malformed
    /// recovery message.
    pub fn serialize(&self) -> ConsensusResult<Vec<u8>> {
        let mut writer = BinaryWriter::new();

        // The three compact arrays are serialized in ASCENDING validator-index
        // order to match C# byte-for-byte (NOT a divergence): C#
        // `MakeRecoveryMessage` builds each `Dictionary<byte, ...Compact>` from a
        // validator-indexed payload array via
        // `.Where(p => p != null).ToDictionary(p => p.ValidatorIndex)` — so the
        // dictionary's insertion order is ascending validator index — and
        // `Serialize` writes `.Values.ToArray()` in that insertion order
        // (RecoveryMessage.cs:117/130-131, ConsensusContext.MakePayload.cs:149-165).
        // Sorting here yields that same deterministic order regardless of our
        // source collection's iteration order; removing it would break parity.

        // ChangeViewMessages (serializable array)
        let mut cvs = self.change_view_messages.clone();
        cvs.sort_by_key(|p| p.validator_index);
        SerializeHelper::serialize_array(&cvs, &mut writer).map_err(writer_error)?;

        // PrepareRequestMessage presence flag + value OR PreparationHash var-bytes.
        let has_prepare_request = self.prepare_request_message.is_some();
        writer.write_bool(has_prepare_request).map_err(writer_error)?;
        if let Some(ref req) = self.prepare_request_message {
            // Embedded message includes its own common header.
            let bytes = super::consensus_message_bytes(
                ConsensusMessageType::PrepareRequest,
                req.block_index,
                req.validator_index,
                req.view_number,
                &req.serialize(),
            );
            writer.write_bytes(&bytes).map_err(writer_error)?;
        } else if let Some(hash) = self.preparation_hash {
            writer.write_var_bytes(&hash.to_bytes()).map_err(writer_error)?;
        } else {
            writer.write_var_int(0).map_err(writer_error)?;
        }

        // PreparationMessages (serializable array)
        let mut preps = self.preparation_messages.clone();
        preps.sort_by_key(|p| p.validator_index);
        SerializeHelper::serialize_array(&preps, &mut writer).map_err(writer_error)?;

        // CommitMessages (serializable array)
        let mut commits = self.commit_messages.clone();
        commits.sort_by_key(|p| p.validator_index);
        SerializeHelper::serialize_array(&commits, &mut writer).map_err(writer_error)?;

        Ok(writer.into_bytes())
    }

    /// Deserializes a `RecoveryMessage` from bytes (body only, excluding the common header).
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        let mut reader = MemoryReader::new(data);

        let change_view_messages = SerializeHelper::deserialize_array::<ChangeViewPayloadCompact>(
            &mut reader,
            u8::MAX as usize,
        )
        .map_err(|_| crate::ConsensusError::invalid_proposal("RecoveryMessage change views"))?;

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

        let preparation_messages = SerializeHelper::deserialize_array::<PreparationPayloadCompact>(
            &mut reader,
            u8::MAX as usize,
        )
        .map_err(|_| crate::ConsensusError::invalid_proposal("RecoveryMessage preparations"))?;
        let commit_messages = SerializeHelper::deserialize_array::<CommitPayloadCompact>(
            &mut reader,
            u8::MAX as usize,
        )
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

    /// Validates the message using C# DBFTPlugin `RecoveryMessage.Verify` rules.
    pub fn validate(
        &self,
        validator_count: usize,
        max_transactions_per_block: u32,
    ) -> ConsensusResult<()> {
        validate_compact_validators(
            self.change_view_messages.iter().map(|p| p.validator_index),
            validator_count,
        )?;
        validate_compact_validators(
            self.preparation_messages.iter().map(|p| p.validator_index),
            validator_count,
        )?;
        validate_compact_validators(
            self.commit_messages.iter().map(|p| p.validator_index),
            validator_count,
        )?;

        if let Some(request) = &self.prepare_request_message {
            if request.validator_index as usize >= validator_count {
                return Err(crate::ConsensusError::InvalidValidatorIndex(
                    request.validator_index,
                ));
            }
            if request.transaction_hashes.len() > max_transactions_per_block as usize {
                return Err(crate::ConsensusError::invalid_proposal(
                    "RecoveryMessage PrepareRequest exceeds MaxTransactionsPerBlock",
                ));
            }

            let mut hashes =
                std::collections::HashSet::with_capacity(request.transaction_hashes.len());
            for hash in &request.transaction_hashes {
                if !hashes.insert(*hash) {
                    return Err(crate::ConsensusError::invalid_proposal(
                        "RecoveryMessage PrepareRequest transaction hashes are duplicate",
                    ));
                }
            }
        }

        Ok(())
    }
}

fn validate_compact_validators<I>(indices: I, validator_count: usize) -> ConsensusResult<()>
where
    I: IntoIterator<Item = u8>,
{
    let mut seen = std::collections::HashSet::new();
    for index in indices {
        if index as usize >= validator_count {
            return Err(crate::ConsensusError::InvalidValidatorIndex(index));
        }
        if !seen.insert(index) {
            return Err(crate::ConsensusError::DuplicateValidator(index));
        }
    }
    Ok(())
}

/// Maps a `neo_io::IoError` from the in-memory `BinaryWriter` into a
/// `ConsensusError`. The current writer writes to a `Vec<u8>` and cannot
/// fail, so this is effectively unreachable — but it keeps `serialize`
/// correct under a future writer change rather than silently dropping the
/// failure the way the old `let _ = ...` pattern did.
fn writer_error(err: neo_io::IoError) -> crate::ConsensusError {
    crate::ConsensusError::SerializationError(format!("RecoveryMessage write failed: {err}"))
}

#[cfg(test)]
#[path = "../tests/messages/recovery.rs"]
mod tests;
