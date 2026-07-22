//! Native-contract read capabilities for wallet compatibility helpers.
//!
//! Wallet compatibility flows mirror C# wallet helper logic while staying
//! inside the RPC crate. This module keeps native Policy reads behind a local
//! provider seam instead of constructing native contracts directly inside fee
//! and transaction-building algorithms.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::TransactionAttributeType;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;

/// Native-contract capabilities required by wallet compatibility helpers.
pub(super) trait WalletCompatNativeProvider {
    /// Returns the active `ExecFeeFactor` at `block_index`.
    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32>;

    /// Returns the active `FeePerByte`.
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32>;

    /// Returns the Policy fee for one transaction attribute type.
    fn attribute_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        attribute_type: TransactionAttributeType,
    ) -> CoreResult<i64>;
}

/// Adapter from the node-composed native-contract provider to wallet
/// compatibility Policy read capabilities.
#[derive(Clone, Debug)]
pub(super) struct NativeWalletCompatProvider<P>
where
    P: NativeContractProvider,
{
    adapter: NativeProviderAdapter<P>,
}

impl<P> NativeWalletCompatProvider<P>
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

impl<P> WalletCompatNativeProvider for NativeWalletCompatProvider<P>
where
    P: NativeContractProvider,
{
    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32> {
        self.adapter
            .exec_fee_factor(snapshot, settings, block_index)
    }

    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.adapter.fee_per_byte(snapshot)
    }

    fn attribute_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        attribute_type: TransactionAttributeType,
    ) -> CoreResult<i64> {
        self.adapter.attribute_fee(snapshot, attribute_type)
    }
}
