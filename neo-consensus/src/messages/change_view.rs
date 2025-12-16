//! ChangeView message - request to change the current view.

use crate::messages::{parse_consensus_message_header, serialize_consensus_message_header};
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

    /// Returns the message type
    pub fn message_type(&self) -> ConsensusMessageType {
        ConsensusMessageType::ChangeView
    }

    /// NewViewNumber is always ViewNumber + 1 on Neo N3.
    pub fn new_view_number(&self) -> u8 {
        self.view_number.wrapping_add(1)
    }

    /// Serializes the message to bytes
    ///
    /// Matches C# `DBFTPlugin.Messages.ChangeView`:
    /// - header (type, block_index, validator_index, view_number)
    /// - timestamp (u64 LE)
    /// - reason (u8)
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = serialize_consensus_message_header(
            ConsensusMessageType::ChangeView,
            self.block_index,
            self.validator_index,
            self.view_number,
        );
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out.push(self.reason.to_byte());
        out
    }

    /// Deserializes a ChangeView message from bytes
    pub fn deserialize(data: &[u8]) -> ConsensusResult<Self> {
        let (msg_type, block_index, validator_index, view_number, body) =
            parse_consensus_message_header(data)?;
        if msg_type != ConsensusMessageType::ChangeView {
            return Err(crate::ConsensusError::invalid_proposal(
                "invalid ChangeView message type",
            ));
        }

        if body.len() < 9 {
            return Err(crate::ConsensusError::invalid_proposal(
                "ChangeView message body too short",
            ));
        }

        let timestamp = u64::from_le_bytes(body[0..8].try_into().unwrap_or([0u8; 8]));
        let reason =
            ChangeViewReason::from_byte(body[8]).unwrap_or(ChangeViewReason::Timeout);

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
        // New view must be greater than current view (always +1 in Neo N3).
        let requested = self.new_view_number();
        if requested <= self.view_number {
            return Err(crate::ConsensusError::InvalidViewNumber {
                current: self.view_number,
                requested,
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
        let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);

        assert_eq!(msg.block_index, 100);
        assert_eq!(msg.view_number, 0);
        assert_eq!(msg.validator_index, 1);
        assert_eq!(msg.new_view_number(), 1);
        assert_eq!(msg.reason, ChangeViewReason::Timeout);
    }

    #[test]
    fn test_change_view_serialize() {
        let msg = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
        let data = msg.serialize();

        // 7 byte header + 8 bytes timestamp + 1 byte reason
        assert_eq!(data.len(), 16);
        assert_eq!(data[0], ConsensusMessageType::ChangeView.to_byte());
    }

    #[test]
    fn test_change_view_validate() {
        let valid = ChangeViewMessage::new(100, 0, 1, 1000, ChangeViewReason::Timeout);
        assert!(valid.validate().is_ok());

        // Wrap-around is treated as invalid
        let invalid = ChangeViewMessage::new(100, u8::MAX, 1, 1000, ChangeViewReason::Timeout);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_change_view_serialize_deserialize_roundtrip() {
        let msg = ChangeViewMessage::new(100, 0, 1, 12345678, ChangeViewReason::TxNotFound);
        let data = msg.serialize();

        let parsed = ChangeViewMessage::deserialize(&data).unwrap();

        assert_eq!(parsed.block_index, 100);
        assert_eq!(parsed.view_number, 0);
        assert_eq!(parsed.validator_index, 1);
        assert_eq!(parsed.new_view_number(), 1);
        assert_eq!(parsed.timestamp, 12345678);
        assert_eq!(parsed.reason, ChangeViewReason::TxNotFound);
    }

    #[test]
    fn test_change_view_deserialize_too_short() {
        let data = vec![0u8; 5]; // Too short
        let result = ChangeViewMessage::deserialize(&data);
        assert!(result.is_err());
    }
}
