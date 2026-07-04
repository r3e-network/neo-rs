//! Shared storage-key construction helpers for native contracts.
//!
//! Replaces the ~25 inlined `let mut key = vec![PREFIX_XXX]; key.extend_from_slice(...)`
//! patterns across the native contracts with a single generic builder.
//! Keeps the byte-level layout (prefix-byte + big-endian-or-raw suffix) identical
//! to the inlined versions, so the on-disk storage keys are unchanged.
//!
//! # Relationship to other key builders (ADR-025)
//!
//! Three key-builder systems coexist by design:
//! - **`neo_storage::KeyBuilder`** — low-level byte builder with length enforcement.
//!   Has zero production callers but is kept as the reference implementation.
//! - **This module (`support::keys`)** — ergonomic typed free functions. This is
//!   the **production standard** for native contracts, enforced by the style test
//!   at `tests/style/mod.rs`. Delegates to `StorageKey::new(id, suffix)` which
//!   keeps id and suffix logically separated.
//! - **`StorageKey::create_with_*`** — retained for test fixtures and
//!   `create_search_prefix` (range scans). Forbidden in production native-contract
//!   code by the style test.

use neo_primitives::{UInt160, UInt256};
use neo_storage::StorageKey;

/// Build a storage key suffix of the form `[prefix_byte] ++ raw_suffix`.
///
/// This is an internal helper — all public API is via the `*_key` variants.
#[inline]
fn prefixed(prefix: u8, suffix: &[u8]) -> Vec<u8> {
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

/// Build a storage key `(contract_id, [prefix_byte] ++ hash.to_bytes())`.
///
/// `hash` is a 20-byte `UInt160` (raw byte order, not reversed).
#[inline]
pub fn prefixed_hash160_key(contract_id: i32, prefix: u8, hash: &UInt160) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, &hash.to_bytes()))
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
    let mut suffix = Vec::with_capacity(20 + 4);
    suffix.extend_from_slice(&hash.to_bytes());
    suffix.extend_from_slice(&value.to_be_bytes());
    StorageKey::new(contract_id, prefixed(prefix, &suffix))
}

/// Build a storage key `(contract_id, [prefix_byte] ++ hash.to_bytes())`.
///
/// `hash` is a 32-byte `UInt256` (raw byte order, not reversed).
#[inline]
pub fn prefixed_hash256_key(contract_id: i32, prefix: u8, hash: &UInt256) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, &hash.to_bytes()))
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
    let mut suffix = Vec::with_capacity(32 + 20);
    suffix.extend_from_slice(&hash.to_bytes());
    suffix.extend_from_slice(&signer.to_bytes());
    StorageKey::new(contract_id, prefixed(prefix, &suffix))
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_u32_be_key(contract_id: i32, prefix: u8, value: u32) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, &value.to_be_bytes()))
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_i32_be_key(contract_id: i32, prefix: u8, value: i32) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, &value.to_be_bytes()))
}

/// Build a storage key `(contract_id, [prefix_byte] ++ value.to_be_bytes())`.
#[inline]
pub fn prefixed_u64_be_key(contract_id: i32, prefix: u8, value: u64) -> StorageKey {
    StorageKey::new(contract_id, prefixed(prefix, &value.to_be_bytes()))
}

#[cfg(test)]
#[path = "../tests/support/keys.rs"]
mod tests;
