//! Native contract definitions, metadata, and helper macros.

#![warn(missing_docs)]

/// Deterministic genesis block construction from protocol settings.
pub mod genesis;
/// Startup hydration policy for persisted native-contract metadata.
pub mod hydration;
pub mod native;

// Re-export the native-contract surface at the crate root so downstream crates
// (neo-ledger, neo-system) can keep using the pre-decomposition
// `neo_native_contracts::X` paths that `neo-core` used to provide.
pub use native::*;

// Genesis lives here (not in neo-core) so the native-contract storage
// initialization and the genesis block that seeds it stay in one crate.
pub use genesis::create_genesis_block;
pub use hydration::{NativeHydrationError, hydrate_native_contracts, metadata_hydration_contracts};

/// Oracle module re-export for convenience
pub mod oracle {
    pub use crate::native::oracle_contract::OracleContract;
    pub use crate::native::oracle_request::OracleRequest;
}

// The canonical, production native-service composition lives in
// `native::composition`. It is re-exported here so that
// `neo_native_contracts::default_native_services()` denotes exactly one thing
// across the workspace: the complete composition with the snapshot-backed
// `StandardNativeStateReader` and the `StandardWitnessHashResolver`.
pub use native::composition::{default_native_services, header_verifying_script_hashes};

/// Builds a **minimal** native-service composition around a caller-supplied
/// registry.
///
/// This is a test/diagnostic-only builder, not the production composition. The
/// returned `EngineNativeServices` uses a no-op state reader (no contract
/// state, block height always `0`, nothing blocked or whitelisted, no oracle
/// original signers) and a no-op witness-hash resolver (so every `CheckWitness`
/// fails).
///
/// Oracle verification, RPC engines and any other engine that must observe real
/// chain state MUST use [`default_native_services`] (equivalently
/// `native::composition::default_native_services()`) instead.
pub fn minimal_native_services_from_registry(
    registry: native::NativeRegistry,
) -> neo_smart_contract::application_engine::EngineNativeServices {
    use neo_primitives::UInt160;
    use neo_protocol::payloads::Signer;
    use neo_smart_contract::application_engine::{
        EngineNativeServices, NativeStateReader, WitnessHashResolver,
    };
    use neo_smart_contract::{ContractState, CoreResult};
    use neo_storage::persistence::PersistenceDataCache;
    use std::sync::Arc;

    // Minimal no-op implementations for basic scenarios: no chain state is
    // visible, which is only acceptable for tests and diagnostics.
    struct NoopStateReader;
    impl NativeStateReader for NoopStateReader {
        fn contract(
            &self,
            _snapshot: &PersistenceDataCache,
            _hash: &UInt160,
        ) -> CoreResult<Option<ContractState>> {
            Ok(None)
        }
        fn current_index(&self, _snapshot: &PersistenceDataCache) -> CoreResult<u32> {
            Ok(0)
        }
        fn is_blocked(
            &self,
            _snapshot: &PersistenceDataCache,
            _hash: &UInt160,
        ) -> CoreResult<bool> {
            Ok(false)
        }
        fn whitelisted_fee(
            &self,
            _snapshot: &PersistenceDataCache,
            _hash: &UInt160,
            _method: &str,
            _param_count: u32,
        ) -> CoreResult<Option<i64>> {
            Ok(None)
        }
        fn oracle_original_signers(
            &self,
            _snapshot: &PersistenceDataCache,
            _id: u64,
        ) -> CoreResult<Option<Vec<Signer>>> {
            Ok(None)
        }
    }

    // Resolves no verifying hashes, so witness checks always fail.
    struct NoopResolver;
    impl WitnessHashResolver for NoopResolver {
        fn resolve(
            &self,
            _verifiable: &dyn neo_primitives::Verifiable,
            _snapshot: &PersistenceDataCache,
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
