//! Shared storage-key construction helpers for native contracts.
//!
//! Replaces the ~25 inlined `let mut key = vec![PREFIX_XXX]; key.extend_from_slice(...)`
//! patterns across the native contracts with a single generic builder.
//! Keeps the byte-level layout (prefix-byte + big-endian-or-raw suffix) identical
//! to the inlined versions, so the on-disk storage keys are unchanged.

use neo_primitives::{UInt160, UInt256};
use neo_storage::StorageKey;

/// Build a storage key suffix of the form `[prefix_byte] ++ raw_suffix`.
#[inline]
pub fn prefixed(prefix: u8, suffix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(1 + suffix.len());
    key.push(prefix);
    key.extend_from_slice(suffix);
    key
}

/// Build a storage key `(contract_id, [prefix_byte] ++ raw_suffix)`.
#[inline]
pub fn prefixed_key(contract_id: i32, prefix: u8, suffix: &[u8]) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, suffix))
}

/// Build a storage key suffix of the form `[prefix_byte] ++ hash.to_bytes()`
/// (20 raw bytes for `UInt160`).
#[inline]
pub fn prefixed_with_hash160(prefix: u8, hash: &UInt160) -> Vec<u8> {
    prefixed(prefix, &hash.to_bytes())
}

/// Build a storage key `(contract_id, [prefix_byte] ++ hash.to_bytes())`.
#[inline]
pub fn prefixed_hash160_key(contract_id: i32, prefix: u8, hash: &UInt160) -> StorageKey {
    StorageKey::new(contract_id, prefixed_with_hash160(prefix, hash))
}

/// Build a storage key suffix of the form
/// `[prefix_byte] ++ hash.to_bytes() ++ value.to_be_bytes()`.
#[inline]
pub fn prefixed_with_hash160_i32_be(prefix: u8, hash: &UInt160, value: i32) -> Vec<u8> {
    let mut suffix = Vec::with_capacity(20 + 4);
    suffix.extend_from_slice(&hash.to_bytes());
    suffix.extend_from_slice(&value.to_be_bytes());
    prefixed(prefix, &suffix)
}

/// Build a storage key
/// `(contract_id, [prefix_byte] ++ hash.to_bytes() ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_hash160_i32_be_key(
    contract_id: i32,
    prefix: u8,
    hash: &UInt160,
    value: i32,
) -> StorageKey {
    StorageKey::new(
        contract_id,
        prefixed_with_hash160_i32_be(prefix, hash, value),
    )
}

/// Build a storage key suffix of the form `[prefix_byte] ++ hash.to_bytes()`
/// (32 raw bytes for `UInt256`).
#[inline]
pub fn prefixed_with_hash256(prefix: u8, hash: &UInt256) -> Vec<u8> {
    prefixed(prefix, &hash.to_bytes())
}

/// Build a storage key `(contract_id, [prefix_byte] ++ hash.to_bytes())`.
#[inline]
pub fn prefixed_hash256_key(contract_id: i32, prefix: u8, hash: &UInt256) -> StorageKey {
    StorageKey::new(contract_id, prefixed_with_hash256(prefix, hash))
}

/// Build a storage key suffix of the form
/// `[prefix_byte] ++ hash.to_bytes() ++ signer.to_bytes()`.
#[inline]
pub fn prefixed_with_hash256_hash160(prefix: u8, hash: &UInt256, signer: &UInt160) -> Vec<u8> {
    let mut suffix = Vec::with_capacity(32 + 20);
    suffix.extend_from_slice(&hash.to_bytes());
    suffix.extend_from_slice(&signer.to_bytes());
    prefixed(prefix, &suffix)
}

/// Build a storage key
/// `(contract_id, [prefix_byte] ++ hash.to_bytes() ++ signer.to_bytes())`.
#[inline]
pub fn prefixed_hash256_hash160_key(
    contract_id: i32,
    prefix: u8,
    hash: &UInt256,
    signer: &UInt160,
) -> StorageKey {
    StorageKey::new(
        contract_id,
        prefixed_with_hash256_hash160(prefix, hash, signer),
    )
}

/// Build a storage key suffix of the form `[prefix_byte] ++ value.to_be_bytes()`.
#[inline]
pub fn prefixed_with_u32_be(prefix: u8, value: u32) -> Vec<u8> {
    prefixed(prefix, &value.to_be_bytes())
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_u32_be_key(contract_id: i32, prefix: u8, value: u32) -> StorageKey {
    StorageKey::new(contract_id, prefixed_with_u32_be(prefix, value))
}

/// Build a storage key suffix of the form `[prefix_byte] ++ value.to_be_bytes()`.
#[inline]
pub fn prefixed_with_i32_be(prefix: u8, value: i32) -> Vec<u8> {
    prefixed(prefix, &value.to_be_bytes())
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_i32_be_key(contract_id: i32, prefix: u8, value: i32) -> StorageKey {
    StorageKey::new(contract_id, prefixed_with_i32_be(prefix, value))
}

/// Build a storage key suffix of the form `[prefix_byte] ++ value.to_be_bytes()`
/// (8 big-endian bytes for `u64`).
#[inline]
pub fn prefixed_with_u64_be(prefix: u8, value: u64) -> Vec<u8> {
    prefixed(prefix, &value.to_be_bytes())
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_u64_be_key(contract_id: i32, prefix: u8, value: u64) -> StorageKey {
    StorageKey::new(contract_id, prefixed_with_u64_be(prefix, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::{UInt160, UInt256};

    #[test]
    fn prefixed_composes_prefix_and_suffix() {
        let key = prefixed(0xAB, &[0x01, 0x02, 0x03]);
        assert_eq!(key, vec![0xAB, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn prefixed_key_wraps_suffix_with_contract_id() {
        let key = prefixed_key(-9, 0xAB, &[0x01, 0x02, 0x03]);
        assert_eq!(key.id(), -9);
        assert_eq!(key.key(), &[0xAB, 0x01, 0x02, 0x03]);
    }

    #[test]
    fn prefixed_with_hash160_matches_manual_construction() {
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let key = prefixed_with_hash160(14, &hash);
        let mut expected = vec![14u8];
        expected.extend_from_slice(&[7u8; 20]);
        assert_eq!(key, expected);
    }

    #[test]
    fn prefixed_hash160_key_wraps_hash_suffix_with_contract_id() {
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let key = prefixed_hash160_key(-7, 14, &hash);
        let mut expected = vec![14u8];
        expected.extend_from_slice(&[7u8; 20]);
        assert_eq!(key.id(), -7);
        assert_eq!(key.key(), expected);
    }

    #[test]
    fn prefixed_with_hash160_i32_be_appends_big_endian_i32() {
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let key = prefixed_with_hash160_i32_be(16, &hash, 0x01020304);
        let mut expected = vec![16u8];
        expected.extend_from_slice(&[7u8; 20]);
        expected.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(key, expected);
    }

    #[test]
    fn prefixed_hash160_i32_be_key_wraps_composite_suffix_with_contract_id() {
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        let key = prefixed_hash160_i32_be_key(-7, 16, &hash, 0x01020304);
        let mut expected = vec![16u8];
        expected.extend_from_slice(&[7u8; 20]);
        expected.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(key.id(), -7);
        assert_eq!(key.key(), expected);
    }

    #[test]
    fn prefixed_with_hash256_matches_manual_construction() {
        let hash = UInt256::from_bytes(&[9u8; 32]).unwrap();
        let key = prefixed_with_hash256(8, &hash);
        let mut expected = vec![8u8];
        expected.extend_from_slice(&[9u8; 32]);
        assert_eq!(key, expected);
    }

    #[test]
    fn typed_key_helpers_match_storage_key_legacy_builders() {
        let hash256 = UInt256::from_bytes(&[9u8; 32]).unwrap();
        let hash160 = UInt160::from_bytes(&[7u8; 20]).unwrap();

        assert_eq!(
            prefixed_hash256_key(-2, 0x15, &hash256),
            StorageKey::create_with_uint256(-2, 0x15, &hash256)
        );
        assert_eq!(
            prefixed_hash256_hash160_key(-2, 0x15, &hash256, &hash160),
            StorageKey::create_with_uint256_uint160(-2, 0x15, &hash256, &hash160)
        );
        assert_eq!(
            prefixed_i32_be_key(-3, 0x20, -0x1020304),
            StorageKey::create_with_int32(-3, 0x20, -0x1020304)
        );
        assert_eq!(
            prefixed_u32_be_key(-4, 0x21, 0x12345678),
            StorageKey::create_with_uint32(-4, 0x21, 0x12345678)
        );
        assert_eq!(
            prefixed_u64_be_key(-5, 0x22, 0x0102030405060708),
            StorageKey::create_with_uint64(-5, 0x22, 0x0102030405060708)
        );
    }

    #[test]
    fn prefixed_with_u32_be_is_big_endian() {
        let key = prefixed_with_u32_be(0xFF, 0x12345678);
        assert_eq!(key, vec![0xFF, 0x12, 0x34, 0x56, 0x78]);
    }

    #[test]
    fn prefixed_with_i32_be_is_big_endian() {
        let key = prefixed_with_i32_be(0x10, -1);
        assert_eq!(key, vec![0x10, 0xFF, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn prefixed_with_u64_be_is_big_endian() {
        let key = prefixed_with_u64_be(0x09, 0x0102030405060708);
        assert_eq!(
            key,
            vec![0x09, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]
        );
    }
}
