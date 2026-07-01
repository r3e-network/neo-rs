//! Message types for StateService extensible payloads.
//!
//! Matches `Neo.Plugins.StateService.Network.MessageType`.

use neo_primitives::protocol_enum;

protocol_enum! {
    /// StateService message type marker.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub MessageType {
        /// Vote message for state root signatures.
        Vote = 0,
        /// State root message containing the signed root.
        StateRoot = 1,
    }
}

#[cfg(test)]
#[path = "../tests/protocol/message_type.rs"]
mod tests;
