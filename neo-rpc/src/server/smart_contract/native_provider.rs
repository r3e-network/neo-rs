//! Native-contract read capabilities for smart-contract RPC handlers.
//!
//! Smart-contract invocation owns RPC response assembly and wallet preview
//! materialization. Keeping native Policy reads behind this local seam prevents
//! those flows from constructing native contracts directly or bypassing the
//! composition root's native registry.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

/// Native-contract capabilities required by smart-contract RPC helpers.
pub(super) trait SmartContractNativeProvider {
    /// Returns the active `MaxValidUntilBlockIncrement` value.
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to the
/// smart-contract RPC Policy read capability.
#[derive(Clone, Debug)]
pub(super) struct NativeSmartContractProvider<P>
where
    P: NativeContractProvider,
{
    adapter: NativeProviderAdapter<P>,
}

impl<P> NativeSmartContractProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            adapter: NativeProviderAdapter::new(native_contract_provider),
        }
    }
}

impl<P> SmartContractNativeProvider for NativeSmartContractProvider<P>
where
    P: NativeContractProvider,
{
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.adapter
            .max_valid_until_block_increment(snapshot, settings)
    }
}
