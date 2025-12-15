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
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.push(self.reason.to_byte());
        data
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

        // 8 bytes timestamp + 1 byte reason
        assert_eq!(data.len(), 9);
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
}
