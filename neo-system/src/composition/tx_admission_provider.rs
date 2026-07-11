//! Transaction-admission read capabilities for composition-root helpers.
//!
//! The composition root wires runtime services and should depend on narrow
//! native capabilities instead of constructing native contracts inside helper
//! flows. Ledger reads use the node-wide routed provider factory directly;
//! this module owns the remaining native-contract adapter.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};

/// Native-contract capabilities required by transaction admission routing.
pub(super) trait TxAdmissionNativeProvider {
    /// Returns the active `MaxTraceableBlocks` value.
    fn max_traceable_blocks<B>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>
    where
        B: CacheRead;
}

/// Adapter from the node-composed native-contract provider to the transaction
/// admission Policy read capability.
#[derive(Clone)]
pub(super) struct NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    native_contract_provider: Arc<P>,
}

impl<P> NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the node's composition-root native provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn provider(&self) -> &P {
        self.native_contract_provider.as_ref()
    }
}

impl<P> std::fmt::Debug for NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeTxAdmissionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> TxAdmissionNativeProvider for NativeTxAdmissionProvider<P>
where
    P: NativeContractProvider,
{
    fn max_traceable_blocks<B>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>
    where
        B: CacheRead,
    {
        self.provider().max_traceable_blocks(snapshot, settings)
    }
}
