//! Native-contract read capabilities for smart-contract RPC handlers.
//!
//! Smart-contract invocation owns RPC response assembly and wallet preview
//! materialization. Keeping native Policy reads behind this local seam prevents
//! those flows from constructing native contracts directly.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

/// Native-contract capabilities required by smart-contract RPC helpers.
pub(super) trait SmartContractNativeProvider {
    /// Returns the active `MaxValidUntilBlockIncrement` value.
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Factory for smart-contract native-contract providers.
pub(super) trait SmartContractNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: SmartContractNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSmartContractProvider {
    policy: PolicyContract,
}

impl NativeSmartContractProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl SmartContractNativeProvider for NativeSmartContractProvider {
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
    }
}

/// Factory for production smart-contract native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSmartContractProviderFactory;

impl SmartContractNativeProviderFactory for NativeSmartContractProviderFactory {
    type Provider = NativeSmartContractProvider;

    fn provider(&self) -> Self::Provider {
        NativeSmartContractProvider::new()
    }
}
