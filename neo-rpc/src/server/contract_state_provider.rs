//! Shared deployed-contract storage provider for RPC handlers.
//!
//! RPC handlers frequently need to resolve a deployed contract by script hash
//! before projecting a JSON response or building a verification witness. This
//! adapter centralizes the ContractManagement storage codec so handlers depend
//! on a small capability instead of reaching into native-contract internals.

use neo_error::CoreResult;
use neo_execution::contract_state::ContractState;
use neo_native_contracts::contract_management::ContractManagement;
use neo_primitives::UInt160;
use neo_storage::DataCache;

/// Read capability for deployed ContractManagement records.
pub(crate) trait DeployedContractProvider {
    /// Returns the deployed contract state for `script_hash`, when present.
    fn contract_state(
        &self,
        snapshot: &DataCache,
        script_hash: &UInt160,
    ) -> CoreResult<Option<ContractState>>;
}

/// Factory for deployed-contract providers.
pub(crate) trait DeployedContractProviderFactory {
    /// Provider returned by this factory.
    type Provider: DeployedContractProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Native ContractManagement-backed deployed-contract provider.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeDeployedContractProvider;

impl DeployedContractProvider for NativeDeployedContractProvider {
    fn contract_state(
        &self,
        snapshot: &DataCache,
        script_hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        ContractManagement::get_contract_from_snapshot(snapshot, script_hash)
    }
}

/// Factory for native ContractManagement-backed deployed-contract providers.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeDeployedContractProviderFactory;

impl DeployedContractProviderFactory for NativeDeployedContractProviderFactory {
    type Provider = NativeDeployedContractProvider;

    fn provider(&self) -> Self::Provider {
        NativeDeployedContractProvider
    }
}
