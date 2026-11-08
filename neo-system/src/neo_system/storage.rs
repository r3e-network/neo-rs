//! Storage initialization and hydration for NeoSystem.
//!
//! This module provides functions to initialize the storage layer and hydrate
//! the ledger from persisted state.

use neo_ledger::{LedgerContext, HeaderCache};
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::store::Store;
use crate::services::StateStoreService;
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

/// Concrete implementation of StateStoreService using MemoryStore.
impl StateStoreService for MemoryStore {
    fn on_new_state_root(&self, _state_root: crate::state_service::StateRoot) -> bool {
        // Memory store doesn't persist state roots
        true
    }
}

/// Initializes the storage layer for NeoSystem.
///
/// # Returns
/// A tuple containing:
/// - The main store
/// - A store cache for ledger hydration
/// - An optional state store
pub fn init_store(
) -> Result<(Arc<dyn Store>, Arc<StoreCache>, Option<Arc<dyn StateStoreService>>), StorageError> {
    let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
    let store_cache = Arc::new(StoreCache::new(Arc::clone(&store)));
    let state_store: Option<Arc<dyn StateStoreService>> = Some(Arc::new(MemoryStore::new()));

    Ok((store, store_cache, state_store))
}

/// Hydrates the ledger from persisted storage.
///
/// # Arguments
/// * `store_cache` - The store cache containing persisted data
/// * `ledger` - The ledger to hydrate
/// * `header_cache` - The header cache to populate
pub fn hydrate_ledger(
    _store_cache: &Arc<StoreCache>,
    _ledger: &Arc<LedgerContext>,
    _header_cache: &Arc<HeaderCache>,
) -> Result<(), StorageError> {
    // Stub implementation - full hydration logic would:
    // 1. Load persisted blocks from store_cache
    // 2. Populate ledger with block data
    // 3. Build header_cache from block headers
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_store() {
        let result = init_store();
        assert!(result.is_ok());

        let (_store, _cache, state_store) = result.unwrap();
        assert!(state_store.is_some());
    }

    #[test]
    fn test_hydrate_ledger_stub() {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let store_cache = Arc::new(StoreCache::new(store));

        // Create minimal ledger and header cache for testing
        // This is a stub - actual test would need proper initialization
        let ledger = Arc::new(Ledger::new());
        let header_cache = Arc::new(HeaderCache::new());

        let result = hydrate_ledger(&store_cache, &ledger, &header_cache);
        assert!(result.is_ok());
    }
}
