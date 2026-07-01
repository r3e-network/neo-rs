//! # neo-native-contracts::treasury
//!
//! Native treasury accounting and fund recovery behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `tests`: Module-local tests and regression coverage.
//! - `verify_witness_tests`: witness verification coverage.

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};

use crate::hashes::TREASURY_HASH;

mod metadata;

native_contract_handle!(
    /// The Treasury native contract.
    pub struct Treasury {
        id: -11,
        contract_name: "Treasury",
        hash: TREASURY_HASH,
    }
);

impl NativeContract for Treasury {
    native_contract_identity!(Treasury);

    // C# `Treasury.Activations => [Hardfork.HF_Faun]` (Treasury.cs:29): the
    // contract does not exist before HF_Faun. Without this override Treasury
    // would be genesis-active in neo-rs, diverging native deployment and
    // manifest state below the configured Faun height. If a custom/private
    // config omits Faun, C# `IsActive` treats ActiveIn as genesis-active, while
    // `IsInitializeBlock` skips the missing hardfork initialization block.
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfFaun)
    }

    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfFaun]
    }

    /// C# `Treasury.OnManifestCompose` (Treasury.cs:31-34): unconditional —
    /// the contract only exists from HF_Faun onwards.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        crate::native_supported_standards(&[
            crate::NEP26_STANDARD,
            crate::NEP27_STANDARD,
            crate::NEP30_STANDARD,
        ])
    }

    fn methods(&self) -> &[NativeMethod] {
        &metadata::TREASURY_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            // Both callbacks are no-ops in C# (empty bodies); they return Void,
            // so an empty payload pushes nothing onto the stack.
            crate::NEP17_PAYMENT_METHOD | crate::NEP11_PAYMENT_METHOD => Ok(Vec::new()),
            // C# `Treasury.Verify` (Treasury.cs:41-42) = `CheckCommittee(engine)`:
            // true iff the committee multi-sig address witnesses the current
            // container — the witness seam for Treasury-signed transactions.
            "verify" => {
                let authorized =
                    crate::committee::is_committee_witness(engine, "Treasury::verify")?;
                Ok(vec![u8::from(authorized)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "Treasury method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "../tests/treasury/mod.rs"]
mod tests;

/// End-to-end verification of `verify` through the VM: a script
/// `System.Contract.Call`s Treasury and the boolean result reflects whether
/// the committee multisig address witnesses the transaction (C#
/// `Treasury.Verify` = `CheckCommittee(engine)`).
#[cfg(test)]
#[path = "../tests/treasury/verify_witness_tests.rs"]
mod verify_witness_tests;
