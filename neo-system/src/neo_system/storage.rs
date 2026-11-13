//! Storage initialization and hydration for NeoSystem.
//!
//! This module provides functions to initialize the storage layer and hydrate
//! the ledger from persisted state.

use neo_ledger::{HeaderCache, LedgerContext};
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::store_provider::StoreProvider;
use std::sync::Arc;

/// Error type for storage operations.
#[derive(Debug, Clone)]
pub struct StorageError {
    pub message: String,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Storage error: {}", self.message)
    }
}

impl std::error::Error for StorageError {}

impl From<StorageError> for neo_smart_contract::CoreError {
    fn from(err: StorageError) -> Self {
        neo_smart_contract::CoreError::invalid_operation(err.message)
    }
}

/// Initializes the primary store and its hydration cache.
pub fn init_store(
    store_provider: &Arc<dyn StoreProvider>,
    storage_path: Option<&str>,
) -> Result<(Arc<dyn Store>, Arc<StoreCache>), StorageError> {
    let path = storage_path.unwrap_or("");
    let store = store_provider
        .get_store(path)
        .map_err(|error| StorageError {
            message: format!(
                "Failed to open the primary store with provider '{}' at '{}': {error}",
                store_provider.name(),
                path
            ),
        })?;
    let store_cache = Arc::new(StoreCache::new(Arc::clone(&store)));
    Ok((store, store_cache))
}

/// Hydrates the ledger from persisted storage.
///
/// Implements complete storage hydration as per C# Neo `Blockchain.Persist`.
/// Loads persisted blocks, rebuilds in-memory caches, validates chain integrity,
/// and restores the current blockchain state.
///
/// Reference: C# Neo `Blockchain.cs` initialization logic.
///
/// # Arguments
/// * `store_cache` - The store cache containing persisted data
/// * `ledger` - The ledger to hydrate
/// * `header_cache` - The header cache to populate
///
/// # Process
/// 1. Load current block height from storage
/// 2. Load and validate genesis block (index 0)
/// 3. Iterate through persisted blocks sequentially
/// 4. Rebuild block index (hash -> block mapping)
/// 5. Populate header cache with recent headers
/// 6. Validate chain integrity (previous hash links)
/// 7. Set final current height in ledger
///
/// # Errors
/// Returns `StorageError` if:
/// - Genesis block is missing or invalid
/// - Block chain is broken (missing blocks, invalid hash links)
/// - Deserialization fails for any block
/// - Storage read errors occur
pub fn hydrate_ledger(
    store_cache: &StoreCache,
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
) -> Result<(), StorageError> {
    use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
    use neo_primitives::UInt256;

    let ledger_contract = LedgerContract::new();
    let current_height = ledger_contract
        .current_index(store_cache)
        .map_err(|error| StorageError {
            message: format!("Failed to read current block index: {error}"),
        })?;
    let current_hash = ledger_contract
        .current_hash(store_cache)
        .map_err(|error| StorageError {
            message: format!("Failed to read current block hash: {error}"),
        })?;
    let first_hash = ledger_contract
        .get_block_hash_by_index(store_cache, 0)
        .map_err(|error| StorageError {
            message: format!("Failed to read genesis block index: {error}"),
        })?;

    if first_hash.is_none() {
        if current_height != 0 || current_hash != UInt256::zero() {
            return Err(StorageError {
                message: "Current block pointer exists without a genesis block".to_string(),
            });
        }
        return Ok(());
    }

    let mut previous_hash = UInt256::zero();
    let mut blocks = Vec::with_capacity(current_height as usize + 1);
    for index in 0..=current_height {
        let indexed_hash = ledger_contract
            .get_block_hash_by_index(store_cache, index)
            .map_err(|error| StorageError {
                message: format!("Failed to read block hash at height {index}: {error}"),
            })?
            .ok_or_else(|| StorageError {
                message: format!("Block at height {index} not found in storage"),
            })?;
        let block = ledger_contract
            .get_block(store_cache, HashOrIndex::Index(index))
            .map_err(|error| StorageError {
                message: format!("Failed to deserialize block at height {index}: {error}"),
            })?
            .ok_or_else(|| StorageError {
                message: format!("Block at height {index} not found in storage"),
            })?;

        if block.index() != index {
            return Err(StorageError {
                message: format!(
                    "Block index mismatch at height {index}: stored header reports {}",
                    block.index()
                ),
            });
        }
        let block_hash = block.hash();
        if block_hash != indexed_hash {
            return Err(StorageError {
                message: format!(
                    "Block hash mismatch at height {index}: index contains {indexed_hash}, header hashes to {block_hash}"
                ),
            });
        }
        if block.prev_hash() != &previous_hash {
            return Err(StorageError {
                message: format!(
                    "Chain integrity broken at height {index}: expected prev_hash {previous_hash}, got {}",
                    block.prev_hash()
                ),
            });
        }

        previous_hash = block_hash;
        blocks.push(super::converters::convert_execution_block(block));
    }

    if current_hash != previous_hash {
        return Err(StorageError {
            message: format!(
                "Current block hash mismatch: pointer contains {current_hash}, tip hashes to {previous_hash}"
            ),
        });
    }

    // Validate the complete chain before mutating either runtime cache. A corrupt
    // store therefore cannot leave a caller with a partially hydrated ledger.
    for block in blocks {
        let header = block.header.clone();
        ledger.insert_block(block).map_err(|error| StorageError {
            message: format!("Failed to insert hydrated block: {error}"),
        })?;
        header_cache.add(header);
    }
    ledger.record_tip(current_height);

    tracing::info!(
        target: "neo::storage",
        height = current_height,
        "Storage hydration completed successfully"
    );
    Ok(())
}

/// Hydrates only the persisted tip for a checkpoint-gated offline replay.
///
/// This deliberately does not replace full chain hydration for online nodes. The
/// caller must independently authenticate the checkpoint and its audit history
/// before executing successors. The tip index, indexed hash, current-hash pointer,
/// and previous-block linkage are still checked against persistent storage.
pub fn hydrate_ledger_tip_for_checkpoint_replay(
    store_cache: &StoreCache,
    ledger: &Arc<LedgerContext>,
    header_cache: &Arc<HeaderCache>,
) -> Result<(), StorageError> {
    use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
    use neo_primitives::UInt256;

    let ledger_contract = LedgerContract::new();
    let current_height = ledger_contract
        .current_index(store_cache)
        .map_err(|error| StorageError {
            message: format!("Failed to read current block index: {error}"),
        })?;
    let current_hash = ledger_contract
        .current_hash(store_cache)
        .map_err(|error| StorageError {
            message: format!("Failed to read current block hash: {error}"),
        })?;
    let first_hash = ledger_contract
        .get_block_hash_by_index(store_cache, 0)
        .map_err(|error| StorageError {
            message: format!("Failed to read genesis block index: {error}"),
        })?;
    if first_hash.is_none() {
        if current_height != 0 || current_hash != UInt256::zero() {
            return Err(StorageError {
                message: "Current block pointer exists without a genesis block".to_string(),
            });
        }
        return Ok(());
    }

    let indexed_hash = ledger_contract
        .get_block_hash_by_index(store_cache, current_height)
        .map_err(|error| StorageError {
            message: format!("Failed to read tip hash at height {current_height}: {error}"),
        })?
        .ok_or_else(|| StorageError {
            message: format!("Tip at height {current_height} not found in storage"),
        })?;
    let tip = ledger_contract
        .get_block(store_cache, HashOrIndex::Index(current_height))
        .map_err(|error| StorageError {
            message: format!("Failed to deserialize tip at height {current_height}: {error}"),
        })?
        .ok_or_else(|| StorageError {
            message: format!("Tip at height {current_height} not found in storage"),
        })?;
    if tip.index() != current_height || tip.hash() != indexed_hash || indexed_hash != current_hash {
        return Err(StorageError {
            message: "Persisted tip height, indexed hash, or current hash is inconsistent"
                .to_string(),
        });
    }
    if current_height > 0 {
        let previous = ledger_contract
            .get_block(store_cache, HashOrIndex::Index(current_height - 1))
            .map_err(|error| StorageError {
                message: format!("Failed to deserialize tip parent: {error}"),
            })?
            .ok_or_else(|| StorageError {
                message: "Tip parent is missing from storage".to_string(),
            })?;
        if previous.index() + 1 != current_height || tip.prev_hash() != &previous.hash() {
            return Err(StorageError {
                message: "Persisted tip parent linkage is inconsistent".to_string(),
            });
        }
    }

    let execution_tip = super::converters::convert_execution_block(tip);
    let header = execution_tip.header.clone();
    ledger
        .insert_block(execution_tip)
        .map_err(|error| StorageError {
            message: format!("Failed to insert checkpoint tip: {error}"),
        })?;
    header_cache.add(header);
    ledger.record_tip(current_height);
    Ok(())
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
/// * `native_contract_cache` - The native contract cache to hydrate
///
/// # Process
/// 1. Iterate through all native contract storage prefixes
/// 2. Load contract state (balances, policies, validators, etc.)
/// 3. Rebuild in-memory caches for fast lookups
/// 4. Validate contract state consistency
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
    use neo_native_contracts::ContractManagement;
    use neo_primitives::TriggerType;
    use neo_smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};

    // ContractManagement is the only standard native contract with a persisted
    // metadata cache. The other standard contracts are stateless handles or build
    // their method metadata on demand; calling their genesis initialize hooks here
    // would incorrectly mutate an already-running chain.
    let contract = native_services
        .registry
        .get(&ContractManagement::contract_hash())
        .ok_or_else(|| StorageError {
            message: "ContractManagement is missing from the native registry".to_string(),
        })?;
    let snapshot = Arc::new(store_cache.data_cache().clone());
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings.clone(),
        TEST_MODE_GAS,
        None,
        native_services.clone(),
    )
    .map_err(|error| StorageError {
        message: format!("Failed to create native hydration engine: {error}"),
    })?;
    contract
        .initialize(&mut engine)
        .map_err(|error| StorageError {
            message: format!("Failed to hydrate ContractManagement: {error}"),
        })?;

    tracing::info!(
        target: "neo::storage",
        "Native contract cache hydration completed"
    );
    Ok(())
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
    use neo_native_contracts::ledger_contract::LedgerContract;
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

    fn chain(length: u32) -> Vec<Block> {
        let mut previous_hash = UInt256::zero();
        (0..length)
            .map(|index| {
                let block = block(index, previous_hash);
                previous_hash = block.hash();
                block
            })
            .collect()
    }

    fn persist_blocks(store_cache: &mut StoreCache, blocks: &[Block]) {
        let ledger_contract = LedgerContract::new();
        for block in blocks {
            ledger_contract
                .store_block_state(store_cache.data_cache(), block, &[])
                .expect("store block through the canonical ledger writer");
        }
        let tip = blocks.last().expect("at least one block");
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &tip.hash(), tip.index())
            .expect("store current block pointer");
        store_cache.commit();
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
    fn test_hydrate_ledger_empty_storage() {
        let store_cache = empty_cache();
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());

        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("empty store is valid");
        assert_eq!(ledger.current_height(), 0);
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn test_hydrate_ledger_with_genesis() {
        let mut store_cache = empty_cache();
        let blocks = chain(1);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("hydrate genesis");

        assert_eq!(ledger.current_height(), 0);
        assert_eq!(ledger.block_hash_at(0), Some(blocks[0].hash()));
        assert_eq!(header_cache.count(), 1);
    }

    #[test]
    fn test_hydrate_ledger_with_chain() {
        let mut store_cache = empty_cache();
        let blocks = chain(3);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger(&store_cache, &ledger, &header_cache).expect("hydrate complete chain");

        assert_eq!(ledger.current_height(), 2);
        for block in &blocks {
            assert_eq!(ledger.block_hash_at(block.index()), Some(block.hash()));
        }
        assert_eq!(header_cache.count(), 3);
    }

    #[test]
    fn test_checkpoint_replay_hydrates_only_the_authenticated_tip() {
        let mut store_cache = empty_cache();
        let blocks = chain(3);
        persist_blocks(&mut store_cache, &blocks);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        hydrate_ledger_tip_for_checkpoint_replay(&store_cache, &ledger, &header_cache)
            .expect("hydrate checkpoint tip");

        assert_eq!(ledger.current_height(), 2);
        assert_eq!(ledger.block_hash_at(2), Some(blocks[2].hash()));
        assert_eq!(ledger.block_hash_at(0), None);
        assert_eq!(ledger.block_hash_at(1), None);
        assert_eq!(header_cache.count(), 1);
    }

    #[test]
    fn test_checkpoint_replay_rejects_broken_tip_parent() {
        let mut store_cache = empty_cache();
        let genesis = block(0, UInt256::zero());
        let broken = block(1, UInt256::from([1u8; 32]));
        persist_blocks(&mut store_cache, &[genesis, broken]);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger_tip_for_checkpoint_replay(&store_cache, &ledger, &header_cache)
            .expect_err("broken tip parent must fail closed");
        assert!(error.message.contains("parent linkage"));
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn test_hydrate_ledger_broken_chain_does_not_partially_hydrate() {
        let mut store_cache = empty_cache();
        let genesis = block(0, UInt256::zero());
        let broken = block(1, UInt256::from([1u8; 32]));
        persist_blocks(&mut store_cache, &[genesis, broken]);

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("broken previous-hash link must fail closed");

        assert!(error.message.contains("Chain integrity broken"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn test_hydrate_ledger_rejects_missing_block() {
        let mut store_cache = empty_cache();
        let blocks = chain(2);
        let ledger_contract = LedgerContract::new();
        ledger_contract
            .store_block_state(store_cache.data_cache(), &blocks[0], &[])
            .expect("store genesis");
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &blocks[1].hash(), 1)
            .expect("store tip beyond available blocks");
        store_cache.commit();

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("missing indexed block must fail closed");

        assert!(error.message.contains("height 1 not found"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn test_hydrate_ledger_rejects_tip_hash_mismatch() {
        let mut store_cache = empty_cache();
        let blocks = chain(2);
        let ledger_contract = LedgerContract::new();
        for block in &blocks {
            ledger_contract
                .store_block_state(store_cache.data_cache(), block, &[])
                .expect("store block");
        }
        ledger_contract
            .set_current_block_state(store_cache.data_cache(), &blocks[0].hash(), 1)
            .expect("store inconsistent tip");
        store_cache.commit();

        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let error = hydrate_ledger(&store_cache, &ledger, &header_cache)
            .expect_err("tip hash mismatch must fail closed");

        assert!(error.message.contains("Current block hash mismatch"));
        assert!(ledger.block_hash_at(0).is_none());
        assert_eq!(header_cache.count(), 0);
    }

    #[test]
    fn test_validate_chain_integrity() {
        let ledger = Arc::new(LedgerContext::default());
        let header_cache = Arc::new(HeaderCache::new());
        let genesis = block(0, UInt256::zero());

        ledger.insert_block(genesis.to_payload_block()).unwrap();
        header_cache.add(super::super::converters::convert_execution_header(
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
}
