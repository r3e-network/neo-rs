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
use neo_storage::DataCache;
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

/// Native-contract capabilities required by RPC session construction.
pub(super) trait SessionNativeProvider {
    /// Returns the active `MaxValidUntilBlockIncrement` value.
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the active `MillisecondsPerBlock` value.
    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to the session's
/// narrow Policy read capability.
#[derive(Clone, Debug)]
pub(super) struct NativeSessionProvider {
    adapter: NativeProviderAdapter,
}

impl NativeSessionProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            adapter: NativeProviderAdapter::new(native_contract_provider),
        }
    }
}

impl SessionNativeProvider for NativeSessionProvider {
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.adapter.with_policy(|policy| {
            policy.get_max_valid_until_block_increment_snapshot(snapshot, settings)
        })
    }

    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.adapter
            .with_policy(|policy| policy.get_milliseconds_per_block_snapshot(snapshot, settings))
    }
}
