//! Native-contract read capabilities for wallet compatibility helpers.
//!
//! Wallet compatibility flows mirror C# wallet helper logic while staying
//! inside the RPC crate. This module keeps native Policy reads behind a local
//! provider seam instead of constructing native contracts directly inside fee
//! and transaction-building algorithms.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;

/// Native-contract capabilities required by wallet compatibility helpers.
pub(super) trait WalletCompatNativeProvider {
    /// Returns the active `ExecFeeFactor` at `block_index`.
    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32>;

    /// Returns the active `FeePerByte`.
    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32>;
}

/// Factory for wallet-compat native-contract providers.
pub(super) trait WalletCompatNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: WalletCompatNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract handles.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletCompatProvider {
    policy: PolicyContract,
}

impl NativeWalletCompatProvider {
    /// Creates a provider backed by canonical native-contract handles.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self {
            policy: PolicyContract::new(),
        }
    }
}

impl WalletCompatNativeProvider for NativeWalletCompatProvider {
    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32> {
        self.policy
            .get_exec_fee_factor_snapshot(snapshot, settings, block_index)
    }

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.policy.get_fee_per_byte_snapshot(snapshot)
    }
}

/// Factory for production wallet compatibility native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeWalletCompatProviderFactory;

impl WalletCompatNativeProviderFactory for NativeWalletCompatProviderFactory {
    type Provider = NativeWalletCompatProvider;

    fn provider(&self) -> Self::Provider {
        NativeWalletCompatProvider::new()
    }
}
