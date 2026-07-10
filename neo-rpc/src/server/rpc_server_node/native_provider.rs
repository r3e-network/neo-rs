//! Native-contract read capabilities for node RPC handlers.
//!
//! Node RPC handlers assemble public node metadata. Dynamic `getversion`
//! protocol fields still come from native Ledger/Policy state, so this module
//! keeps Policy reads behind a local provider seam and leaves the handler
//! focused on response assembly.

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_storage::persistence::{CacheRead, DataCache};
use std::sync::Arc;

use crate::server::native_provider::NativeProviderAdapter;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;

/// Dynamic protocol fields surfaced by `getversion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VersionPolicyValues {
    /// Effective block time in milliseconds.
    pub(super) milliseconds_per_block: u32,
    /// Effective traceability window.
    pub(super) max_traceable_blocks: u32,
    /// Effective transaction validity-window increment.
    pub(super) max_valid_until_block_increment: u32,
}

/// Native-contract capabilities required by node RPC handlers.
pub(super) trait NodeNativeProvider {
    /// Returns the C# `NeoSystemExtensions` dynamic Policy values used by
    /// `getversion`.
    fn version_policy_values<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException>;
}

/// Adapter from the node-composed native-contract provider to node RPC Policy
/// read capabilities.
#[derive(Clone, Debug)]
pub(super) struct NativeNodeProvider<P>
where
    P: NativeContractProvider,
{
    adapter: NativeProviderAdapter<P>,
}

impl<P> NativeNodeProvider<P>
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

impl<P> NodeNativeProvider for NativeNodeProvider<P>
where
    P: NativeContractProvider,
{
    fn version_policy_values<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException> {
        Ok(VersionPolicyValues {
            milliseconds_per_block: self
                .adapter
                .milliseconds_per_block(snapshot, settings)
                .map_err(internal_error)?,
            max_traceable_blocks: self
                .adapter
                .max_traceable_blocks(snapshot, settings)
                .map_err(internal_error)?,
            max_valid_until_block_increment: self
                .adapter
                .max_valid_until_block_increment(snapshot, settings)
                .map_err(internal_error)?,
        })
    }
}
