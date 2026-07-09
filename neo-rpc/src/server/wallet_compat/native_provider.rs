//! Native-contract read capabilities for wallet compatibility helpers.
//!
//! Wallet compatibility flows mirror C# wallet helper logic while staying
//! inside the RPC crate. This module keeps native Policy reads behind a local
//! provider seam instead of constructing native contracts directly inside fee
//! and transaction-building algorithms.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::DataCache;
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

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

/// Adapter from the node-composed native-contract provider to wallet
/// compatibility Policy read capabilities.
#[derive(Clone, Debug)]
pub(super) struct NativeWalletCompatProvider {
    adapter: NativeProviderAdapter,
}

impl NativeWalletCompatProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            adapter: NativeProviderAdapter::new(native_contract_provider),
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
        self.adapter.with_policy(|policy| {
            policy.get_exec_fee_factor_snapshot(snapshot, settings, block_index)
        })
    }

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.adapter
            .with_policy(|policy| policy.get_fee_per_byte_snapshot(snapshot))
    }
}
