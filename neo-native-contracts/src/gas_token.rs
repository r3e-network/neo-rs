//! GasToken (GAS) native contract (id -6).
//!
//! Implements the NEP-17 metadata of the C# `Neo.SmartContract.Native.GasToken`
//! (`symbol` "GAS", `decimals` 8). The stateful NEP-17 methods (`totalSupply`,
//! `balanceOf`, `transfer`) are the next increment on the storage-backed
//! pattern; the methods declared below are byte-for-byte C# parity.

use std::any::Any;
use std::sync::LazyLock;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use num_bigint::BigInt;

use crate::hashes::GAS_TOKEN_HASH;

/// Lazily-initialised script-hash handle for the GAS native contract.
pub static GAS_HASH: LazyLock<UInt160> = LazyLock::new(|| *GAS_TOKEN_HASH);

/// The GasToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct GasToken;

impl GasToken {
    /// Stable native contract id (matches C# `GasToken`).
    pub const ID: i32 = -6;
    /// NEP-17 symbol (C# `GasToken.Symbol => "GAS"`).
    pub const SYMBOL: &'static str = "GAS";
    /// NEP-17 decimals (C# `GasToken.Decimals => 8`).
    pub const DECIMALS: u8 = 8;

    /// Construct a new `GasToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the GAS script hash.
    pub fn script_hash() -> UInt160 {
        *GAS_HASH
    }
}

static GAS_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], ContractParameterType::Integer),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new(
            "totalSupply".into(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "balanceOf".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Integer,
        ),
    ]
});

impl NativeContract for GasToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    fn name(&self) -> &str {
        "GasToken"
    }

    fn methods(&self) -> &[NativeMethod] {
        &GAS_METHODS
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
                    CoreError::invalid_operation("GasToken::balanceOf requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("GasToken::balanceOf: bad account: {e}"))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            other => Err(CoreError::invalid_operation(format!(
                "GasToken method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = GasToken::new();
        assert_eq!(NativeContract::id(&c), -6);
        assert_eq!(NativeContract::name(&c), "GasToken");
        assert_eq!(NativeContract::hash(&c), *GAS_TOKEN_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["symbol", "decimals", "totalSupply", "balanceOf"]);
        // Metadata getters are zero-fee; the state reads are ReadStates getters.
        let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
        assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
        let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
        assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        use neo_storage::persistence::DataCache;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[1u8; 20]).unwrap();
        // C# BalanceOf returns BigInteger.Zero when the account has no entry.
        assert_eq!(
            crate::read_nep17_balance(&cache, GasToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }
}
