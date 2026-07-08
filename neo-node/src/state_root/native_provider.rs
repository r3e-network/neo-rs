//! Native-contract read capabilities for StateService voting.
//!
//! The StateRoot driver needs a narrow RoleManagement view to decide whether
//! this node is a designated StateValidator for a persisted block. Keeping that
//! read behind a local provider seam makes the driver depend on capabilities
//! instead of constructing native contracts directly in the voting loop.

use neo_crypto::ECPoint;
use neo_native_contracts::{Role, RoleManagement};
use neo_storage::DataCache;

/// Native-contract capabilities required by StateService voting.
pub(super) trait StateRootNativeProvider {
    /// Returns StateValidator designated nodes effective at `index`.
    fn state_validators(&self, snapshot: &DataCache, index: u32) -> Vec<ECPoint>;
}

/// Factory for StateService native-contract providers.
pub(super) trait StateRootNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: StateRootNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeStateRootProvider {
    roles: RoleManagement,
}

impl NativeStateRootProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            roles: RoleManagement::new(),
        }
    }
}

impl StateRootNativeProvider for NativeStateRootProvider {
    fn state_validators(&self, snapshot: &DataCache, index: u32) -> Vec<ECPoint> {
        self.roles
            .get_designated_by_role_at(snapshot, Role::StateValidator, index)
            .unwrap_or_default()
    }
}

/// Factory for production StateService native-contract read providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeStateRootProviderFactory;

impl StateRootNativeProviderFactory for NativeStateRootProviderFactory {
    type Provider = NativeStateRootProvider;

    fn provider(&self) -> Self::Provider {
        NativeStateRootProvider::new()
    }
}
