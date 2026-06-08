//! PolicyContract native contract.
//!
//! Real (non-stub) implementation of the Neo policy contract. Mirrors
//! the C# `Neo.SmartContract.Native.PolicyContract` storage layout so
//! the application engine, services, and plugins can read policy
//! values (fee per byte, exec fee factor, max block size, blocked
//! accounts) byte-for-byte compatible with the C# node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix            | Value         |
//! |--------|----------------------|---------------|
//! | 0x15   | (none)               | LE u32 fee/byte |
//! | 0x18   | (none)               | LE u32 exec factor |
//! | 0x19   | (none)               | LE u32 storage price |
//! | 0x1A   | (none)               | LE u32 max block size |
//! | 0x1B   | (none)               | LE u32 max block sys fee |
//! | 0x1C   | (none)               | LE u32 max traceback blocks |
//! | 0x1D   | 20-byte account hash | (blocked marker) |

use crate::hashes::POLICY_CONTRACT_HASH;
use crate::gas_token::{deserialize_i64, serialize_i64};
use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `PolicyContract.PREFIX_FEE_PER_BYTE`.
const PREFIX_FEE_PER_BYTE: u8 = 0x15;
/// C# `PolicyContract.PREFIX_EXEC_FEE_FACTOR`.
const PREFIX_EXEC_FEE_FACTOR: u8 = 0x18;
/// C# `PolicyContract.PREFIX_STORAGE_PRICE`.
const PREFIX_STORAGE_PRICE: u8 = 0x19;
/// C# `PolicyContract.PREFIX_MAX_BLOCK_SIZE`.
const PREFIX_MAX_BLOCK_SIZE: u8 = 0x1A;
/// C# `PolicyContract.PREFIX_MAX_BLOCK_SYSTEM_FEE`.
const PREFIX_MAX_BLOCK_SYSTEM_FEE: u8 = 0x1B;
/// C# `PolicyContract.PREFIX_MAX_TRACEABLE_BLOCKS`.
const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 0x1C;
/// C# `PolicyContract.PREFIX_BLOCKED_ACCOUNT`.
const PREFIX_BLOCKED_ACCOUNT: u8 = 0x1D;

/// Lazily-initialised script-hash handle for the PolicyContract.
pub static POLICY_HASH: LazyLock<UInt160> = LazyLock::new(|| *POLICY_CONTRACT_HASH);

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default storage price (matches C# `PolicyContract.DefaultStoragePrice`).
pub const DEFAULT_STORAGE_PRICE: u32 = 100_000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;
/// Default max block size (matches C# `PolicyContract.DefaultMaxBlockSize`).
pub const DEFAULT_MAX_BLOCK_SIZE: u32 = 1_048_576;
/// Default max block system fee (matches C# `PolicyContract.DefaultMaxBlockSystemFee`).
pub const DEFAULT_MAX_BLOCK_SYSTEM_FEE: i64 = 9_0000_0000_000; // 90,000 GAS
/// Default max traceable blocks
/// (matches C# `PolicyContract.DefaultMaxTraceableBlocks`).
pub const DEFAULT_MAX_TRACEABLE_BLOCKS: u32 = 2_102_352;

/// Static accessor for the PolicyContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContract;

impl PolicyContract {
    /// Stable native contract id (matches C# `PolicyContract.Id`).
    pub const ID: i32 = -7;

    /// Default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = DEFAULT_EXEC_FEE_FACTOR;
    /// Default fee per byte.
    pub const DEFAULT_FEE_PER_BYTE: u32 = DEFAULT_FEE_PER_BYTE;
    /// Default max valid-until-block increment.
    pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT;
    /// Default max block size.
    pub const DEFAULT_MAX_BLOCK_SIZE: u32 = DEFAULT_MAX_BLOCK_SIZE;
    /// Default max block system fee (90,000 GAS, 8 decimals).
    pub const DEFAULT_MAX_BLOCK_SYSTEM_FEE: i64 = DEFAULT_MAX_BLOCK_SYSTEM_FEE;
    /// Default max traceable blocks.
    pub const DEFAULT_MAX_TRACEABLE_BLOCKS: u32 = DEFAULT_MAX_TRACEABLE_BLOCKS;
    /// Default storage price.
    pub const DEFAULT_STORAGE_PRICE: u32 = DEFAULT_STORAGE_PRICE;

    /// Constructs a new `PolicyContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the PolicyContract.
    pub fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    /// Returns the script hash of the PolicyContract (static).
    pub fn script_hash() -> UInt160 {
        *POLICY_HASH
    }

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    #[inline]
    pub fn fee_per_byte_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_FEE_PER_BYTE)
    }
    #[inline]
    pub fn exec_fee_factor_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_EXEC_FEE_FACTOR)
    }
    #[inline]
    pub fn storage_price_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_STORAGE_PRICE)
    }
    #[inline]
    pub fn max_block_size_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_MAX_BLOCK_SIZE)
    }
    #[inline]
    pub fn max_block_system_fee_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_MAX_BLOCK_SYSTEM_FEE)
    }
    #[inline]
    pub fn max_traceable_blocks_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_MAX_TRACEABLE_BLOCKS)
    }
    #[inline]
    pub fn blocked_account_storage_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_BLOCKED_ACCOUNT, account)
    }

    // ------------------------------------------------------------------
    // Read-only surface (returns the value, falling back to the
    // corresponding `DEFAULT_*` constant when the value is not
    // persisted yet).
    // ------------------------------------------------------------------

    pub fn get_fee_per_byte(snapshot: &DataCache) -> CoreResult<u32> {
        Self::read_u32(snapshot, &Self::fee_per_byte_storage_key(), Self::DEFAULT_FEE_PER_BYTE)
    }

    pub fn get_exec_fee_factor(snapshot: &DataCache) -> CoreResult<u32> {
        Self::read_u32(
            snapshot,
            &Self::exec_fee_factor_storage_key(),
            Self::DEFAULT_EXEC_FEE_FACTOR,
        )
    }

    pub fn get_storage_price(snapshot: &DataCache) -> CoreResult<u32> {
        Self::read_u32(
            snapshot,
            &Self::storage_price_storage_key(),
            Self::DEFAULT_STORAGE_PRICE,
        )
    }

    pub fn get_max_block_size(snapshot: &DataCache) -> CoreResult<u32> {
        Self::read_u32(
            snapshot,
            &Self::max_block_size_storage_key(),
            Self::DEFAULT_MAX_BLOCK_SIZE,
        )
    }

    pub fn get_max_block_system_fee(snapshot: &DataCache) -> CoreResult<i64> {
        let key = Self::max_block_system_fee_storage_key();
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()),
            None => Ok(Self::DEFAULT_MAX_BLOCK_SYSTEM_FEE),
        }
    }

    pub fn get_max_traceable_blocks(snapshot: &DataCache) -> CoreResult<u32> {
        Self::read_u32(
            snapshot,
            &Self::max_traceable_blocks_storage_key(),
            Self::DEFAULT_MAX_TRACEABLE_BLOCKS,
        )
    }

    pub fn is_blocked(snapshot: &DataCache, account: &UInt160) -> CoreResult<bool> {
        let key = Self::blocked_account_storage_key(account);
        Ok(snapshot.get(&key).is_some())
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    pub fn set_fee_per_byte(snapshot: &DataCache, value: u32) -> CoreResult<()> {
        Self::write_u32(snapshot, &Self::fee_per_byte_storage_key(), value)
    }

    pub fn set_exec_fee_factor(snapshot: &DataCache, value: u32) -> CoreResult<()> {
        Self::write_u32(snapshot, &Self::exec_fee_factor_storage_key(), value)
    }

    pub fn set_storage_price(snapshot: &DataCache, value: u32) -> CoreResult<()> {
        Self::write_u32(snapshot, &Self::storage_price_storage_key(), value)
    }

    pub fn set_max_block_size(snapshot: &DataCache, value: u32) -> CoreResult<()> {
        Self::write_u32(snapshot, &Self::max_block_size_storage_key(), value)
    }

    pub fn set_max_block_system_fee(snapshot: &DataCache, value: i64) -> CoreResult<()> {
        if value < 0 {
            return Err(CoreError::invalid_argument("max block system fee must be >= 0"));
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot set max block system fee",
            ));
        }
        let bytes = serialize_i64(value)?;
        snapshot.add(
            Self::max_block_system_fee_storage_key(),
            StorageItem::from_bytes(bytes),
        );
        Ok(())
    }

    pub fn set_max_traceable_blocks(snapshot: &DataCache, value: u32) -> CoreResult<()> {
        Self::write_u32(
            snapshot,
            &Self::max_traceable_blocks_storage_key(),
            value,
        )
    }

    pub fn block_account(snapshot: &DataCache, account: &UInt160) -> CoreResult<bool> {
        if account.is_zero() {
            return Err(CoreError::invalid_argument("cannot block zero address"));
        }
        let key = Self::blocked_account_storage_key(account);
        if snapshot.get(&key).is_some() {
            return Ok(false);
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot block account",
            ));
        }
        snapshot.add(key, StorageItem::from_bytes(vec![0x01]));
        Ok(true)
    }

    pub fn unblock_account(snapshot: &DataCache, account: &UInt160) -> CoreResult<bool> {
        let key = Self::blocked_account_storage_key(account);
        if snapshot.get(&key).is_none() {
            return Ok(false);
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot unblock account",
            ));
        }
        snapshot.delete(&key);
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Snapshot helpers (keep the existing API used by the blockchain
    // service).
    // ------------------------------------------------------------------

    pub fn get_max_valid_until_block_increment_snapshot(
        &self,
        _snapshot: &DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> CoreResult<u32> {
        Ok(DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    pub fn get_exec_fee_factor_snapshot(
        &self,
        snapshot: &DataCache,
        _settings: &neo_config::ProtocolSettings,
        _height: u32,
    ) -> CoreResult<u32> {
        Self::get_exec_fee_factor(snapshot)
    }

    pub fn get_fee_per_byte_snapshot(&self, snapshot: &DataCache) -> CoreResult<u32> {
        Self::get_fee_per_byte(snapshot)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    fn read_u32(snapshot: &DataCache, key: &StorageKey, default: u32) -> CoreResult<u32> {
        match snapshot.get(key) {
            Some(item) => {
                let bytes = item.value_bytes();
                if bytes.len() < 4 {
                    return Err(CoreError::invalid_data(format!(
                        "u32 payload too short: {} bytes",
                        bytes.len()
                    )));
                }
                let mut arr = [0u8; 4];
                arr.copy_from_slice(&bytes[..4]);
                Ok(u32::from_le_bytes(arr))
            }
            None => Ok(default),
        }
    }

    fn write_u32(snapshot: &DataCache, key: &StorageKey, value: u32) -> CoreResult<()> {
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write policy value",
            ));
        }
        snapshot.add(key.clone(), StorageItem::from_bytes(value.to_le_bytes().to_vec()));
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

    fn account(byte: u8) -> UInt160 {
        UInt160::from_bytes(&[byte; 20]).unwrap()
    }

    #[test]
    fn test_policy_constants() {
        assert_eq!(PolicyContract::ID, -7);
        assert_eq!(PolicyContract::DEFAULT_FEE_PER_BYTE, 1000);
        assert_eq!(PolicyContract::DEFAULT_EXEC_FEE_FACTOR, 30);
        assert_eq!(PolicyContract::DEFAULT_STORAGE_PRICE, 100_000);
        assert_eq!(PolicyContract::DEFAULT_MAX_BLOCK_SIZE, 1_048_576);
        assert_eq!(PolicyContract::DEFAULT_MAX_TRACEABLE_BLOCKS, 2_102_352);
    }

    #[test]
    fn test_policy_hash() {
        let expected = *POLICY_CONTRACT_HASH;
        assert_eq!(PolicyContract::script_hash(), expected);
        assert_eq!(PolicyContract::new().hash(), expected);
    }

    #[test]
    fn test_default_values_when_uninitialised() {
        let cache = fresh_cache();
        assert_eq!(PolicyContract::get_fee_per_byte(&cache).unwrap(), 1000);
        assert_eq!(PolicyContract::get_exec_fee_factor(&cache).unwrap(), 30);
        assert_eq!(PolicyContract::get_storage_price(&cache).unwrap(), 100_000);
        assert_eq!(PolicyContract::get_max_block_size(&cache).unwrap(), 1_048_576);
        assert_eq!(PolicyContract::get_max_traceable_blocks(&cache).unwrap(), 2_102_352);
    }

    #[test]
    fn test_set_and_get_fee_per_byte() {
        let cache = fresh_cache();
        PolicyContract::set_fee_per_byte(&cache, 500).unwrap();
        assert_eq!(PolicyContract::get_fee_per_byte(&cache).unwrap(), 500);
        PolicyContract::set_fee_per_byte(&cache, 2_000).unwrap();
        assert_eq!(PolicyContract::get_fee_per_byte(&cache).unwrap(), 2_000);
    }

    #[test]
    fn test_set_and_get_exec_fee_factor() {
        let cache = fresh_cache();
        PolicyContract::set_exec_fee_factor(&cache, 100).unwrap();
        assert_eq!(PolicyContract::get_exec_fee_factor(&cache).unwrap(), 100);
    }

    #[test]
    fn test_set_and_get_storage_price() {
        let cache = fresh_cache();
        PolicyContract::set_storage_price(&cache, 50_000).unwrap();
        assert_eq!(PolicyContract::get_storage_price(&cache).unwrap(), 50_000);
    }

    #[test]
    fn test_set_and_get_max_block_size() {
        let cache = fresh_cache();
        PolicyContract::set_max_block_size(&cache, 2_097_152).unwrap();
        assert_eq!(PolicyContract::get_max_block_size(&cache).unwrap(), 2_097_152);
    }

    #[test]
    fn test_set_and_get_max_block_system_fee() {
        let cache = fresh_cache();
        PolicyContract::set_max_block_system_fee(&cache, 1_000_000).unwrap();
        assert_eq!(
            PolicyContract::get_max_block_system_fee(&cache).unwrap(),
            1_000_000
        );
    }

    #[test]
    fn test_set_max_block_system_fee_negative_rejected() {
        let cache = fresh_cache();
        let res = PolicyContract::set_max_block_system_fee(&cache, -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_set_and_get_max_traceable_blocks() {
        let cache = fresh_cache();
        PolicyContract::set_max_traceable_blocks(&cache, 100_000).unwrap();
        assert_eq!(
            PolicyContract::get_max_traceable_blocks(&cache).unwrap(),
            100_000
        );
    }

    #[test]
    fn test_block_unblock_account() {
        let cache = fresh_cache();
        let acct = account(1);
        assert!(!PolicyContract::is_blocked(&cache, &acct).unwrap());

        // Block
        let r = PolicyContract::block_account(&cache, &acct).unwrap();
        assert!(r);
        assert!(PolicyContract::is_blocked(&cache, &acct).unwrap());

        // Re-block returns false (already blocked)
        let r = PolicyContract::block_account(&cache, &acct).unwrap();
        assert!(!r);

        // Unblock
        let r = PolicyContract::unblock_account(&cache, &acct).unwrap();
        assert!(r);
        assert!(!PolicyContract::is_blocked(&cache, &acct).unwrap());

        // Re-unblock returns false
        let r = PolicyContract::unblock_account(&cache, &acct).unwrap();
        assert!(!r);
    }

    #[test]
    fn test_block_zero_address_rejected() {
        let cache = fresh_cache();
        let res = PolicyContract::block_account(&cache, &UInt160::zero());
        assert!(res.is_err());
    }

    #[test]
    fn test_storage_keys_have_correct_prefixes() {
        assert_eq!(
            PolicyContract::fee_per_byte_storage_key().key()[0],
            PREFIX_FEE_PER_BYTE
        );
        assert_eq!(
            PolicyContract::exec_fee_factor_storage_key().key()[0],
            PREFIX_EXEC_FEE_FACTOR
        );
        assert_eq!(
            PolicyContract::storage_price_storage_key().key()[0],
            PREFIX_STORAGE_PRICE
        );
        assert_eq!(
            PolicyContract::max_block_size_storage_key().key()[0],
            PREFIX_MAX_BLOCK_SIZE
        );
        assert_eq!(
            PolicyContract::max_block_system_fee_storage_key().key()[0],
            PREFIX_MAX_BLOCK_SYSTEM_FEE
        );
        assert_eq!(
            PolicyContract::max_traceable_blocks_storage_key().key()[0],
            PREFIX_MAX_TRACEABLE_BLOCKS
        );

        let blocked_key = PolicyContract::blocked_account_storage_key(&account(1));
        assert_eq!(blocked_key.key()[0], PREFIX_BLOCKED_ACCOUNT);
        assert_eq!(blocked_key.key().len(), 21);
    }

    #[test]
    fn test_read_only_cache_rejects_writes() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let res = PolicyContract::set_fee_per_byte(&cache, 100);
        assert!(res.is_err());
    }

    #[test]
    fn test_max_valid_until_block_increment_default() {
        let cache = fresh_cache();
        let pc = PolicyContract::new();
        let settings = neo_config::ProtocolSettings::default();
        let value = pc
            .get_max_valid_until_block_increment_snapshot(&cache, &settings)
            .unwrap();
        assert_eq!(value, DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT);
    }

    #[test]
    fn test_wire_format_u32_little_endian() {
        let cache = fresh_cache();
        PolicyContract::set_fee_per_byte(&cache, 0x1234_5678).unwrap();
        let key = PolicyContract::fee_per_byte_storage_key();
        let item = cache.get(&key).unwrap();
        assert_eq!(item.value_bytes().as_ref(), 0x1234_5678u32.to_le_bytes().as_slice());
    }
}
