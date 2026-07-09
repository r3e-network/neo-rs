//! Native-contract read capabilities for RPC invocation sessions.
//!
//! Session construction needs a narrow Policy view for the C# invocation
//! compatibility fields it synthesizes before execution. Keeping those reads
//! behind a local provider seam prevents the RPC session workflow from
//! constructing native contracts directly or bypassing the composition root's
//! native registry.

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;
use std::sync::Arc;

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
#[derive(Clone)]
pub(super) struct NativeSessionProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeSessionProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn with_contract<T, R>(&self, f: impl FnOnce(&T) -> CoreResult<R>) -> CoreResult<R>
    where
        T: 'static,
    {
        let contract = self
            .native_contract_provider
            .get_native_contract_by_name("PolicyContract")
            .ok_or_else(|| {
                CoreError::invalid_operation("native provider missing PolicyContract")
            })?;
        let policy = contract.as_any().downcast_ref::<T>().ok_or_else(|| {
            CoreError::invalid_operation("native provider returned non-PolicyContract")
        })?;
        f(policy)
    }
}

impl std::fmt::Debug for NativeSessionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeSessionProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl SessionNativeProvider for NativeSessionProvider {
    fn max_valid_until_block_increment(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>(|policy| {
            policy.get_max_valid_until_block_increment_snapshot(snapshot, settings)
        })
    }

    fn milliseconds_per_block(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>(|policy| {
            policy.get_milliseconds_per_block_snapshot(snapshot, settings)
        })
    }
}
