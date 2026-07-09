//! Signed [`neo_state_service::StateRoot`] witness verification against the
//! StateValidators multisig.
//!
//! Mirrors C# `StateService.Network.StateRoot`:
//! - `Verify(settings, snapshot)` = `VerifyWitnesses(settings, snapshot, 2_00000000)`.
//! - `GetScriptHashesForVerifying(snapshot)` = the BFT address of the StateValidators
//!   designated (via `RoleManagement`) at the root's `Index`.
//!
//! This lives in `neo-blockchain` (which already depends on `neo-native-contracts`,
//! `neo-execution`, and `neo-vm`) rather than in the light `neo-state-service`
//! crate, and wraps the [`neo_state_service::StateRoot`] in a `VerifiableExt`
//! newtype so the tested, engine-based witness verification
//! ([`neo_execution::Helper::verify_witnesses_with_native_provider`]) is reused
//! instead of hand-rolling signature checks. The provider-aware entry point is
//! the architecture boundary; callers must pass the native-contract provider
//! composed by their node/service context.

use neo_config::ProtocolSettings;
use neo_crypto::Crypto;
use neo_execution::Helper;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{Role, RoleManagement};
use neo_payloads::{VerifiableExt, Witness};
use neo_primitives::error::PrimitiveResult;
use neo_primitives::{UInt160, UInt256, Verifiable};
use neo_state_service::StateRoot;
use neo_storage::DataCache;
use neo_vm::script_builder::RedeemScript;
use std::sync::Arc;

/// Max GAS for state-root witness verification (C# `StateRoot.Verify`: 2 GAS).
const STATE_ROOT_VERIFY_GAS: i64 = 2_0000_0000;

/// Native-contract capabilities required to resolve state-root verifiers.
trait StateRootNativeProvider: Send + Sync {
    /// Returns StateValidator designated nodes effective at `index`.
    fn state_validators(&self, snapshot: &DataCache, index: u32) -> Vec<neo_crypto::ECPoint>;
}

/// Adapter from the node-composed native-contract provider to the state-root
/// verifier's narrow RoleManagement read capability.
#[derive(Clone)]
struct StateRootNativeProviderAdapter<P> {
    native_contract_provider: Arc<P>,
}

impl<P> StateRootNativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        &self.native_contract_provider
    }
}

impl<P> std::fmt::Debug for StateRootNativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateRootNativeProviderAdapter")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> StateRootNativeProvider for StateRootNativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    fn state_validators(&self, snapshot: &DataCache, index: u32) -> Vec<neo_crypto::ECPoint> {
        self.provider()
            .get_native_contract_by_name("RoleManagement")
            .and_then(|contract| {
                contract
                    .as_any()
                    .downcast_ref::<RoleManagement>()
                    .and_then(|roles| {
                        roles
                            .get_designated_by_role_at(snapshot, Role::StateValidator, index)
                            .ok()
                    })
            })
            .unwrap_or_default()
    }
}

/// Resolves the StateValidators designated at `index` through an explicit
/// native-contract provider.
///
/// StateService voting and signed-root witness verification both need the same
/// C# `RoleManagement.GetDesignatedByRole(StateValidator, index)` view. Keeping
/// this lookup here lets node drivers use the same provider seam without
/// constructing native contract handles locally.
pub fn state_root_verifiers_with_native_provider<P>(
    snapshot: &DataCache,
    index: u32,
    native_contract_provider: Arc<P>,
) -> Vec<neo_crypto::ECPoint>
where
    P: NativeContractProvider,
{
    StateRootNativeProviderAdapter::new(native_contract_provider).state_validators(snapshot, index)
}

/// Owned wrapper making a [`StateRoot`] verifiable through the engine machinery.
/// It is owned rather than borrowing the root because `neo_primitives::Verifiable`
/// requires `Any + 'static`; `StateRoot` is small and clones cheaply.
struct VerifiableStateRoot<P> {
    root: StateRoot,
    native: P,
}

impl<P> VerifiableStateRoot<P> {
    fn new(root: StateRoot, native: P) -> Self {
        Self { root, native }
    }
}

impl<P> Verifiable for VerifiableStateRoot<P>
where
    P: StateRootNativeProvider + 'static,
{
    fn verify(&self) -> bool {
        // State-independent validity: nothing beyond the witness check, which is
        // state-dependent and handled by verify_witnesses.
        true
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        Ok(UInt256::from(Crypto::sha256(&self.root.unsigned_bytes())))
    }

    fn hash_data(&self) -> Vec<u8> {
        self.root.unsigned_bytes()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl<P> VerifiableExt for VerifiableStateRoot<P>
where
    P: StateRootNativeProvider + 'static,
{
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        // C# GetScriptHashesForVerifying: the BFT address of the StateValidators
        // designated at this root's index. No designation -> no verifiable hash.
        let validators = self.native.state_validators(snapshot, self.root.index());
        RedeemScript::bft_address(&validators)
            .map(|hash| vec![hash])
            .unwrap_or_default()
    }

    fn witnesses(&self) -> Vec<&Witness> {
        self.root.witness().map(|w| vec![w]).unwrap_or_default()
    }
}

/// Verifies a signed [`StateRoot`] using an explicit native-contract provider.
///
/// Callers that already own composition or persistence resources should prefer
/// this entry point so state-root witness verification stays bound to the same
/// native-contract set as the surrounding node service.
pub fn verify_state_root_with_native_provider<P>(
    state_root: &StateRoot,
    settings: &ProtocolSettings,
    snapshot: &DataCache,
    native_contract_provider: Arc<P>,
) -> bool
where
    P: NativeContractProvider + 'static,
{
    if state_root.witness().is_none() {
        return false;
    }
    let native = StateRootNativeProviderAdapter::new(native_contract_provider.clone());
    let verifiable = VerifiableStateRoot::new(state_root.clone(), native);
    let provider: Arc<dyn NativeContractProvider> = native_contract_provider.clone();
    Helper::verify_witnesses_with_native_provider(
        &verifiable,
        settings,
        snapshot,
        STATE_ROOT_VERIFY_GAS,
        Some(provider),
    )
}

#[cfg(test)]
#[path = "tests/state_root_verify.rs"]
mod tests;
