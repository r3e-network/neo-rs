//! Standard native-contract registry construction for RPC probes.
//!
//! `NativeRegistry::new()` is empty by design. RPC query tests and handlers
//! need the protocol-native contract set registered explicitly before resolving
//! native hashes, ids, and manifests.

use super::NativeQueries;

impl NativeQueries {
    /// Builds a [`neo_execution::NativeRegistry`] populated with the standard
    /// native contracts.
    pub(crate) fn native_registry() -> neo_execution::NativeRegistry {
        let mut registry = neo_execution::NativeRegistry::new();
        for contract in neo_native_contracts::standard_native_contracts() {
            registry.register(contract);
        }
        registry
    }
}
