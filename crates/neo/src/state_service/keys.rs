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
mod tests {
    use super::*;

    #[test]
    fn test_state_root_key() {
        let key = Keys::state_root(12345);
        assert_eq!(key.len(), 5);
        assert_eq!(key[0], 0x01);
        // 12345 in big-endian is 0x00003039
        assert_eq!(&key[1..], &[0x00, 0x00, 0x30, 0x39]);
    }

    #[test]
    fn test_constant_keys() {
        assert_eq!(Keys::CURRENT_LOCAL_ROOT_INDEX, &[0x02]);
        assert_eq!(Keys::CURRENT_VALIDATED_ROOT_INDEX, &[0x04]);
    }
}
