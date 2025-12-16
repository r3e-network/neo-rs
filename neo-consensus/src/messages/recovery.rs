//! Recovery messages - for consensus state recovery.

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
        self.timestamp.to_le_bytes().to_vec()
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
    /// Validator index
    pub validator_index: u8,
    /// Signature
    pub signature: Vec<u8>,
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
    pub prepare_request_message: Option<PrepareRequestCompact>,
    /// Preparation payloads (PrepareResponses)
    pub preparation_payloads: Vec<PreparationCompact>,
    /// Commit payloads
    pub commit_payloads: Vec<CommitCompact>,
}

/// Compact PrepareRequest for recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareRequestCompact {
    /// Timestamp
    pub timestamp: u64,
    /// Nonce
    pub nonce: u64,
    /// Transaction hashes
    pub transaction_hashes: Vec<UInt256>,
    /// Invocation script
    pub invocation_script: Vec<u8>,
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
        // Binary serialization format for recovery message
        let mut data = Vec::new();

        // Change views count
        data.push(self.change_view_payloads.len() as u8);
        for cv in &self.change_view_payloads {
            data.push(cv.validator_index);
            data.push(cv.original_view_number);
            data.extend_from_slice(&cv.timestamp.to_le_bytes());
        }

        // Has prepare request flag and data
        if let Some(ref prep_req) = self.prepare_request_message {
            data.push(1);
            // Serialize PrepareRequestCompact: timestamp (8) + nonce (8) + tx_count (1) + tx_hashes (32 each)
            data.extend_from_slice(&prep_req.timestamp.to_le_bytes());
            data.extend_from_slice(&prep_req.nonce.to_le_bytes());
            data.push(prep_req.transaction_hashes.len() as u8);
            for tx_hash in &prep_req.transaction_hashes {
                data.extend_from_slice(&tx_hash.to_bytes());
            }
        } else {
            data.push(0);
        }

        // Preparations count
        data.push(self.preparation_payloads.len() as u8);
        for prep in &self.preparation_payloads {
            data.push(prep.validator_index);
        }

        // Commits count
        data.push(self.commit_payloads.len() as u8);
        for commit in &self.commit_payloads {
            data.push(commit.validator_index);
            data.extend_from_slice(&commit.signature);
        }

        data
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
    pub fn deserialize(data: &[u8], block_index: u32, view_number: u8, validator_index: u8) -> ConsensusResult<Self> {
        if data.is_empty() {
            return Err(crate::ConsensusError::invalid_proposal(
                "Empty recovery message data",
            ));
        }

        let mut offset = 0;

        // Parse change views count
        let change_view_count = data.get(offset).copied().unwrap_or(0) as usize;
        offset += 1;

        let mut change_view_payloads = Vec::with_capacity(change_view_count);
        for _ in 0..change_view_count {
            if offset + 10 > data.len() {
                break;
            }
            let cv_validator = data[offset];
            let cv_view = data[offset + 1];
            let cv_timestamp = u64::from_le_bytes(
                data[offset + 2..offset + 10].try_into().unwrap_or([0u8; 8])
            );
            offset += 10;

            change_view_payloads.push(ChangeViewCompact {
                validator_index: cv_validator,
                original_view_number: cv_view,
                timestamp: cv_timestamp,
                invocation_script: Vec::new(),
            });
        }

        // Parse has prepare request flag and data
        let has_prepare_request = data.get(offset).copied().unwrap_or(0) == 1;
        offset += 1;

        let prepare_request_message = if has_prepare_request {
            // Parse PrepareRequestCompact: timestamp (8) + nonce (8) + tx_count (1) + tx_hashes (32 each)
            if offset + 17 > data.len() {
                // Not enough data for timestamp + nonce + tx_count
                None
            } else {
                let timestamp = u64::from_le_bytes(
                    data[offset..offset + 8].try_into().unwrap_or([0u8; 8])
                );
                offset += 8;

                let nonce = u64::from_le_bytes(
                    data[offset..offset + 8].try_into().unwrap_or([0u8; 8])
                );
                offset += 8;

                let tx_count = data[offset] as usize;
                offset += 1;

                let mut transaction_hashes = Vec::with_capacity(tx_count);
                for _ in 0..tx_count {
                    if offset + 32 > data.len() {
                        break;
                    }
                    if let Ok(hash) = UInt256::from_bytes(&data[offset..offset + 32]) {
                        transaction_hashes.push(hash);
                    }
                    offset += 32;
                }

                Some(PrepareRequestCompact {
                    timestamp,
                    nonce,
                    transaction_hashes,
                    invocation_script: Vec::new(),
                })
            }
        } else {
            None
        };

        // Parse preparations count
        let prep_count = data.get(offset).copied().unwrap_or(0) as usize;
        offset += 1;

        let mut preparation_payloads = Vec::with_capacity(prep_count);
        for _ in 0..prep_count {
            if offset >= data.len() {
                break;
            }
            let prep_validator = data[offset];
            offset += 1;

            preparation_payloads.push(PreparationCompact {
                validator_index: prep_validator,
                invocation_script: Vec::new(),
            });
        }

        // Parse commits count
        let commit_count = data.get(offset).copied().unwrap_or(0) as usize;
        offset += 1;

        let mut commit_payloads = Vec::with_capacity(commit_count);
        for _ in 0..commit_count {
            if offset >= data.len() {
                break;
            }
            let commit_validator = data[offset];
            offset += 1;

            // Read 64-byte secp256r1 ECDSA signature
            let sig_len = 64.min(data.len().saturating_sub(offset));
            let signature = if sig_len > 0 {
                data[offset..offset + sig_len].to_vec()
            } else {
                Vec::new()
            };
            offset += sig_len;

            commit_payloads.push(CommitCompact {
                validator_index: commit_validator,
                signature,
            });
        }

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            change_view_payloads,
            prepare_request_message,
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
        msg.prepare_request_message = Some(PrepareRequestCompact {
            timestamp: 1000000,
            nonce: 0xDEADBEEF,
            transaction_hashes: vec![
                UInt256::from_bytes(&[1u8; 32]).unwrap(),
                UInt256::from_bytes(&[2u8; 32]).unwrap(),
            ],
            invocation_script: vec![],
        });

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
            validator_index: 0,
            signature: vec![0xAAu8; 64],
        });

        // Serialize
        let data = msg.serialize();

        // Deserialize
        let parsed = RecoveryMessage::deserialize(&data, 100, 0, 1).unwrap();

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
        let parsed = RecoveryMessage::deserialize(&data, 50, 1, 3).unwrap();

        assert!(parsed.prepare_request_message.is_none());
        assert_eq!(parsed.preparation_payloads.len(), 1);
    }
}
