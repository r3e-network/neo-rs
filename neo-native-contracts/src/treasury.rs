//! Treasury native contract.
//!
//! Real (non-stub) implementation of the Treasury native contract.
//! Mirrors the C# `Neo.SmartContract.Native.Treasury` storage layout
//! so the application engine and the post-persist pipeline can record
//! fee-distribution records byte-for-byte compatible with the C# node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix  | Value           |
//! |--------|-------------|-----------------|
//! | 0x10   | 32 zero bytes | LE i64 main pool |
//!
//! The Treasury contract is gated behind a hardfork that has not been
//! deployed on MainNet yet, but the storage surface is implemented so
//! the executor can read / write fee-distribution records.

use crate::hashes::TREASURY_HASH;
use crate::gas_token::{deserialize_i64, serialize_i64};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `Treasury.PREFIX_TOTAL_SUPPLY` (32 zero bytes -> main pool).
const PREFIX_TOTAL_SUPPLY: u8 = 0x10;

/// Lazily-initialised script-hash handle for the Treasury contract.
pub static TREASURY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *TREASURY_HASH);

/// Static accessor for the Treasury native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Treasury;

impl Treasury {
    /// Stable native contract id (matches C# `Treasury.Id`).
    pub const ID: i32 = -11;

    /// Constructs a new `Treasury` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Treasury contract.
    pub fn hash(&self) -> UInt160 {
        *TREASURY_HASH_REF
    }

    /// Returns the script hash of the Treasury contract (static).
    pub fn script_hash() -> UInt160 {
        *TREASURY_HASH_REF
    }

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    /// Storage key for the main pool total.
    #[inline]
    pub fn total_supply_storage_key() -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_TOTAL_SUPPLY, &[0u8; 32])
    }

    // ------------------------------------------------------------------
    // Read-only surface
    // ------------------------------------------------------------------

    /// Returns the current main pool balance.
    pub fn get_total_pool(snapshot: &DataCache) -> CoreResult<i64> {
        let key = Self::total_supply_storage_key();
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()),
            None => Ok(0),
        }
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    /// Adds `amount` to the main pool.
    pub fn add_to_pool(snapshot: &DataCache, amount: i64) -> CoreResult<()> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("amount must be non-negative"));
        }
        if amount == 0 {
            return Ok(());
        }
        let current = Self::get_total_pool(snapshot)?;
        let new_value = current
            .checked_add(amount)
            .ok_or_else(|| CoreError::native_contract("treasury pool overflow"))?;
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write treasury pool",
            ));
        }
        snapshot.add(
            Self::total_supply_storage_key(),
            StorageItem::from_bytes(serialize_i64(new_value)?),
        );
        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use neo_data_cache::DataCache;
    use std::sync::Arc;

    fn fresh_cache() -> Arc<DataCache> {
        Arc::new(DataCache::new_with_config(
            false,
            None,
            None,
            Default::default(),
        ))
    }

    #[test]
    fn test_treasury_constants() {
        assert_eq!(Treasury::ID, -11);
    }

    #[test]
    fn test_treasury_hash() {
        let expected = *TREASURY_HASH;
        assert_eq!(Treasury::script_hash(), expected);
        assert_eq!(Treasury::new().hash(), expected);
    }

    #[test]
    fn test_get_total_pool_empty() {
        let cache = fresh_cache();
        assert_eq!(Treasury::get_total_pool(&cache).unwrap(), 0);
    }

    #[test]
    fn test_add_to_pool() {
        let cache = fresh_cache();
        Treasury::add_to_pool(&cache, 1_000).unwrap();
        assert_eq!(Treasury::get_total_pool(&cache).unwrap(), 1_000);
        Treasury::add_to_pool(&cache, 2_500).unwrap();
        assert_eq!(Treasury::get_total_pool(&cache).unwrap(), 3_500);
    }

    #[test]
    fn test_add_to_pool_zero_is_noop() {
        let cache = fresh_cache();
        Treasury::add_to_pool(&cache, 0).unwrap();
        assert_eq!(Treasury::get_total_pool(&cache).unwrap(), 0);
    }

    #[test]
    fn test_add_to_pool_negative_rejected() {
        let cache = fresh_cache();
        let res = Treasury::add_to_pool(&cache, -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_total_supply_storage_key_format() {
        let key = Treasury::total_supply_storage_key();
        assert_eq!(key.id(), Treasury::ID);
        assert_eq!(key.key()[0], PREFIX_TOTAL_SUPPLY);
        assert_eq!(key.key().len(), 33);
        assert_eq!(&key.key()[1..], &[0u8; 32]);
    }

    #[test]
    fn test_read_only_cache_rejects_write() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let res = Treasury::add_to_pool(&cache, 100);
        assert!(res.is_err());
    }
}
