//! Shared native-contract provider adapter for RPC handlers.
//!
//! RPC handlers expose narrow, feature-local provider traits, and each of those
//! traits adapts the same composition-root native-contract provider. This helper
//! centralizes typed capability reads and redacted debug output so individual
//! RPC modules only describe the facts they need.

use neo_config::ProtocolSettings;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_primitives::TransactionAttributeType;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

/// Adapter over the node-composed native-contract provider.
#[derive(Clone)]
pub(crate) struct NativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    native_contract_provider: Arc<P>,
}

impl<P> NativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(crate) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    /// Returns Policy.MaxValidUntilBlockIncrement through the provider
    /// capability surface.
    pub(crate) fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .max_valid_until_block_increment(snapshot, settings)
    }

    /// Returns Policy.MillisecondsPerBlock through the provider capability surface.
    pub(crate) fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .milliseconds_per_block(snapshot, settings)
    }

    /// Returns Policy.FeePerByte through the provider capability surface.
    pub(crate) fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.native_contract_provider.fee_per_byte(snapshot)
    }

    /// Returns the Policy fee for one transaction attribute type.
    pub(crate) fn attribute_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        attribute_type: TransactionAttributeType,
    ) -> CoreResult<i64> {
        self.native_contract_provider
            .attribute_fee(snapshot, attribute_type)
    }

    /// Returns Policy.ExecFeeFactor through the provider capability surface.
    pub(crate) fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .exec_fee_factor(snapshot, settings, block_index)
    }

    /// Returns Policy.MaxTraceableBlocks through the provider capability surface.
    pub(crate) fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .max_traceable_blocks(snapshot, settings)
    }
}

impl<P> std::fmt::Debug for NativeProviderAdapter<P>
where
    P: NativeContractProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeProviderAdapter")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}
