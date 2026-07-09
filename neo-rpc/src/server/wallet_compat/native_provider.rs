//! Native-contract read capabilities for wallet compatibility helpers.
//!
//! Wallet compatibility flows mirror C# wallet helper logic while staying
//! inside the RPC crate. This module keeps native Policy reads behind a local
//! provider seam instead of constructing native contracts directly inside fee
//! and transaction-building algorithms.

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use neo_storage::DataCache;
use std::sync::Arc;

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
#[derive(Clone)]
pub(super) struct NativeWalletCompatProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeWalletCompatProvider {
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

impl std::fmt::Debug for NativeWalletCompatProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeWalletCompatProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl WalletCompatNativeProvider for NativeWalletCompatProvider {
    fn exec_fee_factor(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>(|policy| {
            policy.get_exec_fee_factor_snapshot(snapshot, settings, block_index)
        })
    }

    fn fee_per_byte(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.with_contract::<PolicyContract, _>(|policy| policy.get_fee_per_byte_snapshot(snapshot))
    }
}
