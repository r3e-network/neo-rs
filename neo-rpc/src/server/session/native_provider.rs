//! Native-contract read capabilities for RPC invocation sessions.
//!
//! Session construction needs a narrow Policy view for the C# invocation
//! compatibility fields it synthesizes before execution. Keeping those reads
//! behind a local provider seam prevents the RPC session workflow from
//! constructing native contracts directly or bypassing the composition root's
//! native registry.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

/// Native-contract capabilities required by RPC session construction.
pub(super) trait SessionNativeProvider {
    /// Returns the active `MaxValidUntilBlockIncrement` value.
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the active `MillisecondsPerBlock` value.
    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to the session's
/// narrow Policy read capability.
#[derive(Clone, Debug)]
pub(super) struct NativeSessionProvider<P>
where
    P: NativeContractProvider,
{
    adapter: NativeProviderAdapter<P>,
}

impl<P> NativeSessionProvider<P>
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

impl<P> SessionNativeProvider for NativeSessionProvider<P>
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

    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.adapter.milliseconds_per_block(snapshot, settings)
    }
}
