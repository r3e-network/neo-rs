//! State Service Storage Keys
//!
//! Matches C# Neo.Plugins.StateService.Storage.Keys exactly.

/// Storage key prefixes for state service.
pub struct Keys;

impl Keys {
    /// Creates a storage key for a state root at the given index.
    /// Format: [0x01][index as big-endian u32]
    pub fn state_root(index: u32) -> Vec<u8> {
        let mut buffer = vec![0x01];
        buffer.extend_from_slice(&index.to_be_bytes());
        buffer
    }

    /// Storage key for the current local root index.
    pub const CURRENT_LOCAL_ROOT_INDEX: &'static [u8] = &[0x02];

    /// Storage key for the current validated root index.
    pub const CURRENT_VALIDATED_ROOT_INDEX: &'static [u8] = &[0x04];
}

#[cfg(test)]
#[path = "../tests/protocol/keys.rs"]
mod tests;
