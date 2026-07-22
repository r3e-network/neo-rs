//! State Service Storage Keys
//!
//! Matches C# Neo.Plugins.StateService.Storage.Keys exactly.

/// Prefix byte of a persisted StateService state-root record.
pub const STATE_ROOT_PREFIX: u8 = 0x01;

/// Exact byte length of a persisted StateService state-root key.
pub const STATE_ROOT_KEY_LEN: usize = 1 + size_of::<u32>();

/// Returns the block index encoded by an exact StateService state-root key.
#[must_use]
pub fn state_root_index(key: &[u8]) -> Option<u32> {
    (key.len() == STATE_ROOT_KEY_LEN && key.first() == Some(&STATE_ROOT_PREFIX)).then(|| {
        u32::from_be_bytes(
            key[1..STATE_ROOT_KEY_LEN]
                .try_into()
                .expect("validated StateService state-root key length"),
        )
    })
}

/// Storage key prefixes for state service.
pub struct Keys;

impl Keys {
    /// Creates a storage key for a state root at the given index.
    /// Format: [0x01][index as big-endian u32]
    pub fn state_root(index: u32) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(STATE_ROOT_KEY_LEN);
        buffer.push(STATE_ROOT_PREFIX);
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
