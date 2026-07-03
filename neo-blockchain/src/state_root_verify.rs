//! Signed [`StateRoot`] witness verification against the StateValidators multisig.
//!
//! Mirrors C# `StateService.Network.StateRoot`:
//! - `Verify(settings, snapshot)` = `VerifyWitnesses(settings, snapshot, 2_00000000)`.
//! - `GetScriptHashesForVerifying(snapshot)` = the BFT address of the StateValidators
//!   designated (via `RoleManagement`) at the root's `Index`.
//!
//! This lives in `neo-blockchain` (which already depends on `neo-native-contracts`,
//! `neo-execution`, and `neo-vm`) rather than in the light `neo-state-service`
//! crate, and wraps the [`StateRoot`] in a `VerifiableExt` newtype so the tested,
//! engine-based witness verification ([`neo_execution::Helper::verify_witnesses`])
//! is reused instead of hand-rolling signature checks.

use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_execution::Helper;
use neo_native_contracts::{Role, RoleManagement};
use neo_payloads::{VerifiableExt, Witness};
use neo_primitives::error::PrimitiveResult;
use neo_primitives::{UInt160, UInt256, Verifiable};
use neo_state_service::StateRoot;
use neo_storage::DataCache;
use neo_vm::script_builder::RedeemScript;

/// Max GAS for state-root witness verification (C# `StateRoot.Verify`: 2 GAS).
const STATE_ROOT_VERIFY_GAS: i64 = 2_0000_0000;

/// Owned wrapper making a [`StateRoot`] verifiable through the engine machinery.
/// It is owned rather than borrowing the root because `neo_primitives::Verifiable`
/// requires `Any + 'static`; `StateRoot` is small and clones cheaply.
struct VerifiableStateRoot(StateRoot);

impl Verifiable for VerifiableStateRoot {
    fn verify(&self) -> bool {
        // State-independent validity: nothing beyond the witness check, which is
        // state-dependent and handled by verify_witnesses.
        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        Ok(UInt256::from(Crypto::sha256(&self.0.unsigned_bytes())))
    }

    fn hash_data(&self) -> Vec<u8> {
        self.0.unsigned_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl VerifiableExt for VerifiableStateRoot {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        // C# GetScriptHashesForVerifying: the BFT address of the StateValidators
        // designated at this root's index. No designation -> no verifiable hash.
        let validators = RoleManagement::new()
            .get_designated_by_role_at(snapshot, Role::StateValidator, self.0.index())
            .unwrap_or_default();
        RedeemScript::bft_address(&validators)
            .map(|hash| vec![hash])
            .unwrap_or_default()
    }

    fn witnesses(&self) -> Vec<&Witness> {
        self.0.witness().map(|w| vec![w]).unwrap_or_default()
    }
}

/// Verifies a signed [`StateRoot`]'s witness against the StateValidators
/// designated at its index. Returns `false` if the root is unsigned, if no
/// StateValidators are designated at that height, or if the multisig witness
/// does not verify. Matches C# `StateRoot.Verify(settings, snapshot)`.
pub fn verify_state_root(
    state_root: &StateRoot,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
) -> bool {
    if state_root.witness().is_none() {
        return false;
    }
    Helper::verify_witnesses(
        &VerifiableStateRoot(state_root.clone()),
        settings,
        snapshot,
        STATE_ROOT_VERIFY_GAS,
    )
}

#[cfg(test)]
#[path = "tests/state_root_verify.rs"]
mod tests;
