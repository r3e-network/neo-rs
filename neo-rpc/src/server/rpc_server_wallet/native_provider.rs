//! Native-contract read capabilities for wallet RPC handlers.
//!
//! Wallet RPC handlers own transaction finalization and relay projection. This
//! module keeps native Policy reads behind a local provider seam so signing and
//! transfer flows do not construct native contracts directly or bypass the
//! composition root's native registry.

use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

/// Native-contract capabilities required by wallet RPC handlers.
pub(super) trait WalletNativeProvider {
    /// Returns the active `FeePerByte`.
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to wallet RPC
/// Policy read capabilities.
#[derive(Clone, Debug)]
pub(super) struct NativeWalletProvider<P>
where
    P: NativeContractProvider,
{
    adapter: NativeProviderAdapter<P>,
}

impl<P> NativeWalletProvider<P>
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

impl<P> WalletNativeProvider for NativeWalletProvider<P>
where
    P: NativeContractProvider,
{
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.adapter.fee_per_byte(snapshot)
    }
}
