//! Native-contract read capabilities for node RPC handlers.
//!
//! Node RPC handlers assemble public node metadata. Dynamic `getversion`
//! protocol fields still come from native Ledger/Policy state, so this module
//! keeps Policy reads behind a local provider seam and leaves the handler
//! focused on response assembly.

use neo_config::ProtocolSettings;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use neo_storage::persistence::DataCache;
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
    fn version_policy_values(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException>;
}

/// Adapter from the node-composed native-contract provider to node RPC Policy
/// read capabilities.
#[derive(Clone, Debug)]
pub(super) struct NativeNodeProvider {
    adapter: NativeProviderAdapter,
}

impl NativeNodeProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            adapter: NativeProviderAdapter::new(native_contract_provider),
        }
    }
}

impl NodeNativeProvider for NativeNodeProvider {
    fn version_policy_values(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException> {
        self.adapter
            .with_contract::<PolicyContract, _>("PolicyContract", |policy| {
                Ok(VersionPolicyValues {
                    milliseconds_per_block: policy
                        .get_milliseconds_per_block_snapshot(snapshot, settings)?,
                    max_traceable_blocks: policy
                        .get_max_traceable_blocks_snapshot(snapshot, settings)?,
                    max_valid_until_block_increment: policy
                        .get_max_valid_until_block_increment_snapshot(snapshot, settings)?,
                })
            })
            .map_err(internal_error)
    }
}
