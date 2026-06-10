//! Treasury native contract (id -11).
//!
//! Implements the NEP-17 / NEP-11 payment callbacks of the C#
//! `Neo.SmartContract.Native.Treasury`. In C# both `OnNEP17Payment` and
//! `OnNEP11Payment` have empty bodies — the Treasury simply accepts incoming
//! token transfers — so the implementations here are exact no-ops. `verify`
//! (committee witness check) is the next increment.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::{Hardfork, ProtocolSettings};
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

    // C# `Treasury.Activations => [Hardfork.HF_Faun]` (Treasury.cs:29): the
    // contract does not exist before HF_Faun. Without this override Treasury
    // would be genesis-active in neo-rs, diverging native deployment and
    // manifest state below the Faun height (an unscheduled Faun means never
    // active, matching C# `IsActive`).
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    /// C# `Treasury.OnManifestCompose` (Treasury.cs:31-34): unconditional —
    /// the contract only exists from HF_Faun onwards.
    fn supported_standards(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<String> {
        vec![
            "NEP-26".to_string(),
            "NEP-27".to_string(),
            "NEP-30".to_string(),
        ]
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

    /// C# `Treasury.Activations => [HF_Faun]` (Treasury.cs:29) and
    /// `OnManifestCompose` (Treasury.cs:31-34): the contract activates at
    /// Faun and its manifest declares NEP-26/NEP-27/NEP-30 unconditionally.
    #[test]
    fn faun_activation_and_manifest_standards() {
        use neo_execution::native_contract::build_native_contract_state;

        let c = Treasury::new();
        assert_eq!(NativeContract::active_in(&c), Some(Hardfork::HfFaun));
        // Unscheduled Faun (the default mainnet/testnet config): never
        // active, matching C# `IsActive` with an unconfigured hardfork.
        assert!(!c.is_active(&ProtocolSettings::default(), u32::MAX));

        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 10);
        assert!(!c.is_active(&settings, 9));
        assert!(c.is_active(&settings, 10));

        let state = build_native_contract_state(&c, &settings, 10);
        assert_eq!(
            state.manifest.supported_standards,
            ["NEP-26", "NEP-27", "NEP-30"]
        );
    }
}
