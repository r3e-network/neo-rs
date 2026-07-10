//! Native-contract capabilities for empty-block fast-forward staging.
//!
//! The fast-forward stage belongs to `neo-blockchain`, but the state transition
//! math belongs to `neo-native-contracts`. This provider seam keeps staging
//! dependent on a narrow native capability instead of constructing native
//! contracts directly in the pipeline code.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};

/// Native-contract capabilities required by empty-block fast-forward staging.
pub(super) trait EmptyBlockFastForwardNativeProvider {
    /// Applies byte-equivalent native storage effects for a contiguous empty run.
    fn fast_forward_empty_block_rewards<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()>;
}

/// Adapter from the batch native-persistence provider to the fast-forward
/// reward capability.
#[derive(Clone)]
pub(super) struct NativeEmptyBlockFastForwardProvider<P> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the native provider captured for the persist batch.
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

impl<P> std::fmt::Debug for NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeEmptyBlockFastForwardProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> EmptyBlockFastForwardNativeProvider for NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider,
{
    fn fast_forward_empty_block_rewards<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        self.provider()
            .fast_forward_empty_block_rewards(snapshot, settings, start, end)
    }
}
