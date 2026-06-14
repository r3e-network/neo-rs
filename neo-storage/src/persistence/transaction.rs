//! Thin transactional wrapper over `StoreCache` for explicit commit semantics.

use super::data_cache::{DataCacheConfig, DataCacheResult};
use super::store::Store;
use super::store_snapshot::StoreSnapshot;
use super::StoreCache;
use std::sync::Arc;

/// Represents a write transaction over a store or snapshot.
pub struct StoreTransaction {
    cache: StoreCache,
}

impl StoreTransaction {
    /// Creates a transaction bound to the provided store.
    pub fn from_store(store: Arc<dyn Store>, read_only: bool) -> Self {
        Self {
            cache: StoreCache::new_from_store(store, read_only),
        }
    }

    /// Creates a transaction backed by a snapshot.
    pub fn from_snapshot(snapshot: Arc<dyn StoreSnapshot>) -> Self {
        Self::from_snapshot_with_config(snapshot, DataCacheConfig::default())
    }

    /// Creates a transaction backed by a snapshot using a custom cache config.
    pub fn from_snapshot_with_config(
        snapshot: Arc<dyn StoreSnapshot>,
        config: DataCacheConfig,
    ) -> Self {
        Self {
            cache: StoreCache::new_from_snapshot_with_config(snapshot, config),
        }
    }

    /// Mutable access to the underlying cache for staged mutations.
    pub fn cache_mut(&mut self) -> &mut StoreCache {
        &mut self.cache
    }

    /// Immutable access to the underlying cache.
    pub fn cache(&self) -> &StoreCache {
        &self.cache
    }

    /// Commits the transaction.
    pub fn commit(mut self) -> DataCacheResult {
        self.cache.try_commit()
    }
}

impl From<StoreTransaction> for StoreCache {
    fn from(tx: StoreTransaction) -> Self {
        tx.cache
    }
}
