//! Treasury native contract (id -11).
//!
//! Implements the NEP-17 / NEP-11 payment callbacks of the C#
//! `Neo.SmartContract.Native.Treasury`. In C# both `OnNEP17Payment` and
//! `OnNEP11Payment` have empty bodies — the Treasury simply accepts incoming
//! token transfers — so the implementations here are exact no-ops. `verify`
//! (committee witness check) is the next increment.

use std::any::Any;
use std::sync::LazyLock;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{ContractParameterType, UInt160};

use crate::hashes::TREASURY_HASH;

/// Lazily-initialised script-hash handle for the Treasury contract.
pub static TREASURY_HASH_REF: LazyLock<UInt160> = LazyLock::new(|| *TREASURY_HASH);

/// The Treasury native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Treasury;

impl Treasury {
    /// Stable native contract id (matches C# `Treasury`).
    pub const ID: i32 = -11;

    /// Construct a new `Treasury` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the Treasury script hash.
    pub fn script_hash() -> UInt160 {
        *TREASURY_HASH_REF
    }
}

static TREASURY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    use ContractParameterType::{Any as AnyType, ByteArray, Hash160, Integer, Void};
    // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags = CallFlags.None)]`;
    // payment callbacks are not `safe`.
    vec![
        NativeMethod::new(
            "onNEP17Payment".to_string(),
            1 << 5,
            false,
            0,
            vec![Hash160, Integer, AnyType],
            Void,
        ),
        NativeMethod::new(
            "onNEP11Payment".to_string(),
            1 << 5,
            false,
            0,
            vec![Hash160, Integer, ByteArray, AnyType],
            Void,
        ),
    ]
});

impl NativeContract for Treasury {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *TREASURY_HASH_REF
    }

    fn name(&self) -> &str {
        "Treasury"
    }

    fn methods(&self) -> &[NativeMethod] {
        &TREASURY_METHODS
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
            // Both callbacks are no-ops in C# (empty bodies); they return Void,
            // so an empty payload pushes nothing onto the stack.
            "onNEP17Payment" | "onNEP11Payment" => Ok(Vec::new()),
            other => Err(CoreError::invalid_operation(format!(
                "Treasury method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = Treasury::new();
        assert_eq!(NativeContract::id(&c), -11);
        assert_eq!(NativeContract::name(&c), "Treasury");
        assert_eq!(NativeContract::hash(&c), *TREASURY_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["onNEP17Payment", "onNEP11Payment"]);
        // Payment callbacks: not safe, no required call flags, Void return.
        assert!(c
            .methods()
            .iter()
            .all(|m| !m.safe && m.required_call_flags == 0 && m.return_type == ContractParameterType::Void));
    }
}
