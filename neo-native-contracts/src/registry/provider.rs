//! Standard native-contract provider.
//!
//! Implements neo-execution's [`NativeContractProvider`] seam over the concrete
//! native contracts in this crate. Composition roots pass this provider into
//! engines and services so `ApplicationEngine` can dispatch `System.Contract.Call`
//! to a native contract without `neo-execution` depending on
//! `neo-native-contracts` (which would be a crate cycle).
//!
//! The canonical catalog in [`crate::catalog`] is the single source of truth for
//! standard-contract order, id, name, hash, and construction.

use std::sync::Arc;

use neo_execution::NativeContract;
use neo_execution::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use neo_primitives::UInt160;

use crate::LedgerContract;
use crate::catalog::{
    StandardNativeContractSpec, standard_native_contract_hashes,
    standard_native_contract_spec_by_hash, standard_native_contract_spec_by_name,
    standard_native_contracts,
};

/// Provider over every standard native contract, in canonical C# id order.
pub struct StandardNativeProvider {
    contracts: Vec<Arc<dyn NativeContract>>,
}

impl StandardNativeProvider {
    /// Builds the provider from the canonical standard native-contract catalog.
    pub fn new() -> Self {
        Self {
            contracts: standard_native_contracts(),
        }
    }

    fn contract_for_spec(
        &self,
        spec: StandardNativeContractSpec,
    ) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .iter()
            .find(|contract| contract.id() == spec.id)
            .cloned()
    }
}

neo_io::impl_default_via_new!(StandardNativeProvider);

impl NativeContractProvider for StandardNativeProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        standard_native_contract_spec_by_hash(hash).and_then(|spec| self.contract_for_spec(spec))
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        standard_native_contract_spec_by_name(name).and_then(|spec| self.contract_for_spec(spec))
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        self.contracts.clone()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        standard_native_contract_hashes().into_iter().collect()
    }

    fn current_block_index(&self, snapshot: &neo_storage::DataCache) -> neo_error::CoreResult<u32> {
        LedgerContract::new().current_index(snapshot)
    }
}

/// Installs the standard native-contract provider into neo-execution's
/// compatibility bridge.
///
/// Production node composition should pass [`StandardNativeProvider`] explicitly
/// instead of mutating the process-global slot. This helper remains for tests
/// and standalone compatibility callers that still exercise
/// [`neo_execution::native_contract_provider::NativeContractLookup`] directly.
pub fn install() {
    NativeContractLookup::install_provider(Arc::new(StandardNativeProvider::new()));
}

#[cfg(test)]
#[path = "../tests/registry/provider.rs"]
mod tests;
