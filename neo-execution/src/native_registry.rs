//! Registry of native contract implementations.
//!
//! The `NativeRegistry` is a thin map of contract hash → `Arc<dyn
//! NativeContract>`. The application engine consults the registry to
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
use std::sync::Arc;

use crate::native_contract::NativeContract;

/// Registry for native contracts.
pub struct NativeRegistry {
    contracts: IndexMap<UInt160, Arc<dyn NativeContract>>,
}

impl NativeRegistry {
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
    pub fn register(&mut self, contract: Arc<dyn NativeContract>) {
        let hash = contract.hash();
        self.contracts.insert(hash, contract);
    }

    /// Gets a native contract by hash.
    pub fn get(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        self.contracts.get(hash).cloned()
    }

    /// Gets a native contract by name.
    pub fn get_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.contracts
            .values()
            .find(|contract| contract.name().eq_ignore_ascii_case(name))
            .cloned()
    }

    /// Internal helper to look up a contract hash by name.
    fn find_hash_by_name(&self, name: &str) -> Option<UInt160> {
        self.contracts
            .iter()
            .find(|(_, contract)| contract.name().eq_ignore_ascii_case(name))
            .map(|(hash, _)| *hash)
    }

    /// Removes a contract from the registry by name.
    pub fn take_contract_by_name(&mut self, name: &str) -> Option<Arc<dyn NativeContract>> {
        let hash = self.find_hash_by_name(name)?;
        self.contracts.shift_remove(&hash)
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
    pub fn contracts(&self) -> impl Iterator<Item = Arc<dyn NativeContract>> + '_ {
        self.contracts.values().cloned()
    }
}

neo_io::impl_default_via_new!(NativeRegistry);

#[cfg(test)]
#[path = "tests/native_registry.rs"]
mod tests;
