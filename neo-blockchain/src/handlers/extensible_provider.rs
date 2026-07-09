//! Native-contract read capabilities for extensible-payload verification.
//!
//! Extensible payload admission needs a narrow whitelist view over NEO and
//! RoleManagement state. Keeping those reads behind a local provider seam makes
//! the handler depend on capabilities instead of constructing native contracts
//! directly in the verification flow.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{NeoToken, Role, RoleManagement};
use neo_primitives::UInt160;
use neo_storage::DataCache;

/// Native-contract capabilities required to build the extensible witness whitelist.
pub(super) trait ExtensibleNativeProvider {
    /// Returns the cached committee multisig address.
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>>;

    /// Returns the validators for the next block, in C# whitelist order.
    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>>;

    /// Returns StateValidator designated nodes effective at `height`.
    fn state_validators(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>>;
}

/// Adapter from the node-composed native-contract provider to the extensible
/// verifier's narrow whitelist read capability.
#[derive(Clone)]
pub(super) struct NativeExtensibleProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeExtensibleProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> Arc<dyn NativeContractProvider> {
        Arc::clone(&self.native_contract_provider)
    }

    fn neo_token(&self) -> CoreResult<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("NeoToken")
            .ok_or_else(|| CoreError::invalid_operation("native provider missing NeoToken"))
    }

    fn role_management(&self) -> CoreResult<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("RoleManagement")
            .ok_or_else(|| CoreError::invalid_operation("native provider missing RoleManagement"))
    }
}

impl std::fmt::Debug for NativeExtensibleProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeExtensibleProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl ExtensibleNativeProvider for NativeExtensibleProvider {
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        self.neo_token()?
            .as_any()
            .downcast_ref::<NeoToken>()
            .ok_or_else(|| CoreError::invalid_operation("native provider returned non-NeoToken"))?
            .committee_address(snapshot)
    }

    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        self.neo_token()?
            .as_any()
            .downcast_ref::<NeoToken>()
            .ok_or_else(|| CoreError::invalid_operation("native provider returned non-NeoToken"))?
            .next_block_validators(
                snapshot,
                usize::try_from(settings.validators_count).unwrap_or(0),
            )
    }

    fn state_validators(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.role_management()?
            .as_any()
            .downcast_ref::<RoleManagement>()
            .ok_or_else(|| {
                CoreError::invalid_operation("native provider returned non-RoleManagement")
            })?
            .get_designated_by_role_at(snapshot, Role::StateValidator, height)
    }
}
