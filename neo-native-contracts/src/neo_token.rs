//! NeoToken (NEO) native contract (id -5).
//!
//! Implements the NEP-17 metadata of the C# `Neo.SmartContract.Native.NeoToken`
//! (`symbol` "NEO", `decimals` 0). NEO's stateful surface (NEP-17 balances plus
//! governance: vote, candidates, committee, getGasPerBlock, unclaimedGas, ...)
//! is the next increment on the storage-backed pattern; the methods declared
//! below are byte-for-byte C# parity.

use std::any::Any;
use std::sync::LazyLock;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::StorageKey;
use num_bigint::BigInt;

use crate::hashes::NEO_TOKEN_HASH;
use crate::LedgerContract;

/// C# `NeoToken.Prefix_RegisterPrice`.
const PREFIX_REGISTER_PRICE: u8 = 13;
/// C# default candidate register price: 1000 GAS, in datoshi (1000 * 1e8).
const DEFAULT_REGISTER_PRICE: i64 = 1000 * 100_000_000;
/// C# `NeoToken.Prefix_GasPerBlock`.
const PREFIX_GAS_PER_BLOCK: u8 = 29;
/// C# default GAS-per-block at index 0: 5 GAS, in datoshi (5 * 1e8).
const DEFAULT_GAS_PER_BLOCK: i64 = 5 * 100_000_000;

/// Lazily-initialised script-hash handle for the NEO native contract.
pub static NEO_HASH: LazyLock<UInt160> = LazyLock::new(|| *NEO_TOKEN_HASH);

/// The NeoToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (matches C# `NeoToken`).
    pub const ID: i32 = -5;
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;

    /// Construct a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the NEO script hash.
    pub fn script_hash() -> UInt160 {
        *NEO_HASH
    }
}

/// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
fn register_price(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(
        snapshot,
        NeoToken::ID,
        PREFIX_REGISTER_PRICE,
        DEFAULT_REGISTER_PRICE,
    )
}

/// Returns the GAS-per-block effective at `index`: the most recent
/// `Prefix_GasPerBlock` record whose record index is ≤ `index` (C#
/// `GetSortedGasRecords(...).First().GasPerBlock`), defaulting to 5 GAS.
fn gas_per_block_at(snapshot: &DataCache, index: u32) -> BigInt {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_GAS_PER_BLOCK]);
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
        let key_bytes = key.key();
        if key_bytes.len() >= 5 {
            let record_index =
                u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
            if record_index <= index {
                return BigInt::from_signed_bytes_le(&item.value_bytes());
            }
        }
    }
    BigInt::from(DEFAULT_GAS_PER_BLOCK)
}

static NEO_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], int),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new("totalSupply".into(), 1 << 15, true, read_states, vec![], int),
        NativeMethod::new(
            "balanceOf".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        ),
        // Governance reads.
        NativeMethod::new("getGasPerBlock".into(), 1 << 15, true, read_states, vec![], int),
        NativeMethod::new("getRegisterPrice".into(), 1 << 15, true, read_states, vec![], int),
    ]
});

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    fn name(&self) -> &str {
        "NeoToken"
    }

    fn methods(&self) -> &[NativeMethod] {
        &NEO_METHODS
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
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
            "totalSupply" => {
                let snapshot = engine.snapshot_cache();
                let total =
                    crate::read_storage_int(&snapshot, Self::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY, 0)?;
                Ok(BigInt::from(total).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::balanceOf requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::balanceOf: bad account: {e}"))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "getGasPerBlock" => {
                let snapshot = engine.snapshot_cache();
                let index = LedgerContract::new().current_index(&snapshot)?.saturating_add(1);
                Ok(gas_per_block_at(&snapshot, index).to_signed_bytes_le())
            }
            "getRegisterPrice" => {
                let snapshot = engine.snapshot_cache();
                Ok(BigInt::from(register_price(&snapshot)?).to_signed_bytes_le())
            }
            other => Err(CoreError::invalid_operation(format!(
                "NeoToken method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = NeoToken::new();
        assert_eq!(NativeContract::id(&c), -5);
        assert_eq!(NativeContract::name(&c), "NeoToken");
        assert_eq!(NativeContract::hash(&c), *NEO_TOKEN_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["symbol", "decimals", "totalSupply", "balanceOf", "getGasPerBlock", "getRegisterPrice"]
        );
        let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
        assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
        let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
        assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[2u8; 20]).unwrap();
        assert_eq!(
            crate::read_nep17_balance(&cache, NeoToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }

    #[test]
    fn governance_reads_have_defaults_and_read_storage() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);

        // Defaults when unset: 1000 GAS register price, 5 GAS per block.
        assert_eq!(register_price(&cache).unwrap(), DEFAULT_REGISTER_PRICE);
        assert_eq!(gas_per_block_at(&cache, 100), BigInt::from(DEFAULT_GAS_PER_BLOCK));

        // register price reads the prefix-13 BigInteger.
        cache.add(
            StorageKey::new(NeoToken::ID, vec![PREFIX_REGISTER_PRICE]),
            StorageItem::from_bytes(BigInt::from(500 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(register_price(&cache).unwrap(), 500 * 100_000_000);

        // gas-per-block backward seek: record at index 10 applies from 10 on.
        let mut key = vec![PREFIX_GAS_PER_BLOCK];
        key.extend_from_slice(&10u32.to_be_bytes());
        cache.add(
            StorageKey::new(NeoToken::ID, key),
            StorageItem::from_bytes(BigInt::from(3 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(gas_per_block_at(&cache, 9), BigInt::from(DEFAULT_GAS_PER_BLOCK));
        assert_eq!(gas_per_block_at(&cache, 20), BigInt::from(3 * 100_000_000i64));
    }
}
