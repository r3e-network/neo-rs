//! Consensus message types - dBFT protocol message identifiers.

use neo_primitives::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[allow(missing_docs)]
    /// Consensus message type enum matching C# `ConsensusMessageType` exactly
    pub ConsensusMessageType {
        ChangeView = 0x00,
        PrepareRequest = 0x20,
        PrepareResponse = 0x21,
        Commit = 0x30,
        RecoveryRequest = 0x40,
        RecoveryMessage = 0x41,
    }
}

#[cfg(test)]
#[path = "../tests/protocol/message_type.rs"]
mod tests;
