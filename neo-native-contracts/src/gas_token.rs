//! GasToken (GAS) native contract.
//!
//! Real (non-stub) implementation of the GAS NEP-17 token. Mirrors the
//! C# `Neo.SmartContract.Native.GasToken` storage layout so the
//! `ApplicationEngine`, services, and plugins can read and write GAS
//! balances byte-for-byte compatible with the C# node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix            | Value (LE i64) |
//! |--------|----------------------|----------------|
//! | 0x14   | 20-byte account hash | account balance |
//! | 0x14   | 32 zero bytes        | total supply    |
//!
//! This module owns the storage-query surface (read / mutate balances,
//! mint, burn, total supply). The transaction-validation and
//! witness-checking surfaces are handled by the application engine +
//! the blockchain service.

use crate::hashes::GAS_TOKEN_HASH;
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `GasToken.PREFIX_BALANCE` (account -> balance).
const PREFIX_BALANCE: u8 = 0x14;

/// Total-supply storage key suffix (32 zero bytes).
const TOTAL_SUPPLY_SUFFIX: [u8; 32] = [0u8; 32];

/// Lazily-initialised script-hash handle for the GAS native contract.
pub static GAS_HASH: LazyLock<UInt160> = LazyLock::new(|| *GAS_TOKEN_HASH);

/// Static accessor for the GasToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct GasToken;

impl GasToken {
    /// Stable native contract id (matches C# `GasToken.Id`).
    pub const ID: i32 = -6;

    /// Constructs a new `GasToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the GAS native contract.
    pub fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    /// Returns the script hash of the GAS native contract (static).
    pub fn script_hash() -> UInt160 {
        *GAS_HASH
    }

    /// The token name (`"GAS"`).
    pub const NAME: &'static str = "GAS";

    /// The token symbol (`"GAS"`).
    pub const SYMBOL: &'static str = "GAS";

    /// The token decimals (8 - matches C# `GasToken.Decimals`).
    pub const DECIMALS: u8 = 8;

    /// C# `GasToken.TotalSupply` initial value (52,000,000 GAS with
    /// 8 decimals). Persisted to storage on first mint.
    pub const INITIAL_TOTAL_SUPPLY: i64 = 52_000_000 * 100_000_000;

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    /// Storage key for an account's GAS balance.
    ///
    /// Matches C# `GasToken.CreateBalanceKey`:
    /// `StorageKey(contract=Id, key=[PREFIX_BALANCE, account_hash_be])`.
    #[inline]
    pub fn balance_storage_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_BALANCE, account)
    }

    /// Storage key for the total supply record.
    ///
    /// Matches C# `GasToken.CreateTotalSupplyKey`:
    /// `StorageKey(contract=Id, key=[PREFIX_BALANCE, 32 zeros])`.
    #[inline]
    pub fn total_supply_storage_key() -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_BALANCE, &TOTAL_SUPPLY_SUFFIX)
    }

    // ------------------------------------------------------------------
    // Read-only surface
    // ------------------------------------------------------------------

    /// Returns the GAS balance of `account` (0 when uninitialised).
    pub fn balance_of(snapshot: &DataCache, account: &UInt160) -> i64 {
        let key = Self::balance_storage_key(account);
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(0),
            None => 0,
        }
    }

    /// Returns the current total supply of GAS.
    pub fn total_supply(snapshot: &DataCache) -> i64 {
        let key = Self::total_supply_storage_key();
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(0),
            None => 0,
        }
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    /// Mints `amount` GAS to `account`, increasing the total supply.
    pub fn mint(snapshot: &DataCache, account: &UInt160, amount: i64) -> CoreResult<()> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("amount must be non-negative"));
        }
        if amount == 0 {
            return Ok(());
        }
        if account.is_zero() {
            return Err(CoreError::invalid_argument("cannot mint to zero address"));
        }

        let current = Self::balance_of(snapshot, account);
        let key = Self::balance_storage_key(account);
        let new_balance = current
            .checked_add(amount)
            .ok_or_else(|| CoreError::native_contract("GAS balance overflow"))?;
        Self::write_balance(snapshot, &key, new_balance)?;

        let supply = Self::total_supply(snapshot);
        let new_supply = supply
            .checked_add(amount)
            .ok_or_else(|| CoreError::native_contract("GAS total supply overflow"))?;
        Self::write_total_supply(snapshot, new_supply)?;
        Ok(())
    }

    /// Burns `amount` GAS from `account`, decreasing the total supply.
    pub fn burn(snapshot: &DataCache, account: &UInt160, amount: i64) -> CoreResult<()> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("amount must be non-negative"));
        }
        if amount == 0 {
            return Ok(());
        }
        let current = Self::balance_of(snapshot, account);
        if current < amount {
            return Err(CoreError::native_contract(format!(
                "GAS balance {current} < burn amount {amount}"
            )));
        }
        let key = Self::balance_storage_key(account);
        Self::write_balance(snapshot, &key, current - amount)?;

        let supply = Self::total_supply(snapshot);
        Self::write_total_supply(snapshot, supply - amount)?;
        Ok(())
    }

    /// Transfers `amount` GAS from `from` -> `to`.
    ///
    /// Returns `Ok(true)` on success, `Ok(false)` if the sender does
    /// not have enough balance (matches C# NEP-17 `transfer` return
    /// semantics), and `Err` only for programmer errors
    /// (negative amount, storage overflow).
    pub fn transfer(
        snapshot: &DataCache,
        from: &UInt160,
        to: &UInt160,
        amount: i64,
    ) -> CoreResult<bool> {
        if amount < 0 {
            return Err(CoreError::invalid_argument("amount must be non-negative"));
        }
        if from == to || amount == 0 {
            return Ok(true);
        }
        let from_balance = Self::balance_of(snapshot, from);
        if from_balance < amount {
            return Ok(false);
        }
        let to_balance = Self::balance_of(snapshot, to);

        Self::write_balance(
            snapshot,
            &Self::balance_storage_key(from),
            from_balance - amount,
        )?;
        Self::write_balance(
            snapshot,
            &Self::balance_storage_key(to),
            to_balance
                .checked_add(amount)
                .ok_or_else(|| CoreError::native_contract("GAS balance overflow"))?,
        )?;
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    fn write_balance(snapshot: &DataCache, key: &StorageKey, value: i64) -> CoreResult<()> {
        let bytes = serialize_i64(value)?;
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write GAS balance",
            ));
        }
        snapshot.add(key.clone(), StorageItem::from_bytes(bytes));
        Ok(())
    }

    fn write_total_supply(snapshot: &DataCache, value: i64) -> CoreResult<()> {
        let bytes = serialize_i64(value)?;
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write GAS total supply",
            ));
        }
        snapshot.add(
            Self::total_supply_storage_key(),
            StorageItem::from_bytes(bytes),
        );
        Ok(())
    }
}

// ============================================================================
// Wire-format helpers (i64 little-endian, matches C# BigInteger wire format
// for values in the i64 range used by the GAS / NEO token contracts).
// ============================================================================

/// Serialise an `i64` as 8 little-endian bytes (matches C# wire format
/// for in-range `BigInteger`s emitted by the GAS contract).
pub fn serialize_i64(value: i64) -> CoreResult<Vec<u8>> {
    let mut writer = BinaryWriter::new();
    writer
        .write_i64(value)
        .map_err(|e| CoreError::serialization(e.to_string()))?;
    Ok(writer.into_bytes())
}

/// Deserialise an `i64` from 8 little-endian bytes.
pub fn deserialize_i64(bytes: &[u8]) -> CoreResult<i64> {
    if bytes.len() < 8 {
        return Err(CoreError::invalid_data(format!(
            "i64 payload too short: {} bytes",
            bytes.len()
        )));
    }
    let mut reader = MemoryReader::new(&bytes[..8]);
    reader
        .read_i64()
        .map_err(|e| CoreError::deserialization(e.to_string()))
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
    fn test_gas_token_constants() {
        assert_eq!(GasToken::ID, -6);
        assert_eq!(GasToken::DECIMALS, 8);
        assert_eq!(GasToken::SYMBOL, "GAS");
        assert_eq!(GasToken::NAME, "GAS");
        assert_eq!(GasToken::INITIAL_TOTAL_SUPPLY, 5_200_000_000_000_000);
    }

    #[test]
    fn test_gas_token_hash() {
        let expected = *GAS_TOKEN_HASH;
        assert_eq!(GasToken::script_hash(), expected);
        assert_eq!(GasToken::new().hash(), expected);
    }

    #[test]
    fn test_balance_of_empty() {
        let cache = fresh_cache();
        assert_eq!(GasToken::balance_of(&cache, &account(1)), 0);
        assert_eq!(GasToken::total_supply(&cache), 0);
    }

    #[test]
    fn test_mint_increases_balance_and_supply() {
        let cache = fresh_cache();
        let acct = account(1);

        GasToken::mint(&cache, &acct, 1_000).unwrap();
        assert_eq!(GasToken::balance_of(&cache, &acct), 1_000);
        assert_eq!(GasToken::total_supply(&cache), 1_000);

        GasToken::mint(&cache, &acct, 2_500).unwrap();
        assert_eq!(GasToken::balance_of(&cache, &acct), 3_500);
        assert_eq!(GasToken::total_supply(&cache), 3_500);
    }

    #[test]
    fn test_mint_to_zero_address_rejected() {
        let cache = fresh_cache();
        let res = GasToken::mint(&cache, &UInt160::zero(), 100);
        assert!(res.is_err());
    }

    #[test]
    fn test_mint_zero_is_noop() {
        let cache = fresh_cache();
        GasToken::mint(&cache, &account(1), 0).unwrap();
        assert_eq!(GasToken::balance_of(&cache, &account(1)), 0);
        assert_eq!(GasToken::total_supply(&cache), 0);
    }

    #[test]
    fn test_mint_negative_rejected() {
        let cache = fresh_cache();
        let res = GasToken::mint(&cache, &account(1), -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_burn_decreases_balance_and_supply() {
        let cache = fresh_cache();
        let acct = account(2);

        GasToken::mint(&cache, &acct, 1_000).unwrap();
        GasToken::burn(&cache, &acct, 300).unwrap();

        assert_eq!(GasToken::balance_of(&cache, &acct), 700);
        assert_eq!(GasToken::total_supply(&cache), 700);
    }

    #[test]
    fn test_burn_more_than_balance_errors() {
        let cache = fresh_cache();
        let acct = account(2);

        GasToken::mint(&cache, &acct, 100).unwrap();
        let res = GasToken::burn(&cache, &acct, 200);
        assert!(res.is_err());
        // State unchanged
        assert_eq!(GasToken::balance_of(&cache, &acct), 100);
    }

    #[test]
    fn test_burn_zero_is_noop() {
        let cache = fresh_cache();
        let acct = account(2);
        GasToken::mint(&cache, &acct, 100).unwrap();
        GasToken::burn(&cache, &acct, 0).unwrap();
        assert_eq!(GasToken::balance_of(&cache, &acct), 100);
    }

    #[test]
    fn test_transfer_success() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);

        GasToken::mint(&cache, &from, 1_000).unwrap();
        let result = GasToken::transfer(&cache, &from, &to, 300).unwrap();
        assert!(result);

        assert_eq!(GasToken::balance_of(&cache, &from), 700);
        assert_eq!(GasToken::balance_of(&cache, &to), 300);
        // Total supply unchanged
        assert_eq!(GasToken::total_supply(&cache), 1_000);
    }

    #[test]
    fn test_transfer_insufficient_returns_false() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);

        GasToken::mint(&cache, &from, 100).unwrap();
        let result = GasToken::transfer(&cache, &from, &to, 500).unwrap();
        assert!(!result);

        assert_eq!(GasToken::balance_of(&cache, &from), 100);
        assert_eq!(GasToken::balance_of(&cache, &to), 0);
    }

    #[test]
    fn test_transfer_zero_amount_always_true() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);

        let result = GasToken::transfer(&cache, &from, &to, 0).unwrap();
        assert!(result);
    }

    #[test]
    fn test_transfer_self_noop() {
        let cache = fresh_cache();
        let me = account(1);

        GasToken::mint(&cache, &me, 1_000).unwrap();
        let result = GasToken::transfer(&cache, &me, &me, 500).unwrap();
        assert!(result);
        assert_eq!(GasToken::balance_of(&cache, &me), 1_000);
    }

    #[test]
    fn test_transfer_negative_amount_rejected() {
        let cache = fresh_cache();
        let res = GasToken::transfer(&cache, &account(1), &account(2), -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_wire_format_i64_roundtrip() {
        let value: i64 = 1_234_567_890_123_456;
        let bytes = serialize_i64(value).unwrap();
        assert_eq!(bytes, value.to_le_bytes());
        assert_eq!(deserialize_i64(&bytes).unwrap(), value);
    }

    #[test]
    fn test_wire_format_negative_i64() {
        let value: i64 = -42;
        let bytes = serialize_i64(value).unwrap();
        assert_eq!(bytes, value.to_le_bytes());
        assert_eq!(deserialize_i64(&bytes).unwrap(), value);
    }

    #[test]
    fn test_wire_format_initial_total_supply() {
        let value = GasToken::INITIAL_TOTAL_SUPPLY;
        let bytes = serialize_i64(value).unwrap();
        assert_eq!(bytes, value.to_le_bytes());
        assert_eq!(bytes, 5_200_000_000_000_000_i64.to_le_bytes());
    }

    #[test]
    fn test_balance_storage_key_uses_be_bytes() {
        let acct = account(1);
        let key = GasToken::balance_storage_key(&acct);
        assert_eq!(key.id(), GasToken::ID);
        // 1 byte prefix + 20 byte account hash
        assert_eq!(key.key().len(), 21);
        assert_eq!(key.key()[0], PREFIX_BALANCE);
        assert_eq!(&key.key()[1..], &acct.to_bytes());
    }

    #[test]
    fn test_total_supply_storage_key_is_33_bytes() {
        let key = GasToken::total_supply_storage_key();
        assert_eq!(key.id(), GasToken::ID);
        // 1 byte prefix + 32 zero bytes
        assert_eq!(key.key().len(), 33);
        assert_eq!(key.key()[0], PREFIX_BALANCE);
        assert_eq!(&key.key()[1..], &[0u8; 32]);
    }

    #[test]
    fn test_read_only_cache_rejects_writes() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let res = GasToken::mint(&cache, &account(1), 100);
        assert!(res.is_err());
    }
}
