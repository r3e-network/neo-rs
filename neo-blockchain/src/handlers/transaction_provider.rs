//! Native-contract read capabilities for transaction admission.
//!
//! Transaction admission needs a narrow Policy view for traceable conflict
//! checks. Keeping that read behind a local provider seam makes the handler
//! depend on capabilities instead of constructing native contracts directly in
//! the admission flow.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

/// Native-contract capabilities required by transaction admission.
pub(super) trait TransactionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Factory for transaction native-contract providers.
pub(super) trait TransactionNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: TransactionNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTransactionProvider {
    policy: PolicyContract,
}

impl NativeTransactionProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl TransactionNativeProvider for NativeTransactionProvider {
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_traceable_blocks_snapshot(snapshot, settings)
    }
}

/// Factory for production transaction native-contract read providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeTransactionProviderFactory;

impl TransactionNativeProviderFactory for NativeTransactionProviderFactory {
    type Provider = NativeTransactionProvider;

    fn provider(&self) -> Self::Provider {
        NativeTransactionProvider::new()
    }
}
