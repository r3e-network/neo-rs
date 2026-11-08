//! Native contract definitions, metadata, and helper macros.

/// Deterministic genesis block construction from protocol settings.
pub mod genesis;
pub mod native;

// Re-export the native-contract surface at the crate root so downstream crates
// (neo-ledger, neo-system) can keep using the pre-decomposition
// `neo_native_contracts::X` paths that `neo-core` used to provide.
pub use native::*;

// Genesis lives here (not in neo-core) so the native-contract storage
// initialization and the genesis block that seeds it stay in one crate.
pub use genesis::create_genesis_block;

/// Oracle module re-export for convenience
pub mod oracle {
    pub use crate::native::oracle_request::OracleRequest;
    pub use crate::native::oracle_contract::OracleContract;
}

/// Returns the default native services configuration.
///
/// This creates a standard native contract registry with all built-in contracts.
pub fn default_native_services() -> native::NativeRegistry {
    native::composition::standard_registry()
}

/// Creates a basic EngineNativeServices from a NativeRegistry.
///
/// This is a convenience helper for tests and simple scenarios. Production code
/// should construct EngineNativeServices with proper state readers and resolvers.
pub fn engine_native_services_from_registry(
    registry: native::NativeRegistry,
) -> neo_smart_contract::application_engine::EngineNativeServices {
    use neo_smart_contract::application_engine::{EngineNativeServices, NativeStateReader, WitnessHashResolver};
    use neo_smart_contract::{ContractState, CoreError, CoreResult};
    use neo_storage::persistence::DataCache;
    use neo_protocol::payloads::Signer;
    use neo_primitives::UInt160;
    use std::sync::Arc;

    // Minimal no-op implementations for basic scenarios
    struct NoopStateReader;
    impl NativeStateReader for NoopStateReader {
        fn contract(&self, _snapshot: &DataCache, _hash: &UInt160) -> CoreResult<Option<ContractState>> {
            Ok(None)
        }
        fn current_index(&self, _snapshot: &DataCache) -> CoreResult<u32> {
            Ok(0)
        }
        fn is_blocked(&self, _snapshot: &DataCache, _hash: &UInt160) -> CoreResult<bool> {
            Ok(false)
        }
        fn whitelisted_fee(
            &self,
            _snapshot: &DataCache,
            _hash: &UInt160,
            _method: &str,
            _param_count: u32,
        ) -> CoreResult<Option<i64>> {
            Ok(None)
        }
        fn oracle_original_signers(
            &self,
            _snapshot: &DataCache,
            _id: u64,
        ) -> CoreResult<Option<Vec<Signer>>> {
            Ok(None)
        }
    }

    struct NoopResolver;
    impl WitnessHashResolver for NoopResolver {
        fn resolve(
            &self,
            _verifiable: &dyn neo_primitives::Verifiable,
            _snapshot: &DataCache,
        ) -> Vec<UInt160> {
            Vec::new()
        }
    }

    EngineNativeServices {
        registry,
        state_reader: Arc::new(NoopStateReader),
        witness_hash_resolver: Arc::new(NoopResolver),
        policy_hash: UInt160::zero(),
        default_exec_fee_factor: 30,
        default_storage_price: 100_000,
    }
}

