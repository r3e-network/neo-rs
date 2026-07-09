//! Native-contract read capabilities for node RPC handlers.
//!
//! Node RPC handlers assemble public node metadata. Dynamic `getversion`
//! protocol fields still come from native Ledger/Policy state, so this module
//! keeps Policy reads behind a local provider seam and leaves the handler
//! focused on response assembly.

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::PolicyContract;
use neo_storage::persistence::DataCache;
use std::sync::Arc;

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
#[derive(Clone)]
pub(super) struct NativeNodeProvider {
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl NativeNodeProvider {
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<dyn NativeContractProvider>) -> Self {
        Self {
            native_contract_provider,
        }
    }

    fn with_contract<T, R>(&self, f: impl FnOnce(&T) -> CoreResult<R>) -> Result<R, RpcException>
    where
        T: 'static,
    {
        let contract = self
            .native_contract_provider
            .get_native_contract_by_name("PolicyContract")
            .ok_or_else(|| {
                internal_error(CoreError::invalid_operation(
                    "native provider missing PolicyContract",
                ))
            })?;
        let policy = contract.as_any().downcast_ref::<T>().ok_or_else(|| {
            internal_error(CoreError::invalid_operation(
                "native provider returned non-PolicyContract",
            ))
        })?;
        f(policy).map_err(internal_error)
    }
}

impl std::fmt::Debug for NativeNodeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeNodeProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl NodeNativeProvider for NativeNodeProvider {
    fn version_policy_values(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException> {
        self.with_contract::<PolicyContract, _>(|policy| {
            Ok(VersionPolicyValues {
                milliseconds_per_block: policy
                    .get_milliseconds_per_block_snapshot(snapshot, settings)?,
                max_traceable_blocks: policy
                    .get_max_traceable_blocks_snapshot(snapshot, settings)?,
                max_valid_until_block_increment: policy
                    .get_max_valid_until_block_increment_snapshot(snapshot, settings)?,
            })
        })
    }
}
