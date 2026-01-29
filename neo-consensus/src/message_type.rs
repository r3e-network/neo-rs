//! Consensus message type identifiers (mirrors `Neo.Consensus.ConsensusMessageType`).

use serde::{Deserialize, Serialize};

/// Consensus message type enum matching C# `ConsensusMessageType` exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ConsensusMessageType {
    /// Change view message - request to change the current view
    ChangeView = 0x00,
    /// Prepare request message - sent by the speaker to propose a block
    PrepareRequest = 0x20,
    /// Prepare response message - sent by validators to acknowledge the proposal
    PrepareResponse = 0x21,
    /// Commit message - sent when a validator is ready to commit the block
    Commit = 0x30,
    /// Recovery request message - request to recover consensus state
    RecoveryRequest = 0x40,
    /// Recovery message - response with consensus state for recovery
    RecoveryMessage = 0x41,
}

impl ConsensusMessageType {
    /// Converts from byte value
    #[must_use] 
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::ChangeView),
            0x20 => Some(Self::PrepareRequest),
            0x21 => Some(Self::PrepareResponse),
            0x30 => Some(Self::Commit),
            0x40 => Some(Self::RecoveryRequest),
            0x41 => Some(Self::RecoveryMessage),
            _ => None,
        }
    }

    /// Converts to byte value
    #[must_use] 
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Returns the string representation
    #[must_use] 
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChangeView => "ChangeView",
            Self::PrepareRequest => "PrepareRequest",
            Self::PrepareResponse => "PrepareResponse",
            Self::Commit => "Commit",
            Self::RecoveryRequest => "RecoveryRequest",
            Self::RecoveryMessage => "RecoveryMessage",
        }
    }
}

impl std::fmt::Display for ConsensusMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consensus_message_type_values() {
        assert_eq!(ConsensusMessageType::ChangeView as u8, 0x00);
        assert_eq!(ConsensusMessageType::PrepareRequest as u8, 0x20);
        assert_eq!(ConsensusMessageType::PrepareResponse as u8, 0x21);
        assert_eq!(ConsensusMessageType::Commit as u8, 0x30);
        assert_eq!(ConsensusMessageType::RecoveryRequest as u8, 0x40);
        assert_eq!(ConsensusMessageType::RecoveryMessage as u8, 0x41);
    }

    #[test]
    fn test_consensus_message_type_from_byte() {
        assert_eq!(
            ConsensusMessageType::from_byte(0x00),
            Some(ConsensusMessageType::ChangeView)
        );
        assert_eq!(
            ConsensusMessageType::from_byte(0x20),
            Some(ConsensusMessageType::PrepareRequest)
        );
        assert_eq!(
            ConsensusMessageType::from_byte(0x30),
            Some(ConsensusMessageType::Commit)
        );
        assert_eq!(ConsensusMessageType::from_byte(0x99), None);
    }

    #[test]
    fn test_consensus_message_type_roundtrip() {
        for msg_type in [
            ConsensusMessageType::ChangeView,
            ConsensusMessageType::PrepareRequest,
            ConsensusMessageType::PrepareResponse,
            ConsensusMessageType::Commit,
            ConsensusMessageType::RecoveryRequest,
            ConsensusMessageType::RecoveryMessage,
        ] {
            let byte = msg_type.to_byte();
            let recovered = ConsensusMessageType::from_byte(byte);
            assert_eq!(recovered, Some(msg_type));
        }
    }

    #[test]
    fn test_consensus_message_type_display() {
        assert_eq!(ConsensusMessageType::ChangeView.to_string(), "ChangeView");
        assert_eq!(
            ConsensusMessageType::PrepareRequest.to_string(),
            "PrepareRequest"
        );
        assert_eq!(ConsensusMessageType::Commit.to_string(), "Commit");
    }
}
