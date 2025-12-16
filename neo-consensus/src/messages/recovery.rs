//! Recovery messages - for consensus state recovery.

use crate::messages::{parse_consensus_message_header, serialize_consensus_message_header};
use crate::{ConsensusMessageType, ConsensusResult};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// RecoveryRequest message sent when a validator needs to recover consensus state.
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
    /// Creates a new RecoveryRequest message
    pub fn new(block_index: u32, view_number: u8, validator_index: u8, timestamp: u64) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::RecoveryRequest
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = serialize_consensus_message_header(
            ConsensusMessageType::RecoveryRequest,
            self.block_index,
            self.validator_index,
            self.view_number,
        );
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out
    }

    /// Deserializes a RecoveryRequest message from bytes.
    pub fn deserialize(data: &[u8]) -> ConsensusResult<Self> {
        let (msg_type, block_index, validator_index, view_number, body) =
            parse_consensus_message_header(data)?;
        if msg_type != ConsensusMessageType::RecoveryRequest {
            return Err(crate::ConsensusError::invalid_proposal(
                "invalid RecoveryRequest message type",
            ));
        }
        if body.len() < 8 {
            return Err(crate::ConsensusError::invalid_proposal(
                "RecoveryRequest message body too short",
            ));
        }
        let timestamp = u64::from_le_bytes(body[0..8].try_into().unwrap_or([0u8; 8]));
        Ok(Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
        })
    }
}

/// Compact representation of a ChangeView for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeViewCompact {
    /// Validator index
    pub validator_index: u8,
    /// Original view number
    pub original_view_number: u8,
    /// Timestamp
    pub timestamp: u64,
    /// Invocation script (signature)
    pub invocation_script: Vec<u8>,
}

/// Compact representation of a PrepareResponse for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparationCompact {
    /// Validator index
    pub validator_index: u8,
    /// Invocation script (signature)
    pub invocation_script: Vec<u8>,
}

/// Compact representation of a Commit for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitCompact {
    /// View number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Signature
    pub signature: Vec<u8>,
    /// Invocation script (extensible payload witness)
    pub invocation_script: Vec<u8>,
}

/// RecoveryMessage sent in response to a RecoveryRequest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryMessage {
    /// Block index
    pub block_index: u32,
    /// View number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Change view payloads
    pub change_view_payloads: Vec<ChangeViewCompact>,
    /// Prepare request message (if received)
    pub prepare_request_message: Option<crate::messages::PrepareRequestMessage>,
    /// PreparationHash if PrepareRequest isn't included
    pub preparation_hash: Option<UInt256>,
    /// Preparation payloads (PrepareResponses)
    pub preparation_payloads: Vec<PreparationCompact>,
    /// Commit payloads
    pub commit_payloads: Vec<CommitCompact>,
}

impl RecoveryMessage {
    /// Creates a new empty RecoveryMessage
    pub fn new(block_index: u32, view_number: u8, validator_index: u8) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            change_view_payloads: Vec::new(),
            prepare_request_message: None,
            preparation_hash: None,
            preparation_payloads: Vec::new(),
            commit_payloads: Vec::new(),
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::RecoveryMessage
    }

    /// Serializes the message to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = serialize_consensus_message_header(
            ConsensusMessageType::RecoveryMessage,
            self.block_index,
            self.validator_index,
            self.view_number,
        );

        let mut writer = neo_io::BinaryWriter::new();

        let _ = writer.write_var_int(self.change_view_payloads.len() as u64);
        for cv in &self.change_view_payloads {
            let _ = writer.write_u8(cv.validator_index);
            let _ = writer.write_u8(cv.original_view_number);
            let _ = writer.write_u64(cv.timestamp);
            let _ = writer.write_var_bytes(&cv.invocation_script);
        }

        let has_prepare_request = self.prepare_request_message.is_some();
        let _ = writer.write_bool(has_prepare_request);
        if let Some(ref pr) = self.prepare_request_message {
            let bytes = pr.serialize();
            let _ = writer.write_bytes(&bytes);
        } else if let Some(hash) = self.preparation_hash {
            let _ = writer.write_var_bytes(&hash.to_array());
        } else {
            let _ = writer.write_var_int(0);
        }

        let _ = writer.write_var_int(self.preparation_payloads.len() as u64);
        for prep in &self.preparation_payloads {
            let _ = writer.write_u8(prep.validator_index);
            let _ = writer.write_var_bytes(&prep.invocation_script);
        }

        let _ = writer.write_var_int(self.commit_payloads.len() as u64);
        for commit in &self.commit_payloads {
            let _ = writer.write_u8(commit.view_number);
            let _ = writer.write_u8(commit.validator_index);
            let _ = writer.write_bytes(&commit.signature);
            let _ = writer.write_var_bytes(&commit.invocation_script);
        }

        out.extend_from_slice(&writer.into_bytes());
        out
    }

    /// Validates the recovery message
    pub fn validate(&self) -> ConsensusResult<()> {
        // Basic validation - ensure no duplicate validator indices
        let mut seen_validators = std::collections::HashSet::new();
        for prep in &self.preparation_payloads {
            if !seen_validators.insert(prep.validator_index) {
                return Err(crate::ConsensusError::DuplicateValidator(
                    prep.validator_index,
                ));
            }
        }
        Ok(())
    }

    /// Deserializes a RecoveryMessage from bytes
    pub fn deserialize(data: &[u8]) -> ConsensusResult<Self> {
        let (msg_type, block_index, validator_index, view_number, body) =
            parse_consensus_message_header(data)?;
        if msg_type != ConsensusMessageType::RecoveryMessage {
            return Err(crate::ConsensusError::invalid_proposal(
                "invalid RecoveryMessage message type",
            ));
        }

        let mut reader = neo_io::MemoryReader::new(body);

        let change_view_count = reader
            .read_var_int(u8::MAX as u64)
            .map_err(|_| crate::ConsensusError::invalid_proposal("invalid change view count"))?
            as usize;
        let mut change_view_payloads = Vec::with_capacity(change_view_count);
        for _ in 0..change_view_count {
            let v = reader
                .read_byte()
                .map_err(|_| crate::ConsensusError::invalid_proposal("change view validator missing"))?;
            let ov = reader
                .read_byte()
                .map_err(|_| crate::ConsensusError::invalid_proposal("change view original view missing"))?;
            let ts = reader
                .read_u64()
                .map_err(|_| crate::ConsensusError::invalid_proposal("change view timestamp missing"))?;
            let inv = reader
                .read_var_bytes(1024)
                .map_err(|_| crate::ConsensusError::invalid_proposal("change view invocation missing"))?;
            change_view_payloads.push(ChangeViewCompact {
                validator_index: v,
                original_view_number: ov,
                timestamp: ts,
                invocation_script: inv,
            });
        }

        let has_prepare_request = reader
            .read_bool()
            .map_err(|_| crate::ConsensusError::invalid_proposal("invalid prepare request flag"))?;

        let (prepare_request_message, preparation_hash) = if has_prepare_request {
            let pr = crate::messages::PrepareRequestMessage::deserialize_from_reader(&mut reader)?;
            (Some(pr), None)
        } else {
            let len = reader
                .read_var_int(32)
                .map_err(|_| crate::ConsensusError::invalid_proposal("invalid preparation hash length"))?
                as usize;
            if len == 0 {
                (None, None)
            } else if len == 32 {
                let bytes = reader
                    .read_memory(32)
                    .map_err(|_| crate::ConsensusError::invalid_proposal("preparation hash missing"))?;
                let hash = UInt256::from_bytes(bytes)
                    .map_err(|_| crate::ConsensusError::invalid_proposal("invalid preparation hash"))?;
                (None, Some(hash))
            } else {
                return Err(crate::ConsensusError::invalid_proposal(
                    "invalid preparation hash length",
                ));
            }
        };

        let prep_count = reader
            .read_var_int(u8::MAX as u64)
            .map_err(|_| crate::ConsensusError::invalid_proposal("invalid preparation count"))?
            as usize;
        let mut preparation_payloads = Vec::with_capacity(prep_count);
        for _ in 0..prep_count {
            let v = reader
                .read_byte()
                .map_err(|_| crate::ConsensusError::invalid_proposal("preparation validator missing"))?;
            let inv = reader
                .read_var_bytes(1024)
                .map_err(|_| crate::ConsensusError::invalid_proposal("preparation invocation missing"))?;
            preparation_payloads.push(PreparationCompact {
                validator_index: v,
                invocation_script: inv,
            });
        }

        let commit_count = reader
            .read_var_int(u8::MAX as u64)
            .map_err(|_| crate::ConsensusError::invalid_proposal("invalid commit count"))?
            as usize;
        let mut commit_payloads = Vec::with_capacity(commit_count);
        for _ in 0..commit_count {
            let cv = reader
                .read_byte()
                .map_err(|_| crate::ConsensusError::invalid_proposal("commit view missing"))?;
            let vi = reader
                .read_byte()
                .map_err(|_| crate::ConsensusError::invalid_proposal("commit validator missing"))?;
            let sig = reader
                .read_memory(64)
                .map_err(|_| crate::ConsensusError::invalid_proposal("commit signature missing"))?
                .to_vec();
            let inv = reader
                .read_var_bytes(1024)
                .map_err(|_| crate::ConsensusError::invalid_proposal("commit invocation missing"))?;
            commit_payloads.push(CommitCompact {
                view_number: cv,
                validator_index: vi,
                signature: sig,
                invocation_script: inv,
            });
        }

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            change_view_payloads,
            prepare_request_message,
            preparation_hash,
            preparation_payloads,
            commit_payloads,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_request_new() {
        let msg = RecoveryRequestMessage::new(100, 0, 1, 1000);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.timestamp, 1000);
    }

    #[test]
    fn test_recovery_message_new() {
        let msg = RecoveryMessage::new(100, 0, 1);

        assert_eq!(msg.block_index, 100);
        assert!(msg.change_view_payloads.is_empty());
        assert!(msg.prepare_request_message.is_none());
        assert!(msg.preparation_hash.is_none());
        assert!(msg.preparation_payloads.is_empty());
        assert!(msg.commit_payloads.is_empty());
    }

    #[test]
    fn test_recovery_message_validate() {
        let mut msg = RecoveryMessage::new(100, 0, 1);

        // Valid - no duplicates
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 0,
            invocation_script: vec![],
        });
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 1,
            invocation_script: vec![],
        });
        assert!(msg.validate().is_ok());

        // Invalid - duplicate
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 0,
            invocation_script: vec![],
        });
        assert!(msg.validate().is_err());
    }

    #[test]
    fn test_recovery_message_serialize_deserialize_roundtrip() {
        let mut msg = RecoveryMessage::new(100, 0, 1);

        // Add change view payloads
        msg.change_view_payloads.push(ChangeViewCompact {
            validator_index: 2,
            original_view_number: 0,
            timestamp: 12345678,
            invocation_script: vec![],
        });

        // Add prepare request
        msg.prepare_request_message = Some(crate::messages::PrepareRequestMessage::new(
            100,
            0,
            1,
            0,
            UInt256::zero(),
            1000000,
            0xDEADBEEF,
            vec![
                UInt256::from_bytes(&[1u8; 32]).unwrap(),
                UInt256::from_bytes(&[2u8; 32]).unwrap(),
            ],
        ));

        // Add preparation payloads
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 0,
            invocation_script: vec![],
        });
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 1,
            invocation_script: vec![],
        });

        // Add commit payloads
        msg.commit_payloads.push(CommitCompact {
            view_number: 0,
            validator_index: 0,
            signature: vec![0xAAu8; 64],
            invocation_script: vec![],
        });

        // Serialize
        let data = msg.serialize();

        // Deserialize
        let parsed = RecoveryMessage::deserialize(&data).unwrap();

        // Verify change views
        assert_eq!(parsed.change_view_payloads.len(), 1);
        assert_eq!(parsed.change_view_payloads[0].validator_index, 2);
        assert_eq!(parsed.change_view_payloads[0].original_view_number, 0);
        assert_eq!(parsed.change_view_payloads[0].timestamp, 12345678);

        // Verify prepare request
        assert!(parsed.prepare_request_message.is_some());
        let prep_req = parsed.prepare_request_message.unwrap();
        assert_eq!(prep_req.timestamp, 1000000);
        assert_eq!(prep_req.nonce, 0xDEADBEEF);
        assert_eq!(prep_req.transaction_hashes.len(), 2);

        // Verify preparations
        assert_eq!(parsed.preparation_payloads.len(), 2);
        assert_eq!(parsed.preparation_payloads[0].validator_index, 0);
        assert_eq!(parsed.preparation_payloads[1].validator_index, 1);

        // Verify commits
        assert_eq!(parsed.commit_payloads.len(), 1);
        assert_eq!(parsed.commit_payloads[0].validator_index, 0);
        assert_eq!(parsed.commit_payloads[0].signature.len(), 64);
    }

    #[test]
    fn test_recovery_message_without_prepare_request() {
        let mut msg = RecoveryMessage::new(50, 1, 3);

        // No prepare request
        msg.preparation_payloads.push(PreparationCompact {
            validator_index: 0,
            invocation_script: vec![],
        });

        let data = msg.serialize();
        let parsed = RecoveryMessage::deserialize(&data).unwrap();

        assert!(parsed.prepare_request_message.is_none());
        assert_eq!(parsed.preparation_payloads.len(), 1);
    }
}
