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
use neo_primitives::{ContractParameterType, UInt160};
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
    // `[ContractMethod]` with no CpuFee -> fee 0, RequiredCallFlags None.
    vec![
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], ContractParameterType::Integer),
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
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
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
        assert_eq!(names, ["symbol", "decimals"]);
        // NEP-17 metadata getters: zero fee, no required state access.
        assert!(c.methods().iter().all(|m| m.safe && m.cpu_fee == 0 && m.required_call_flags == 0));
    }
}
