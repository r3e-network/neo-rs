//! Native-contract capabilities for empty-block fast-forward staging.
//!
//! The fast-forward stage belongs to `neo-blockchain`, but the state transition
//! math belongs to `neo-native-contracts`. This provider seam keeps staging
//! dependent on a narrow native capability instead of constructing native
//! contracts directly in the pipeline code.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::NeoToken;
use neo_storage::DataCache;

/// Native-contract capabilities required by empty-block fast-forward staging.
pub(super) trait EmptyBlockFastForwardNativeProvider {
    /// Applies byte-equivalent native storage effects for a contiguous empty run.
    fn fast_forward_empty_block_rewards(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()>;
}

/// Adapter from the batch native-persistence provider to the fast-forward
/// reward capability.
#[derive(Clone)]
pub(super) struct NativeEmptyBlockFastForwardProvider<P: ?Sized> {
    native_contract_provider: Arc<P>,
}

impl<P> NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider + ?Sized,
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

    fn neo_token(&self) -> CoreResult<Arc<dyn NativeContract>> {
        self.provider()
            .get_native_contract_by_name("NeoToken")
            .ok_or_else(|| CoreError::invalid_operation("native provider missing NeoToken"))
    }
}

impl<P> std::fmt::Debug for NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider + ?Sized,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeEmptyBlockFastForwardProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<P> EmptyBlockFastForwardNativeProvider for NativeEmptyBlockFastForwardProvider<P>
where
    P: NativeContractProvider + ?Sized,
{
    fn fast_forward_empty_block_rewards(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        self.neo_token()?
            .as_any()
            .downcast_ref::<NeoToken>()
            .ok_or_else(|| CoreError::invalid_operation("native provider returned non-NeoToken"))?
            .fast_forward_empty_block_rewards(snapshot, settings, start, end)
    }
}
