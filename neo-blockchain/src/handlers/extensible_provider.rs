//! Native-contract read capabilities for extensible-payload verification.
//!
//! Extensible payload admission needs a narrow whitelist view over NEO and
//! RoleManagement state. Keeping those reads behind a local provider seam makes
//! the handler depend on capabilities instead of constructing native contracts
//! directly in the verification flow.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
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

/// Factory for extensible native-contract providers.
pub(super) trait ExtensibleNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: ExtensibleNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeExtensibleProvider {
    neo: NeoToken,
    roles: RoleManagement,
}

impl NativeExtensibleProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            neo: NeoToken::new(),
            roles: RoleManagement::new(),
        }
    }
}

impl ExtensibleNativeProvider for NativeExtensibleProvider {
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        neo_execution::NativeContract::committee_address(&self.neo, snapshot)
    }

    fn next_block_validators(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        self.neo.next_block_validators(
            snapshot,
            usize::try_from(settings.validators_count).unwrap_or(0),
        )
    }

    fn state_validators(&self, snapshot: &DataCache, height: u32) -> CoreResult<Vec<ECPoint>> {
        self.roles
            .get_designated_by_role_at(snapshot, Role::StateValidator, height)
    }
}

/// Factory for production extensible native-contract read providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeExtensibleProviderFactory;

impl ExtensibleNativeProviderFactory for NativeExtensibleProviderFactory {
    type Provider = NativeExtensibleProvider;

    fn provider(&self) -> Self::Provider {
        NativeExtensibleProvider::new()
    }
}
