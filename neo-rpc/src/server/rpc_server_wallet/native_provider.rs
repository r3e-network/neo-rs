//! Native-contract read capabilities for wallet RPC handlers.
//!
//! Wallet RPC handlers own transaction finalization and relay projection. This
//! module keeps native Policy reads behind a local provider seam so signing and
//! transfer flows do not construct native contracts directly.

use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

/// Native-contract capabilities required by wallet RPC handlers.
pub(super) trait WalletNativeProvider {
    /// Returns the active `FeePerByte`.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32>;
}

/// Factory for wallet RPC native-contract providers.
pub(super) trait WalletNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: WalletNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletProvider {
    policy: PolicyContract,
}

impl NativeWalletProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl WalletNativeProvider for NativeWalletProvider {
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.policy.get_fee_per_byte_snapshot(snapshot)
    }
}

/// Factory for production wallet RPC native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletProviderFactory;

impl WalletNativeProviderFactory for NativeWalletProviderFactory {
    type Provider = NativeWalletProvider;

    fn provider(&self) -> Self::Provider {
        NativeWalletProvider::new()
    }
}
