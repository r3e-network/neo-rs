//! `ChangeView` message - request to change the current view.

use crate::{ChangeViewReason, ConsensusMessageType, ConsensusResult};
use serde::{Deserialize, Serialize};

/// `ChangeView` message sent when a validator wants to change the view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeViewMessage {
    /// Block index
    pub block_index: u32,
    /// Current view number
    pub view_number: u8,
    /// Validator index
    pub validator_index: u8,
    /// Timestamp of the request
    pub timestamp: u64,
    /// Reason for the view change
    pub reason: ChangeViewReason,
}

impl ChangeViewMessage {
    /// Creates a new `ChangeView` message
    #[must_use] 
    pub const fn new(
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        timestamp: u64,
        reason: ChangeViewReason,
    ) -> Self {
        Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
        }
    }

    /// `NewViewNumber` is always `ViewNumber + 1` (matches C# `DBFTPlugin`).
    pub fn new_view_number(&self) -> ConsensusResult<u8> {
        self.view_number
            .checked_add(1)
            .ok_or_else(|| crate::ConsensusError::invalid_proposal("View number overflow"))
    }

    /// Returns the message type
    #[must_use] 
    pub const fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::ChangeView
    }

    /// Serializes the message to bytes
    /// Neo N3 `DBFTPlugin` format: `timestamp (8) + reason (1)`.
    #[must_use] 
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.push(self.reason.to_byte());
        data
    }

    /// Deserializes a `ChangeView` message from bytes
    /// Neo N3 `DBFTPlugin` format: `timestamp (8) + reason (1)`.
    pub fn deserialize(
        data: &[u8],
        block_index: u32,
        view_number: u8,
        validator_index: u8,
    ) -> ConsensusResult<Self> {
        if data.len() < 9 {
            return Err(crate::ConsensusError::invalid_proposal(
                "ChangeView message too short",
            ));
        }

        let timestamp = u64::from_le_bytes(data[0..8].try_into().unwrap_or([0u8; 8]));
        let reason = ChangeViewReason::from_byte(data[8]).unwrap_or(ChangeViewReason::Timeout);

        Ok(Self {
            block_index,
            view_number,
            validator_index,
            timestamp,
            reason,
        })
    }

    /// Validates the message
    pub fn validate(&self) -> ConsensusResult<()> {
        // Ensure NewViewNumber is representable and strictly larger than ViewNumber.
        let new_view = self.new_view_number()?;
        if new_view <= self.view_number {
            return Err(crate::ConsensusError::invalid_proposal(
                "Invalid ChangeView new view number",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_view_new() {
        let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.new_view_number().unwrap(), 1);
        assert_eq!(msg.reason, ChangeViewReason::Timeout);
    }

    #[test]
    fn test_change_view_serialize() {
        let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
        let data = msg.serialize();

        // 8 bytes timestamp + 1 byte reason
        assert_eq!(data.len(), 9);
    }

    #[test]
    fn test_change_view_wire_format_bytes() {
        let timestamp = 0x0102_0304_0506_0708u64;
        let msg = ChangeViewMessage::new(100, 7, 1, timestamp, ChangeViewReason::TxNotFound);
        let data = msg.serialize();

        let mut expected = Vec::new();
        expected.extend_from_slice(&timestamp.to_le_bytes());
        expected.push(ChangeViewReason::TxNotFound.to_byte());
        assert_eq!(data, expected);
    }

    #[test]
    fn test_change_view_validate() {
        let valid = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
        assert!(valid.validate().is_ok());

        // Overflow case cannot be constructed as valid.
        let overflow = ChangeViewMessage::new(100, u8::MAX, 1, 1000, ChangeViewReason::Timeout);
        assert!(overflow.validate().is_err());
    }

    #[test]
    fn test_change_view_serialize_deserialize_roundtrip() {
        let msg = ChangeViewMessage::new(100, 0, 1, 12345678, ChangeViewReason::TxNotFound);
        let data = msg.serialize();

        let parsed = ChangeViewMessage::deserialize(&data, 100, 0, 1).unwrap();

        assert_eq!(parsed.block_index, 100);
        assert_eq!(parsed.view_number, 0);
        assert_eq!(parsed.validator_index, 1);
        assert_eq!(parsed.new_view_number().unwrap(), 1);
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
