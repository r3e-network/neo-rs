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
#[path = "tests/keys.rs"]
mod tests;
