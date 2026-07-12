//! Native-contract read capabilities for transaction admission.
//!
//! Transaction admission needs a narrow Policy view for traceable conflict
//! checks. Keeping that read behind a local provider seam makes the handler
//! depend on capabilities instead of constructing native contracts directly in
//! the admission flow.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};

/// Native-contract capabilities required by transaction admission.
pub(in crate::pipeline::handlers) trait TransactionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to the transaction
/// admission Policy read capability.
#[derive(Clone)]
pub(in crate::pipeline::handlers) struct NativeTransactionProvider<P> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeTransactionProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(in crate::pipeline::handlers) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }
}

impl<P> std::fmt::Debug for NativeTransactionProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTransactionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> TransactionNativeProvider for NativeTransactionProvider<P>
where
    P: NativeContractProvider,
{
    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.provider().max_traceable_blocks(snapshot, settings)
    }
}
