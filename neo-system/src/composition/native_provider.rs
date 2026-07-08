//! Native-contract read capabilities for composition-root helpers.
//!
//! The composition root wires runtime services and should depend on narrow
//! native capabilities instead of constructing native contracts inside helper
//! flows. This module owns those local provider seams.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

/// Native-contract capabilities required by transaction admission routing.
pub(super) trait TxAdmissionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Factory for transaction-admission native providers.
pub(super) trait TxAdmissionNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: TxAdmissionNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionProvider {
    policy: PolicyContract,
}

impl NativeTxAdmissionProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl TxAdmissionNativeProvider for NativeTxAdmissionProvider {
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_traceable_blocks_snapshot(snapshot, settings)
    }
}

/// Factory for production transaction-admission native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTxAdmissionProviderFactory;

impl TxAdmissionNativeProviderFactory for NativeTxAdmissionProviderFactory {
    type Provider = NativeTxAdmissionProvider;

    fn provider(&self) -> Self::Provider {
        NativeTxAdmissionProvider::new()
    }
}
