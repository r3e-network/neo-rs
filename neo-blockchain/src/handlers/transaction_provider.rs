//! Native-contract read capabilities for transaction admission.
//!
//! Transaction admission needs a narrow Policy view for traceable conflict
//! checks. Keeping that read behind a local provider seam makes the handler
//! depend on capabilities instead of constructing native contracts directly in
//! the admission flow.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
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

/// Adapter from the node-composed native-contract provider to the transaction
/// admission Policy read capability.
#[derive(Clone)]
pub(super) struct NativeTransactionProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeTransactionProvider {
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

    fn policy_contract(&self) -> CoreResult<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("PolicyContract")
            .ok_or_else(|| CoreError::invalid_operation("native provider missing PolicyContract"))
    }
}

impl std::fmt::Debug for NativeTransactionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTransactionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl TransactionNativeProvider for NativeTransactionProvider {
    fn max_traceable_blocks(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy_contract()?
            .as_any()
            .downcast_ref::<PolicyContract>()
            .ok_or_else(|| {
                CoreError::invalid_operation("native provider returned non-PolicyContract")
            })?
            .get_max_traceable_blocks_snapshot(snapshot, settings)
    }
}
