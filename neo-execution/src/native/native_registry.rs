//! Registry of native contract implementations.
//!
//! The `NativeRegistry` is a thin map of contract hash to provider-typed
//! native contract handles. The application engine consults the registry to
//! dispatch `System.Contract.CallNative` and `getcontractstate`.
//!
//! The canonical home of this type is [`crate::native_registry`] (it
//! is owned by the application-engine crate because the engine is
//! the sole consumer). The default constructor creates an **empty**
//! registry; populating it with the standard native contracts is
//! the responsibility of the higher-level consumer (typically
//! `neo-native-contracts::populate_standard_native_contracts` or
//! similar). This keeps the dependency direction:
//!
//! ```text
//! neo-execution         ──> defines NativeContract / NativeRegistry
//!       ▲
//!       │ implements
//! neo-native-contracts  ──> provides BaseNativeContract + the 11 standard contracts
//! ```

use indexmap::IndexMap;
use neo_primitives::UInt160;

use crate::native_contract::NativeContract;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};

/// Registry for native contracts.
pub struct NativeRegistry<P = NoNativeContractProvider>
where
    P: NativeContractProvider + 'static,
{
    contracts: IndexMap<UInt160, P::Contract>,
}

impl<P> NativeRegistry<P>
where
    P: NativeContractProvider + 'static,
{
    /// Creates a new, empty native contract registry.
    ///
    /// Note: this used to pre-populate the registry with the standard
    /// native contracts; that responsibility has been moved to
    /// `neo-native-contracts` to break the crate-level dependency
    /// cycle between the engine (consumer) and the native contracts
    /// (provider). Callers that need the standard contracts should
    /// use `neo_native_contracts::populate_standard_native_contracts`
    /// or build their own `NativeRegistry` via [`NativeRegistry::new_empty`]
    /// followed by [`NativeRegistry::register`].
    pub fn new() -> Self {
        Self {
            contracts: IndexMap::new(),
        }
    }

    /// Creates a new, empty native contract registry (alias for
    /// [`NativeRegistry::new`]).
    pub fn new_empty() -> Self {
        Self::new()
    }

    /// Registers a native contract.
    pub fn register(&mut self, contract: P::Contract) {
        let hash = contract.hash();
        self.contracts.insert(hash, contract);
    }

    /// Gets a native contract by hash.
    pub fn get(&self, hash: &UInt160) -> Option<P::Contract> {
        self.contracts.get(hash).cloned()
    }

    /// Gets a native contract by name.
    pub fn get_by_name(&self, name: &str) -> Option<P::Contract> {
        self.contracts
            .values()
            .find(|contract| contract.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    /// Checks if a contract hash is a native contract.
    pub fn is_native(&self, hash: &UInt160) -> bool {
        self.contracts.contains_key(hash)
    }

    /// Gets all native contract hashes.
    pub fn all_hashes(&self) -> Vec<UInt160> {
        self.contracts.keys().copied().collect()
    }

    /// Returns all registered native contracts in deterministic registration order.
    ///
    /// Persistence order is consensus-critical. This follows the same
    /// declaration order as the standard registration routine in
    /// `neo-native-contracts`.
    pub fn contracts(&self) -> impl Iterator<Item = P::Contract> + '_ {
        self.contracts.values().cloned()
    }
}

impl<P> Default for NativeRegistry<P>
where
    P: NativeContractProvider + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/native/native_registry.rs"]
mod tests;
