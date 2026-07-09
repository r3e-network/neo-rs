//! Native-contract read capabilities for wallet RPC handlers.
//!
//! Wallet RPC handlers own transaction finalization and relay projection. This
//! module keeps native Policy reads behind a local provider seam so signing and
//! transfer flows do not construct native contracts directly or bypass the
//! composition root's native registry.

use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;
use std::sync::Arc;

/// Native-contract capabilities required by wallet RPC handlers.
pub(super) trait WalletNativeProvider {
    /// Returns the active `FeePerByte`.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to wallet RPC
/// Policy read capabilities.
#[derive(Clone)]
pub(super) struct NativeWalletProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeWalletProvider {
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

impl std::fmt::Debug for NativeWalletProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeWalletProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl WalletNativeProvider for NativeWalletProvider {
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>(|policy| policy.get_fee_per_byte_snapshot(snapshot))
    }
}
