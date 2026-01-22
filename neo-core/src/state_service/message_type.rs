//! Message types for StateService extensible payloads.
//!
//! Matches `Neo.Plugins.StateService.Network.MessageType`.

/// StateService message type marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Vote message for state root signatures.
    Vote = 0,
    /// State root message containing the signed root.
    StateRoot = 1,
}

impl MessageType {
    /// Convert a raw byte into a message type.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Vote),
            1 => Some(Self::StateRoot),
            _ => None,
        }
    }
}
