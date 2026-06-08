//! PolicyContract native contract stub.

use crate::hashes::POLICY_CONTRACT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_storage::persistence::DataCache;
use neo_storage::StorageKey;
use num_bigint::BigInt;
use std::any::Any;
use std::sync::LazyLock;

/// C# `PolicyContract.Prefix_FeePerByte` storage prefix.
const PREFIX_FEE_PER_BYTE: u8 = 10;
/// C# `PolicyContract.Prefix_StoragePrice` storage prefix.
const PREFIX_STORAGE_PRICE: u8 = 19;
/// C# `PolicyContract.DefaultStoragePrice`.
const DEFAULT_STORAGE_PRICE: i64 = 100_000;
/// C# `PolicyContract.Prefix_BlockedAccount` storage prefix.
const PREFIX_BLOCKED_ACCOUNT: u8 = 15;

/// Lazily-initialised script-hash handle for the PolicyContract.
pub static POLICY_HASH: LazyLock<UInt160> = LazyLock::new(|| *POLICY_CONTRACT_HASH);

/// Default execution fee factor (matches C# `PolicyContract.DefaultExecFeeFactor`).
pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
/// Default fee per byte (matches C# `PolicyContract.DefaultFeePerByte`).
pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
/// Default max valid-until-block increment
/// (matches C# `PolicyContract.DefaultMaxValidUntilBlockIncrement`).
pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

/// Static accessor for the PolicyContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyContract;

impl PolicyContract {
    /// Stable native contract id (-7 in C# Policy contract).
    pub const ID: i32 = -7;

    /// Default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;
    /// Default fee per byte.
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1000;
    /// Default max valid-until-block increment.
    pub const DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 5_760;

    /// Construct a new `PolicyContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the Policy native contract.
    pub fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    /// Returns the script hash of the Policy native contract (static).
    pub fn script_hash() -> UInt160 {
        *POLICY_HASH
    }

    /// Stub: returns the max valid-until-block increment from the
    /// snapshot, or `Ok(default)` if not configured.
    pub fn get_max_valid_until_block_increment_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_MAX_VALID_UNTIL_BLOCK_INCREMENT)
    }

    /// Stub: returns the execution fee factor from the snapshot, or
    /// `Ok(DEFAULT_EXEC_FEE_FACTOR)` if not configured.
    pub fn get_exec_fee_factor_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
        _settings: &neo_config::ProtocolSettings,
        _height: u32,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_EXEC_FEE_FACTOR)
    }

    /// Stub: returns the fee-per-byte from the snapshot, or
    /// `Ok(DEFAULT_FEE_PER_BYTE)` if not configured.
    pub fn get_fee_per_byte_snapshot(
        &self,
        _snapshot: &neo_storage::persistence::DataCache,
    ) -> neo_error::CoreResult<u32> {
        Ok(DEFAULT_FEE_PER_BYTE)
    }
}

/// C# `GetFeePerByte` = `(long)(BigInteger)snapshot[_feePerByte]`.
fn fee_per_byte(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(
        snapshot,
        PolicyContract::ID,
        PREFIX_FEE_PER_BYTE,
        i64::from(DEFAULT_FEE_PER_BYTE),
    )
}

/// C# `GetStoragePrice` = `(uint)(BigInteger)snapshot[_storagePrice]`.
fn storage_price(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(snapshot, PolicyContract::ID, PREFIX_STORAGE_PRICE, DEFAULT_STORAGE_PRICE)
}

static POLICY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getFeePerByte".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getStoragePrice".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "isBlocked".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        ),
    ]
});

impl NativeContract for PolicyContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *POLICY_HASH
    }

    fn name(&self) -> &str {
        "PolicyContract"
    }

    fn methods(&self) -> &[NativeMethod] {
        &POLICY_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getFeePerByte" => Ok(BigInt::from(fee_per_byte(&snapshot)?).to_signed_bytes_le()),
            "getStoragePrice" => Ok(BigInt::from(storage_price(&snapshot)?).to_signed_bytes_le()),
            "isBlocked" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("PolicyContract::isBlocked requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("PolicyContract::isBlocked: bad account: {e}"))
                })?;
                let mut key_bytes = vec![PREFIX_BLOCKED_ACCOUNT];
                key_bytes.extend_from_slice(&account.to_bytes());
                // C# IsBlocked = snapshot.Contains(key(Prefix_BlockedAccount, account)).
                let blocked = snapshot.get(&StorageKey::new(Self::ID, key_bytes)).is_some();
                Ok(vec![u8::from(blocked)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "PolicyContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::StorageItem;

    #[test]
    fn native_contract_surface() {
        let c = PolicyContract::new();
        assert_eq!(NativeContract::id(&c), -7);
        assert_eq!(NativeContract::name(&c), "PolicyContract");
        assert_eq!(NativeContract::hash(&c), *POLICY_CONTRACT_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["getFeePerByte", "getStoragePrice", "isBlocked"]);
    }

    #[test]
    fn is_blocked_checks_storage_existence() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();
        let key = {
            let mut k = vec![PREFIX_BLOCKED_ACCOUNT];
            k.extend_from_slice(&account.to_bytes());
            StorageKey::new(PolicyContract::ID, k)
        };
        // Not blocked until a record exists.
        assert!(cache.get(&key).is_none());
        cache.add(key.clone(), StorageItem::from_bytes(vec![]));
        assert!(cache.get(&key).is_some());
    }

    #[test]
    fn fee_per_byte_reads_storage_with_default() {
        let cache = DataCache::new(false);
        // Absent -> default 1000 (C# writes this at initialization).
        assert_eq!(fee_per_byte(&cache).unwrap(), 1000);

        // A configured value is read back from the BigInteger storage item.
        let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_FEE_PER_BYTE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(4242).to_signed_bytes_le()));
        assert_eq!(fee_per_byte(&cache).unwrap(), 4242);
    }

    #[test]
    fn storage_price_reads_storage_with_default() {
        let cache = DataCache::new(false);
        assert_eq!(storage_price(&cache).unwrap(), DEFAULT_STORAGE_PRICE);

        let key = StorageKey::new(PolicyContract::ID, vec![PREFIX_STORAGE_PRICE]);
        cache.add(key, StorageItem::from_bytes(BigInt::from(250_000).to_signed_bytes_le()));
        assert_eq!(storage_price(&cache).unwrap(), 250_000);
    }
}
