//! Canonical state-root record schema, projected for out-of-process readers.
//!
//! The state service persists its records in the *chain* database through the
//! shared [`neo_storage::Store`] key space:
//!
//! ```text
//! database key = [namespace id: i32, little-endian, 4 bytes]
//!             ++ [record prefix: u8, 1 byte]
//!             ++ [height: u32, big-endian, 4 bytes]        (root records)
//!
//! database key = [namespace id: i32, little-endian, 4 bytes]
//!             ++ [cursor prefix: u8, 1 byte]               (progress cursors)
//!
//! root record value = [root hash: 32 bytes][version: 1 byte][witness flag: 1 byte]
//!                  ++ length-prefixed invocation/verification scripts when a
//!                     witness is present
//!
//! cursor value = [height: u64, big-endian]
//! ```
//!
//! Root records are therefore stored under a single namespaced prefix and sort
//! numerically by height, which lets offline auditors stream them with one
//! forward iterator instead of 13.1M point reads.
//!
//! Offline tools must not re-derive this layout: a diverging copy would read
//! the wrong bytes and report a false "mismatch" against a healthy database.
//! Every accessor here delegates to the same functions the live state service
//! uses, so the encoder, the decoder and the key builder have exactly one
//! definition.

use crate::StateRoot;
use crate::commit_handlers::{
    PREFIX_STATE_ROOT, STATE_ROOT_STORAGE_ID, deserialize_state_root_record,
    serialize_state_root_record, state_root_record_key as canonical_record_key,
};
use crate::store::{PREFIX_LOCAL_ROOT_INDEX, PREFIX_VALIDATED_ROOT_INDEX};
use neo_storage::StorageKey;

/// Human-readable byte layout of the persisted root record key.
///
/// Printed by read-only tools at startup so an audit log names the exact
/// on-disk schema it assumed.
pub const KEY_LAYOUT: &str = "key = i32::MIN (little-endian: 00 00 00 80) ++ 0xf0 ++ height (big-endian u32); value = \
     root_hash(32) ++ version(1) ++ witness_flag(1) ++ optional scripts";

/// Namespace id shared by root records, trie nodes and progress cursors.
///
/// Native contracts use small negative ids and deployed contracts positive
/// ids, so `i32::MIN` cannot collide with either namespace.
#[must_use]
pub const fn namespace_id() -> i32 {
    STATE_ROOT_STORAGE_ID
}

/// Storage key of the root record persisted for `index`.
#[must_use]
pub fn state_root_record_key(index: u32) -> StorageKey {
    canonical_record_key(index)
}

/// Database key prefix (5 bytes) shared by every persisted root record.
///
/// Appending a big-endian height yields the full 9-byte record key.
#[must_use]
pub fn state_root_record_prefix() -> Vec<u8> {
    StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_STATE_ROOT]).to_array()
}

/// Serializes a root record with the canonical live-service encoder.
#[must_use]
pub fn encode_state_root_record(root: &StateRoot) -> Vec<u8> {
    serialize_state_root_record(root)
}

/// Decodes a persisted root record with the canonical live-service decoder.
///
/// Returns `None` for malformed bytes (fail-closed); the height is supplied by
/// the caller because it lives in the key, not the value.
#[must_use]
pub fn decode_state_root_record(index: u32, bytes: &[u8]) -> Option<StateRoot> {
    deserialize_state_root_record(index, bytes)
}

/// Storage key of the durable local-commit cursor.
///
/// The cursor names the highest height whose root record was committed by a
/// local block (its backing block exists on this node).
#[must_use]
pub fn local_root_cursor_key() -> StorageKey {
    StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_LOCAL_ROOT_INDEX])
}

/// Storage key of the quorum-validated (network) cursor.
#[must_use]
pub fn validated_root_cursor_key() -> StorageKey {
    StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_VALIDATED_ROOT_INDEX])
}

/// Encodes a cursor value (`u64` big-endian holding a `u32` height).
#[must_use]
pub fn encode_cursor(index: u32) -> Vec<u8> {
    u64::from(index).to_be_bytes().to_vec()
}

/// Decodes a cursor value, rejecting truncated or out-of-range heights.
#[must_use]
pub fn decode_cursor(bytes: &[u8]) -> Option<u32> {
    let raw: [u8; 8] = bytes.try_into().ok()?;
    u32::try_from(u64::from_be_bytes(raw)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt256;

    #[test]
    fn record_key_matches_documented_layout() {
        let key = state_root_record_key(13_141_250).to_array();
        let expected_height = 13_141_250u32.to_be_bytes();
        assert_eq!(key.len(), 9);
        assert_eq!(&key[..4], &i32::MIN.to_le_bytes());
        assert_eq!(key[4], PREFIX_STATE_ROOT);
        assert_eq!(&key[5..], &expected_height);
        assert_eq!(&key[..5], state_root_record_prefix().as_slice());
    }

    #[test]
    fn record_keys_sort_numerically_by_height() {
        let mut keys: Vec<Vec<u8>> = [0u32, 1, 255, 256, 65_536, 13_141_250]
            .into_iter()
            .map(|h| state_root_record_key(h).to_array())
            .collect();
        let mut heights = vec![0u32, 1, 255, 256, 65_536, 13_141_250];
        keys.sort();
        heights.sort_unstable();
        let decoded: Vec<u32> = keys
            .iter()
            .map(|k| u32::from_be_bytes([k[5], k[6], k[7], k[8]]))
            .collect();
        assert_eq!(decoded, heights);
    }

    #[test]
    fn record_value_round_trips_through_canonical_codec() {
        let root = StateRoot::new_current(7, UInt256::from([0x5au8; 32]));
        let bytes = encode_state_root_record(&root);
        let decoded = decode_state_root_record(7, &bytes).expect("canonical record decodes");
        assert_eq!(decoded.index, 7);
        assert_eq!(decoded.version, root.version);
        assert_eq!(decoded.root_hash, root.root_hash);
        assert!(decoded.witness.is_none());
        assert!(decode_state_root_record(7, &bytes[..33]).is_none());
    }

    #[test]
    fn cursor_codec_rejects_truncated_and_oversized_values() {
        assert_eq!(decode_cursor(&encode_cursor(9_999)), Some(9_999));
        assert_eq!(decode_cursor(&[0u8; 7]), None);
        assert_eq!(
            decode_cursor(&(u64::from(u32::MAX) + 1).to_be_bytes()),
            None
        );
        assert_eq!(decode_cursor(&[0u8; 9]), None);
    }
}
