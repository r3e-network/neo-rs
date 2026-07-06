//! # neo-native-contracts::notary
//!
//! Native Notary contract state and request verification behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `constants`: deposit defaults and storage-prefix bytes.
//! - `initialize`: HF_Echidna activation setting seeding.
//! - `invoke`: native method dispatch for deposit/withdraw/verify calls.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `persist`: Notary-assisted fee accounting and designated notary rewards.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.
//! - `verify_dispatch_tests`: notary dispatch verification coverage.

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};

use crate::hashes::NOTARY_HASH;

mod constants;
mod initialize;
mod invoke;
mod metadata;
mod persist;
mod storage;

pub(in crate::notary) use constants::*;

native_contract_handle!(
    /// The Notary native contract.
    pub struct Notary {
        id: -10,
        contract_name: "Notary",
        hash: NOTARY_HASH,
    }
);

impl NativeContract for Notary {
    native_contract_identity!(Notary);

    // C# `Notary.Activations => [Hardfork.HF_Echidna, Hardfork.HF_Faun]`
    // (Notary.cs): the contract itself does not exist before HF_Echidna —
    // `ActiveIn` is the first activation. Without this override the contract
    // would be genesis-active in neo-rs, diverging native deployment, manifest
    // state, and call resolution below the Echidna height.
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfEchidna)
    }

    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfEchidna, Hardfork::HfFaun]
    }

    /// C# `Notary.OnManifestCompose` (Notary.cs:92-102): NEP-30 joins NEP-27
    /// once HF_Faun is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            crate::native_supported_standards(&[crate::NEP27_STANDARD, crate::NEP30_STANDARD])
        } else {
            crate::native_supported_standards(&[crate::NEP27_STANDARD])
        }
    }

    fn methods(&self) -> &[NativeMethod] {
        &metadata::NOTARY_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.initialize_native(engine)
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.on_persist_native(engine)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }
}

#[cfg(test)]
#[path = "../tests/notary/mod.rs"]
mod tests;

/// End-to-end coverage of `verify` through the VM dispatch (the proven
/// witness-gated script-execution harness): the Notary native is seeded via a
/// ContractManagement record, a P2PNotary designation is written in the
/// RoleManagement storage layout, and `verify(signature)` is exercised through
/// `System.Contract.Call` against NotaryAssisted transaction containers.
#[cfg(test)]
#[path = "../tests/notary/verify_dispatch_tests.rs"]
mod verify_dispatch_tests;
