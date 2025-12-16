//! ChangeView message - request to change the current view.

use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use serde::{Deserialize, Serialize};

/// ChangeView message sent when a validator wants to change the view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeViewMessage {
    /// Block index
    pub block_index: u32,
    /// Current view number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// New view number being requested
    pub new_view_number: u8,
    /// Timestamp of the request
    pub timestamp: u64,
    /// Reason for the view change
    pub reason: ChangeViewReason,
}

impl ChangeViewMessage {
    /// Creates a new ChangeView message
    pub fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        new_view_number: u8,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            new_view_number,
            timestamp,
            reason,
        }
    }

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::ChangeView
    }

    /// Serializes the message to bytes
    /// Format: new_view_number (1) + timestamp (8) + reason (1)
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(self.new_view_number);
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.push(self.reason.to_byte());
        data
    }

    /// Deserializes a ChangeView message from bytes
    /// Format: new_view_number (1) + timestamp (8) + reason (1)
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        if data.len() < 10 {
            return Err(crate::ConsensusError::invalid_proposal(
                "ChangeView message too short",
            ));
        }

        let new_view_number = data[0];
        let timestamp = u64::from_le_bytes(
            data[1..9].try_into().unwrap_or([0u8; 8])
        );
        let reason = ChangeViewReason::from_byte(data[9])
            .unwrap_or(ChangeViewReason::Timeout);

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            new_view_number,
            timestamp,
            reason,
        })
    }

    /// Validates the message
    pub fn validate(&self) -> ConsensusResult<()> {
        // New view must be greater than current view
        if self.new_view_number <= self.view_number {
            return Err(crate::ConsensusError::InvalidViewNumber {
                current: self.view_number,
                requested: self.new_view_number,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_view_new() {
        let msg = ChangeViewMessage::new(100, 0, 1, 1, 1000, ChangeViewReason::Timeout);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.new_view_number, 1);
        assert_eq!(msg.reason, ChangeViewReason::Timeout);
    }

    #[test]
    fn test_change_view_serialize() {
        let msg = ChangeViewMessage::new(100, 0, 1, 1, 1000, ChangeViewReason::Timeout);
        let data = msg.serialize();

        // 1 byte new_view_number + 8 bytes timestamp + 1 byte reason
        assert_eq!(data.len(), 10);
        assert_eq!(data[0], 1); // new_view_number
    }

    #[test]
    fn test_change_view_validate() {
        let valid = ChangeViewMessage::new(100, 0, 1, 1, 1000, ChangeViewReason::Timeout);
        assert!(valid.validate().is_ok());

        let invalid = ChangeViewMessage::new(100, 1, 1, 1, 1000, ChangeViewReason::Timeout);
        assert!(invalid.validate().is_err());

        let invalid2 = ChangeViewMessage::new(100, 2, 1, 1, 1000, ChangeViewReason::Timeout);
        assert!(invalid2.validate().is_err());
    }

    #[test]
    fn test_change_view_serialize_deserialize_roundtrip() {
        let msg = ChangeViewMessage::new(100, 0, 1, 2, 12345678, ChangeViewReason::TxNotFound);
        let data = msg.serialize();

        let parsed = ChangeViewMessage::deserialize(&data, 100, 0, 1).unwrap();

        assert_eq!(parsed.block_index, 100);
        assert_eq!(parsed.view_number, 0);
        assert_eq!(parsed.validator_index, 1);
        assert_eq!(parsed.new_view_number, 2);
        assert_eq!(parsed.timestamp, 12345678);
        assert_eq!(parsed.reason, ChangeViewReason::TxNotFound);
    }

    #[test]
    fn test_change_view_deserialize_too_short() {
        let data = vec![0u8; 5]; // Too short
        let result = ChangeViewMessage::deserialize(&data, 100, 0, 1);
        assert!(result.is_err());
    }
}
