//! Native-contract read capabilities for RPC invocation sessions.
//!
//! Session construction needs a narrow Policy view for the C# invocation
//! compatibility fields it synthesizes before execution. Keeping those reads
//! behind a local provider seam prevents the RPC session workflow from
//! constructing native contracts directly.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

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

/// Factory for RPC session native-contract providers.
pub(super) trait SessionNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: SessionNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSessionProvider {
    policy: PolicyContract,
}

impl NativeSessionProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl SessionNativeProvider for NativeSessionProvider {
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
    }

    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.policy
            .get_milliseconds_per_block_snapshot(snapshot, settings)
    }
}

/// Factory for production RPC session native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeSessionProviderFactory;

impl SessionNativeProviderFactory for NativeSessionProviderFactory {
    type Provider = NativeSessionProvider;

    fn provider(&self) -> Self::Provider {
        NativeSessionProvider::new()
    }
}
