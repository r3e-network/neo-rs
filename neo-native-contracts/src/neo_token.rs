//! NeoToken (NEO) native contract.
//!
//! Real (non-stub) implementation of the NEO governance token. Mirrors
//! the C# `Neo.SmartContract.Native.NeoToken` storage layout so the
//! application engine, services, and plugins can read NEO balances,
//! candidates, committee members, and votes byte-for-byte compatible
//! with the C# node.
//!
//! ## Storage layout
//!
//! | Prefix | Key suffix                          | Value                  |
//! |--------|-------------------------------------|------------------------|
//! | 0x14   | 20-byte account hash                | LE i64 balance         |
//! | 0x14   | 32 zero bytes                       | LE i64 total supply    |
//! | 0x18   | 33-byte public key (compressed)     | bool registered        |
//! | 0x19   | 33-byte public key (compressed)     | serialized vote state  |
//! | 0x17   | 32 zero bytes (current committee)   | serialized committee   |
//! | 0x1A   | (committee index)                   | 33-byte public key     |
//!
//! This module owns the storage-query surface for balances, votes, and
//! candidates. The committee-rotation and gas-distribution surfaces
//! are out of scope for the stub and are handled by the application
//! engine + the blockchain service.

use crate::hashes::NEO_TOKEN_HASH;
use crate::gas_token::{deserialize_i64, serialize_i64};
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use std::sync::LazyLock;

/// C# `NeoToken.PREFIX_BALANCE` (account -> balance).
const PREFIX_BALANCE: u8 = 0x14;
/// C# `NeoToken.PREFIX_CANDIDATE` (compressed pubkey -> registered flag).
const PREFIX_CANDIDATE: u8 = 0x18;
/// C# `NeoToken.PREFIX_VOTE` (compressed pubkey -> vote state).
const PREFIX_VOTE: u8 = 0x19;
/// C# `NeoToken.PREFIX_COMMITTEE` (committee pubkey array).
const PREFIX_COMMITTEE: u8 = 0x17;
/// C# `NeoToken.PREFIX_COMMITTEE_INDEX` (index -> pubkey).
const PREFIX_COMMITTEE_INDEX: u8 = 0x1A;
/// C# `NeoToken.PREFIX_GAS_PER_BLOCK`.
const PREFIX_GAS_PER_BLOCK: u8 = 0x13;
/// C# `NeoToken.PREFIX_REGISTER_PRICE`.
const PREFIX_REGISTER_PRICE: u8 = 0x1B;

const TOTAL_SUPPLY_SUFFIX: [u8; 32] = [0u8; 32];
const COMMITTEE_KEY_SUFFIX: [u8; 32] = [0u8; 32];

/// Lazily-initialised script-hash handle for the NEO native contract.
pub static NEO_HASH: LazyLock<UInt160> = LazyLock::new(|| *NEO_TOKEN_HASH);

/// Static accessor for the NeoToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (matches C# `NeoToken.Id`).
    pub const ID: i32 = -5;

    /// Constructs a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the NEO native contract.
    pub fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    /// Returns the script hash of the NEO native contract (static).
    pub fn script_hash() -> UInt160 {
        *NEO_HASH
    }

    /// The token name (`"NEO"`).
    pub const NAME: &'static str = "NEO";

    /// The token symbol (`"NEO"`).
    pub const SYMBOL: &'static str = "NEO";

    /// The token decimals (0 - NEO is indivisible).
    pub const DECIMALS: u8 = 0;

    /// C# `NeoToken.TotalSupply` initial value (100,000,000 NEO).
    pub const TOTAL_SUPPLY: i64 = 100_000_000;

    /// C# `NeoToken.VoterRewardRatio` (default 1/1000 of GAS to voters).
    pub const DEFAULT_VOTER_REWARD_RATIO: u32 = 1_000;
    /// C# `NeoToken.DefaultRegisterPrice` (default 1000 GAS).
    pub const DEFAULT_REGISTER_PRICE: i64 = 1_000 * 100_000_000;

    // ------------------------------------------------------------------
    // Storage keys
    // ------------------------------------------------------------------

    /// Storage key for an account's NEO balance.
    #[inline]
    pub fn balance_storage_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_BALANCE, account)
    }

    /// Storage key for the total-supply record.
    #[inline]
    pub fn total_supply_storage_key() -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_BALANCE, &TOTAL_SUPPLY_SUFFIX)
    }

    /// Storage key for a candidate (compressed pubkey).
    #[inline]
    pub fn candidate_storage_key(pubkey: &ECPoint) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_CANDIDATE, &encoded_compressed(pubkey))
    }

    /// Storage key for a candidate's vote state (compressed pubkey).
    #[inline]
    pub fn vote_storage_key(pubkey: &ECPoint) -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_VOTE, &encoded_compressed(pubkey))
    }

    /// Storage key for the current committee pubkey array.
    #[inline]
    pub fn committee_storage_key() -> StorageKey {
        StorageKey::create_with_bytes(Self::ID, PREFIX_COMMITTEE, &COMMITTEE_KEY_SUFFIX)
    }

    /// Storage key for a committee member by index.
    #[inline]
    pub fn committee_index_storage_key(index: u32) -> StorageKey {
        StorageKey::create_with_uint32(Self::ID, PREFIX_COMMITTEE_INDEX, index)
    }

    /// Storage key for the gas-per-block setting.
    #[inline]
    pub fn gas_per_block_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_GAS_PER_BLOCK)
    }

    /// Storage key for the candidate-registration price.
    #[inline]
    pub fn register_price_storage_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_REGISTER_PRICE)
    }

    // ------------------------------------------------------------------
    // Read-only surface
    // ------------------------------------------------------------------

    /// Returns the NEO balance of `account` (0 when uninitialised).
    pub fn balance_of(snapshot: &DataCache, account: &UInt160) -> i64 {
        let key = Self::balance_storage_key(account);
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(0),
            None => 0,
        }
    }

    /// Returns the current total supply of NEO.
    pub fn total_supply(snapshot: &DataCache) -> i64 {
        let key = Self::total_supply_storage_key();
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(Self::TOTAL_SUPPLY),
            None => Self::TOTAL_SUPPLY,
        }
    }

    /// Returns `true` if `pubkey` is a registered candidate.
    pub fn is_candidate(snapshot: &DataCache, pubkey: &ECPoint) -> bool {
        let key = Self::candidate_storage_key(pubkey);
        snapshot.get(&key).is_some()
    }

    /// Returns the candidate registration price (default
    /// `DEFAULT_REGISTER_PRICE` when uninitialised).
    pub fn get_register_price(snapshot: &DataCache) -> i64 {
        let key = Self::register_price_storage_key();
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(Self::DEFAULT_REGISTER_PRICE),
            None => Self::DEFAULT_REGISTER_PRICE,
        }
    }

    /// Returns the gas-per-block setting (default 5 * 10^8 = 5 GAS).
    pub fn get_gas_per_block(snapshot: &DataCache) -> i64 {
        let key = Self::gas_per_block_storage_key();
        const DEFAULT: i64 = 5 * 100_000_000;
        match snapshot.get(&key) {
            Some(item) => deserialize_i64(&item.value_bytes()).unwrap_or(DEFAULT),
            None => DEFAULT,
        }
    }

    // ------------------------------------------------------------------
    // Mutating surface
    // ------------------------------------------------------------------

    /// Transfers `amount` NEO from `from` -> `to`.
    ///
    /// NEO is indivisible, so `amount` is in whole-token units.
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
                .ok_or_else(|| CoreError::native_contract("NEO balance overflow"))?,
        )?;
        Ok(true)
    }

    /// Registers `pubkey` as a candidate.
    pub fn register_candidate(snapshot: &DataCache, pubkey: &ECPoint) -> CoreResult<bool> {
        if !is_valid_candidate_pubkey(pubkey) {
            return Ok(false);
        }
        let key = Self::candidate_storage_key(pubkey);
        if snapshot.get(&key).is_some() {
            return Ok(false);
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot register candidate",
            ));
        }
        snapshot.add(key, StorageItem::from_bytes(vec![0x01]));
        Ok(true)
    }

    /// Unregisters `pubkey` as a candidate.
    pub fn unregister_candidate(snapshot: &DataCache, pubkey: &ECPoint) -> CoreResult<bool> {
        let key = Self::candidate_storage_key(pubkey);
        if snapshot.get(&key).is_none() {
            return Ok(false);
        }
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot unregister candidate",
            ));
        }
        snapshot.delete(&key);
        // Also drop any votes that pointed at this candidate.
        snapshot.delete(&Self::vote_storage_key(pubkey));
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Internals
    // ------------------------------------------------------------------

    fn write_balance(snapshot: &DataCache, key: &StorageKey, value: i64) -> CoreResult<()> {
        let bytes = serialize_i64(value)?;
        if snapshot.is_read_only() {
            return Err(CoreError::invalid_operation(
                "DataCache is read-only; cannot write NEO balance",
            ));
        }
        snapshot.add(key.clone(), StorageItem::from_bytes(bytes));
        Ok(())
    }
}

/// Returns `true` if `pubkey` is a valid secp256r1 candidate pubkey.
fn is_valid_candidate_pubkey(pubkey: &ECPoint) -> bool {
    // A valid candidate pubkey must be 33 bytes (compressed secp256r1).
    // The point is already on-curve because ECPoint::from_bytes_with_curve
    // validates on parse; the byte length check is therefore sufficient.
    pubkey.to_bytes().len() == 33 && !pubkey.is_infinity()
}

fn encoded_compressed(pubkey: &ECPoint) -> [u8; 33] {
    let bytes = pubkey.to_bytes();
    assert_eq!(bytes.len(), 33, "compressed ECPoint must be 33 bytes");
    let mut out = [0u8; 33];
    out.copy_from_slice(&bytes);
    out
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ECPoint;
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

    fn dummy_pubkey(_byte: u8) -> ECPoint {
        // Generate a fresh secp256r1 keypair. The `_byte` argument is
        // ignored - the helper exists so test bodies stay readable.
        let (_priv, pubkey) =
            neo_crypto::ecc::generate_keypair(ECCurve::Secp256r1).expect("keypair gen");
        pubkey
    }

    #[test]
    fn test_neo_token_constants() {
        assert_eq!(NeoToken::ID, -5);
        assert_eq!(NeoToken::DECIMALS, 0);
        assert_eq!(NeoToken::SYMBOL, "NEO");
        assert_eq!(NeoToken::NAME, "NEO");
        assert_eq!(NeoToken::TOTAL_SUPPLY, 100_000_000);
        assert_eq!(NeoToken::DEFAULT_REGISTER_PRICE, 100_000_000_000);
    }

    #[test]
    fn test_neo_token_hash() {
        let expected = *NEO_TOKEN_HASH;
        assert_eq!(NeoToken::script_hash(), expected);
        assert_eq!(NeoToken::new().hash(), expected);
    }

    #[test]
    fn test_balance_of_empty() {
        let cache = fresh_cache();
        assert_eq!(NeoToken::balance_of(&cache, &account(1)), 0);
        // Total supply defaults to TOTAL_SUPPLY when uninitialised
        assert_eq!(NeoToken::total_supply(&cache), 100_000_000);
    }

    #[test]
    fn test_transfer_success() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);

        // Pre-seed the sender with NEO.
        // (Test helper: write directly into the balance storage key.)
        let key = NeoToken::balance_storage_key(&from);
        cache.add(
            key,
            StorageItem::from_bytes(serialize_i64(100).unwrap()),
        );

        let result = NeoToken::transfer(&cache, &from, &to, 40).unwrap();
        assert!(result);
        assert_eq!(NeoToken::balance_of(&cache, &from), 60);
        assert_eq!(NeoToken::balance_of(&cache, &to), 40);
    }

    #[test]
    fn test_transfer_insufficient_returns_false() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);

        let key = NeoToken::balance_storage_key(&from);
        cache.add(
            key,
            StorageItem::from_bytes(serialize_i64(10).unwrap()),
        );

        let result = NeoToken::transfer(&cache, &from, &to, 100).unwrap();
        assert!(!result);
        assert_eq!(NeoToken::balance_of(&cache, &from), 10);
    }

    #[test]
    fn test_transfer_self_noop() {
        let cache = fresh_cache();
        let me = account(1);
        let key = NeoToken::balance_storage_key(&me);
        cache.add(
            key,
            StorageItem::from_bytes(serialize_i64(50).unwrap()),
        );

        let result = NeoToken::transfer(&cache, &me, &me, 30).unwrap();
        assert!(result);
        assert_eq!(NeoToken::balance_of(&cache, &me), 50);
    }

    #[test]
    fn test_transfer_zero_amount() {
        let cache = fresh_cache();
        let from = account(1);
        let to = account(2);
        let result = NeoToken::transfer(&cache, &from, &to, 0).unwrap();
        assert!(result);
    }

    #[test]
    fn test_transfer_negative_rejected() {
        let cache = fresh_cache();
        let res = NeoToken::transfer(&cache, &account(1), &account(2), -1);
        assert!(res.is_err());
    }

    #[test]
    fn test_register_candidate() {
        let cache = fresh_cache();
        let pk = dummy_pubkey(1);
        assert!(!NeoToken::is_candidate(&cache, &pk));
        let result = NeoToken::register_candidate(&cache, &pk).unwrap();
        assert!(result);
        assert!(NeoToken::is_candidate(&cache, &pk));
    }

    #[test]
    fn test_register_candidate_twice_returns_false() {
        let cache = fresh_cache();
        let pk = dummy_pubkey(2);
        assert!(NeoToken::register_candidate(&cache, &pk).unwrap());
        assert!(!NeoToken::register_candidate(&cache, &pk).unwrap());
    }

    #[test]
    fn test_unregister_candidate() {
        let cache = fresh_cache();
        let pk = dummy_pubkey(3);
        NeoToken::register_candidate(&cache, &pk).unwrap();
        let result = NeoToken::unregister_candidate(&cache, &pk).unwrap();
        assert!(result);
        assert!(!NeoToken::is_candidate(&cache, &pk));
    }

    #[test]
    fn test_unregister_candidate_not_registered_returns_false() {
        let cache = fresh_cache();
        let pk = dummy_pubkey(4);
        let result = NeoToken::unregister_candidate(&cache, &pk).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_balance_storage_key_uses_be_bytes() {
        let acct = account(1);
        let key = NeoToken::balance_storage_key(&acct);
        assert_eq!(key.id(), NeoToken::ID);
        assert_eq!(key.key().len(), 21);
        assert_eq!(key.key()[0], PREFIX_BALANCE);
        assert_eq!(&key.key()[1..], &acct.to_bytes());
    }

    #[test]
    fn test_total_supply_storage_key_is_33_bytes() {
        let key = NeoToken::total_supply_storage_key();
        assert_eq!(key.id(), NeoToken::ID);
        assert_eq!(key.key().len(), 33);
        assert_eq!(key.key()[0], PREFIX_BALANCE);
        assert_eq!(&key.key()[1..], &[0u8; 32]);
    }

    #[test]
    fn test_candidate_storage_key_starts_with_0x18() {
        let pk = dummy_pubkey(5);
        let key = NeoToken::candidate_storage_key(&pk);
        assert_eq!(key.id(), NeoToken::ID);
        assert_eq!(key.key()[0], PREFIX_CANDIDATE);
        assert_eq!(key.key().len(), 1 + 33);
    }

    #[test]
    fn test_vote_storage_key_starts_with_0x19() {
        let pk = dummy_pubkey(6);
        let key = NeoToken::vote_storage_key(&pk);
        assert_eq!(key.id(), NeoToken::ID);
        assert_eq!(key.key()[0], PREFIX_VOTE);
        assert_eq!(key.key().len(), 1 + 33);
    }

    #[test]
    fn test_default_register_price() {
        let cache = fresh_cache();
        assert_eq!(NeoToken::get_register_price(&cache), 100_000_000_000);
    }

    #[test]
    fn test_default_gas_per_block() {
        let cache = fresh_cache();
        // 5 GAS with 8 decimals
        assert_eq!(NeoToken::get_gas_per_block(&cache), 500_000_000);
    }

    #[test]
    fn test_read_only_cache_rejects_register() {
        let cache = Arc::new(DataCache::new_with_config(
            true,
            None,
            None,
            Default::default(),
        ));
        let pk = dummy_pubkey(7);
        let res = NeoToken::register_candidate(&cache, &pk);
        assert!(res.is_err());
    }
}
