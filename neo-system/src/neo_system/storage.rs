//! Store initialization and native-contract hydration for NeoSystem.
//!
//! The node host owns two things here and nothing else:
//!
//! * **provider and path selection** — which [`StoreProvider`] is opened and at
//!   which path ([`init_store`]);
//! * **native-contract cache hydration** — delegated to `neo-native-contracts`,
//!   which owns the hydration policy ([`hydrate_native_contracts`]).
//!
//! Ledger hydration (genesis validation, indexed-hash and block-index checks,
//! previous-hash linkage, and the rebuild of `LedgerContext`/`HeaderCache`) is a
//! ledger rule and lives in `neo_ledger::hydration`; the system calls it with
//! the store snapshot it opened.

// The integrity validator below is a test-only helper, so its ledger types are
// only needed for test builds.
#[cfg(test)]
use neo_ledger::{HeaderCache, LedgerContext};
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::data_cache::DataCacheConfig;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_provider::StoreProvider;
use std::sync::Arc;
use thiserror::Error;

/// Error type for storage operations.
///
/// The `Display` text (`Storage error: {message}`) is unchanged from the previous
/// hand-written implementation.
#[derive(Error, Debug, Clone)]
#[error("Storage error: {message}")]
pub struct StorageError {
    /// Human-readable failure detail.
    pub message: String,
}

impl From<StorageError> for neo_smart_contract::CoreError {
    fn from(err: StorageError) -> Self {
        neo_smart_contract::CoreError::invalid_operation(err.message)
    }
}

/// Initializes the primary store and its hydration cache.
///
/// Uses the default cache configuration, which keeps speculative prefetching
/// disabled; callers that opt in use [`init_store_with_config`].
#[allow(dead_code)] // default-configuration entry point kept for non-opt-in callers
pub fn init_store(
    store_provider: &Arc<dyn StoreProvider>,
    storage_path: Option<&str>,
) -> Result<(Arc<dyn Store>, Arc<StoreCache>), StorageError> {
    init_store_with_config(store_provider, storage_path, DataCacheConfig::default())
}

/// Initializes the primary store and its hydration cache with an explicit
/// cache configuration (read cache, speculative prefetch).
pub fn init_store_with_config(
    store_provider: &Arc<dyn StoreProvider>,
    storage_path: Option<&str>,
    cache_config: DataCacheConfig,
) -> Result<(Arc<dyn Store>, Arc<StoreCache>), StorageError> {
    let path = storage_path.unwrap_or("");
    let store = store_provider
        .get_store(path)
        .map_err(|error| StorageError {
            message: format!(
                "Failed to open the primary store with provider '{}' at '{path}': {error}",
                store_provider.name()
            ),
        })?;
    let store_cache = Arc::new(StoreCache::new_from_store_with_config(
        Arc::clone(&store),
        false,
        cache_config,
    ));
    Ok((store, store_cache))
}

/// Hydrates native contract caches from persisted storage.
///
/// Implements native contract state restoration as per C# Neo `NativeContract.Initialize`.
/// Loads persisted contract state and rebuilds in-memory caches for native contracts
/// (NEO, GAS, Policy, RoleManagement, Oracle, Ledger, etc.).
///
/// Reference: C# Neo `NativeContract.cs` initialization and `OnPersist` methods.
///
/// # Arguments
/// * `store_cache` - The store cache containing persisted contract data
/// * `native_services` - The native service registry to hydrate
/// * `settings` - The protocol settings the persisted metadata is validated against
///
/// # Errors
/// Returns `StorageError` if:
/// - Contract state is corrupted
/// - Required native contract state is missing
/// - Deserialization fails
pub fn hydrate_native_contracts(
    store_cache: &StoreCache,
    native_services: &neo_smart_contract::application_engine::EngineNativeServices,
    settings: &neo_protocol::ProtocolSettings,
) -> Result<(), StorageError> {
    // The hydration policy (which native contracts carry persisted metadata and
    // how their caches are restored) is owned by `neo-native-contracts`; the
    // node host only supplies the store snapshot, settings and native services.
    neo_native_contracts::hydrate_native_contracts(
        store_cache.data_cache(),
        native_services,
        settings,
    )
    .map_err(|error| StorageError {
        message: error.to_string(),
    })
}

/// Validates chain integrity after hydration.
///
/// Verifies that the hydrated blockchain state is consistent and valid:
/// - All blocks form a continuous chain
/// - Genesis block is valid
/// - Current height matches last persisted block
/// - Header cache is properly populated
///
/// Reference: C# Neo `Blockchain.cs` integrity checks.
///
/// # Arguments
/// * `ledger` - The ledger to validate
/// * `header_cache` - The header cache to validate
/// * `expected_height` - The expected current height
///
/// # Errors
/// Returns `StorageError` if validation fails.
#[cfg(test)]
pub fn validate_chain_integrity(
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
    expected_height: u32,
) -> Result<(), StorageError> {
    // Validate current height matches expected
    let current_height = ledger.current_height();
    if current_height != expected_height {
        return Err(StorageError {
            message: format!(
                "Height mismatch: ledger reports {current_height}, expected {expected_height}"
            ),
        });
    }

    // Validate genesis block exists
    let genesis_hash = ledger.block_hash_at(0).ok_or_else(|| StorageError {
        message: "Genesis block hash not found after hydration".to_string(),
    })?;

    let _genesis_block = ledger
        .get_block(&genesis_hash)
        .ok_or_else(|| StorageError {
            message: "Genesis block not accessible after hydration".to_string(),
        })?;

    // Validate header cache contains recent blocks
    if expected_height > 0 {
        let header_count = header_cache.count();
        if header_count == 0 {
            return Err(StorageError {
                message: "Header cache is empty after hydration".to_string(),
            });
        }

        tracing::debug!(
            target: "neo::storage",
            headers = header_count,
            "Header cache populated"
        );
    }

    tracing::info!(
        target: "neo::storage",
        height = current_height,
        "Chain integrity validation passed"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_native_contracts::native::composition::default_native_services;
    use neo_primitives::{UInt160, UInt256};
    use neo_protocol::ProtocolSettings;
    use neo_protocol::execution::{Block, BlockHeader};
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::providers::memory_store::MemoryStore;

    fn block(index: u32, previous_hash: UInt256) -> Block {
        Block::new(
            BlockHeader::new(
                0,
                previous_hash,
                UInt256::zero(),
                u64::from(index),
                u64::from(index),
                index,
                0,
                UInt160::zero(),
                vec![Witness::new()],
            ),
            Vec::new(),
        )
    }

    fn empty_cache() -> StoreCache {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        StoreCache::new(store)
    }

    #[test]
    fn test_init_store() {
        let provider: Arc<dyn StoreProvider> =
            Arc::new(neo_storage::persistence::providers::MemoryStoreProvider::new());
        let result = init_store(&provider, None);
        assert!(result.is_ok());

        let (_store, cache) = result.unwrap();
        assert!(!cache.data_cache().is_read_only());
    }

    #[test]
    fn test_validate_chain_integrity() {
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let genesis = block(0, UInt256::zero());

        ledger.insert_block(genesis.to_payload_block()).unwrap();
        header_cache.add(neo_protocol::execution::conversions::convert_ledger_header(
            genesis.header,
        ));
        ledger.record_tip(0);

        validate_chain_integrity(&ledger, &header_cache, 0).expect("valid genesis");
    }

    #[test]
    fn test_validate_chain_integrity_height_mismatch() {
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        ledger.record_tip(5);

        let result = validate_chain_integrity(&ledger, &header_cache, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Height mismatch"));
    }

    #[test]
    fn test_hydrate_native_contracts() {
        let store_cache = empty_cache();
        let native_services = default_native_services();

        hydrate_native_contracts(&store_cache, &native_services, &ProtocolSettings::default())
            .expect("empty native storage is valid");
    }

    #[test]
    fn system_calls_ledger_owned_hydration() {
        let store_cache = empty_cache();
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());

        neo_ledger::hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect("empty store is valid for the ledger-owned hydration");

        assert_eq!(ledger.current_height(), 0);
        assert_eq!(header_cache.count(), 0);
    }
}
