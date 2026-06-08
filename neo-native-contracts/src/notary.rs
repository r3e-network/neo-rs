//! Notary native contract.
//!
//! Real (non-stub) implementation of the P2P notary contract. Mirrors
//! the C# `Neo.SmartContract.Native.Notary` storage layout so the
//! notary service and the application engine can read and write
//! deposit / lock state byte-for-byte compatible with the C# node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix            | Value          |
//! |--------|----------------------|----------------|
//! | 0x10   | 20-byte account hash | LE i64 deposit |
//! | 0x11   | 20-byte account hash | LE i64 max fee |
//! | 0x12   | 32-byte tx hash      | (lock marker)  |

use crate::hashes::NOTARY_HASH;
use crate::gas_token::{deserialize_i64, serialize_i64};
use neo_error::{CoreError, CoreResult};
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `Notary.PREFIX_DEPOSIT` (account -> deposit amount).
const PREFIX_DEPOSIT: u8 = 0x10;
/// C# `Notary.PREFIX_MAX_FEE` (account -> max fee per key).
const PREFIX_MAX_FEE: u8 = 0x11;
/// C# `Notary.PREFIX_WITHDRAWAL` (tx hash -> withdrawal lock).
const PREFIX_WITHDRAWAL: u8 = 0x12;

/// Lazily-initialised script-hash handle for the Notary contract.
pub static NOTARY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *NOTARY_HASH);

/// Default max notary service fee per key (matches C# `Notary.DefaultMaxNotaryServiceFeePerKey`).
pub const DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY: i64 = 1_000_000; // 0.01 GAS in raw units

/// Static accessor for the Notary native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Notary;

impl Notary {
    /// Stable native contract id (matches C# `Notary.Id`).
    pub const ID: i32 = -10;

    /// Constructs a new `Notary` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Notary contract.
    pub fn hash(&self) -> UInt160 {
        *NOTARY_HASH_REF
    }

    /// Returns the script hash of the Notary contract (static).
    pub fn script_hash() -> UInt160 {
        *NOTARY_HASH_REF
    }

    /// Default max notary service fee per key.
    pub const DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY: i64 =
        DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY;

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    /// Storage key for an account's deposit.
    #[inline]
    pub fn deposit_storage_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_DEPOSIT, account)
    }

    /// Storage key for an account's max fee per key.
    #[inline]
    pub fn max_fee_storage_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_MAX_FEE, account)
    }

    /// Storage key for a withdrawal lock.
    #[inline]
    pub fn withdrawal_storage_key(tx_hash: &UInt256) -> StorageKey {
        StorageKey::create_with_uint256(Self::ID, PREFIX_WITHDRAWAL, tx_hash)
    }

    // ------------------------------------------------------------------
    // Read-only surface
    // ------------------------------------------------------------------

    /// Returns the current deposit balance for `account`.
    pub fn balance_of(snapshot: &DataCache, account: &UInt160) -> CoreResult<i64> {
        let key = Self::deposit_storage_key(account);
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()),
            None => Ok(0),
        }
    }

    /// Returns the max notary service fee per key (default when
    /// uninitialised).
    pub fn get_max_notary_service_fee_per_key(snapshot: &DataCache) -> CoreResult<i64> {
        let key = Self::max_fee_storage_key(&UInt160::zero());
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()),
            None => Ok(Self::DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY),
        }
    }

    /// Returns `true` if the given transaction hash is locked.
    pub fn is_withdraw_locked(snapshot: &DataCache, tx_hash: &UInt256) -> CoreResult<bool> {
        let key = Self::withdrawal_storage_key(tx_hash);
        Ok(snapshot.get(&key).is_some())
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    /// Locks `amount` GAS from `account` as notary deposit.
    pub fn lock_deposit(
        snapshot: &DataCache,
        account: &UInt160,
        amount: i64,
    ) -> CoreResult<()> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("deposit must be non-negative"));
        }
        if account.is_zero() {
            return Err(CoreError::invalid_argument("cannot deposit from zero address"));
        }
        let current = Self::balance_of(snapshot, account)?;
        let key = Self::deposit_storage_key(account);
        let new_balance = current
            .checked_add(amount)
            .ok_or_else(|| CoreError::native_contract("notary deposit overflow"))?;
        Self::write_i64(snapshot, &key, new_balance)?;
        Ok(())
    }

    /// Unlocks `amount` GAS from `account`'s notary deposit.
    pub fn unlock_deposit(
        snapshot: &DataCache,
        account: &UInt160,
        amount: i64,
    ) -> CoreResult<()> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("unlock must be non-negative"));
        }
        let current = Self::balance_of(snapshot, account)?;
        if current < amount {
            return Err(CoreError::native_contract(format!(
                "notary deposit {current} < unlock {amount}"
            )));
        }
        let key = Self::deposit_storage_key(account);
        Self::write_i64(snapshot, &key, current - amount)?;
        Ok(())
    }

    /// Withdraws the entire deposit of `account`.
    pub fn withdraw(
        snapshot: &DataCache,
        account: &UInt160,
        tx_hash: &UInt256,
    ) -> CoreResult<i64> {
        if Self::is_withdraw_locked(snapshot, tx_hash)? {
            return Err(CoreError::native_contract("withdraw already locked for tx"));
        }
        let current = Self::balance_of(snapshot, account)?;
        if current == 0 {
            return Ok(0);
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot withdraw",
            ));
        }
        // Lock the withdrawal.
        snapshot.add(
            Self::withdrawal_storage_key(tx_hash),
            StorageItem::from_bytes(Vec::new()),
        );
        // Clear the deposit.
        snapshot.delete(&Self::deposit_storage_key(account));
        Ok(current)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    fn write_i64(snapshot: &DataCache, key: &StorageKey, value: i64) -> CoreResult<()> {
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write notary value",
            ));
        }
        snapshot.add(key.clone(), StorageItem::from_bytes(serialize_i64(value)?));
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

    fn tx_hash(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).unwrap()
    }

    #[test]
    fn test_notary_constants() {
        assert_eq!(Notary::ID, -10);
        assert_eq!(Notary::DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY, 1_000_000);
    }

    #[test]
    fn test_notary_hash() {
        let expected = *NOTARY_HASH;
        assert_eq!(Notary::script_hash(), expected);
        assert_eq!(Notary::new().hash(), expected);
    }

    #[test]
    fn test_balance_of_empty() {
        let cache = fresh_cache();
        assert_eq!(Notary::balance_of(&cache, &account(1)).unwrap(), 0);
    }

    #[test]
    fn test_lock_deposit() {
        let cache = fresh_cache();
        let acct = account(1);
        Notary::lock_deposit(&cache, &acct, 1_000).unwrap();
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 1_000);
        Notary::lock_deposit(&cache, &acct, 2_500).unwrap();
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 3_500);
    }

    #[test]
    fn test_lock_deposit_zero_is_noop() {
        let cache = fresh_cache();
        let acct = account(1);
        Notary::lock_deposit(&cache, &acct, 0).unwrap();
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 0);
    }

    #[test]
    fn test_lock_deposit_negative_rejected() {
        let cache = fresh_cache();
        let res = Notary::lock_deposit(&cache, &account(1), -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_lock_deposit_zero_address_rejected() {
        let cache = fresh_cache();
        let res = Notary::lock_deposit(&cache, &UInt160::zero(), 100);
        assert!(res.is_err());
    }

    #[test]
    fn test_unlock_deposit() {
        let cache = fresh_cache();
        let acct = account(2);
        Notary::lock_deposit(&cache, &acct, 1_000).unwrap();
        Notary::unlock_deposit(&cache, &acct, 300).unwrap();
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 700);
    }

    #[test]
    fn test_unlock_more_than_deposit_errors() {
        let cache = fresh_cache();
        let acct = account(2);
        Notary::lock_deposit(&cache, &acct, 100).unwrap();
        let res = Notary::unlock_deposit(&cache, &acct, 200);
        assert!(res.is_err());
        // State unchanged
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 100);
    }

    #[test]
    fn test_withdraw_returns_balance_and_clears() {
        let cache = fresh_cache();
        let acct = account(3);
        let tx = tx_hash(1);
        Notary::lock_deposit(&cache, &acct, 1_000).unwrap();
        let amount = Notary::withdraw(&cache, &acct, &tx).unwrap();
        assert_eq!(amount, 1_000);
        assert_eq!(Notary::balance_of(&cache, &acct).unwrap(), 0);
        assert!(Notary::is_withdraw_locked(&cache, &tx).unwrap());
    }

    #[test]
    fn test_withdraw_no_deposit_returns_zero() {
        let cache = fresh_cache();
        let acct = account(4);
        let tx = tx_hash(2);
        let amount = Notary::withdraw(&cache, &acct, &tx).unwrap();
        assert_eq!(amount, 0);
        // No lock should be created for an empty withdraw.
        assert!(!Notary::is_withdraw_locked(&cache, &tx).unwrap());
    }

    #[test]
    fn test_withdraw_twice_for_same_tx_rejected() {
        let cache = fresh_cache();
        let acct = account(5);
        let tx = tx_hash(3);
        Notary::lock_deposit(&cache, &acct, 1_000).unwrap();
        Notary::withdraw(&cache, &acct, &tx).unwrap();
        // Second withdraw for same tx hash should be rejected.
        let res = Notary::withdraw(&cache, &acct, &tx);
        assert!(res.is_err());
    }

    #[test]
    fn test_default_max_notary_service_fee_per_key() {
        let cache = fresh_cache();
        assert_eq!(
            Notary::get_max_notary_service_fee_per_key(&cache).unwrap(),
            Notary::DEFAULT_MAX_NOTARY_SERVICE_FEE_PER_KEY
        );
    }

    #[test]
    fn test_deposit_storage_key_format() {
        let key = Notary::deposit_storage_key(&account(1));
        assert_eq!(key.id(), Notary::ID);
        assert_eq!(key.key()[0], PREFIX_DEPOSIT);
        assert_eq!(key.key().len(), 21);
    }

    #[test]
    fn test_withdrawal_storage_key_format() {
        let key = Notary::withdrawal_storage_key(&tx_hash(1));
        assert_eq!(key.id(), Notary::ID);
        assert_eq!(key.key()[0], PREFIX_WITHDRAWAL);
        assert_eq!(key.key().len(), 33);
    }

    #[test]
    fn test_read_only_cache_rejects_lock() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let res = Notary::lock_deposit(&cache, &account(1), 100);
        assert!(res.is_err());
    }
}
