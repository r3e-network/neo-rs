//! Thin transactional wrapper over `StoreCache` for explicit commit semantics.

use super::StoreCache;
use super::data_cache::{DataCacheConfig, DataCacheResult};
use super::providers::memory_store::MemoryStore;
use super::store::Store;
use std::sync::Arc;

/// Represents a write transaction over a store or snapshot.
pub struct StoreTransaction<S: Store = MemoryStore> {
    cache: StoreCache<S>,
}

impl<S> StoreTransaction<S>
where
    S: Store + 'static,
{
    /// Creates a transaction bound to the provided store.
    pub fn from_store(store: Arc<S>, read_only: bool) -> Self {
        Self {
            cache: StoreCache::new_from_store(store, read_only),
        }
    }
}

impl<S> StoreTransaction<S>
where
    S: Store + 'static,
{
    /// Creates a transaction backed by a snapshot.
    pub fn from_snapshot(snapshot: Arc<S::Snapshot>) -> Self {
        Self::from_snapshot_with_config(snapshot, DataCacheConfig::default())
    }

    /// Creates a transaction backed by a snapshot using a custom cache config.
    pub fn from_snapshot_with_config(snapshot: Arc<S::Snapshot>, config: DataCacheConfig) -> Self {
        Self {
            cache: StoreCache::new_from_snapshot_with_config(snapshot, config),
        }
    }
}

impl<S> StoreTransaction<S>
where
    S: Store + 'static,
{
    /// Mutable access to the underlying cache for staged mutations.
    pub fn cache_mut(&mut self) -> &mut StoreCache<S> {
        &mut self.cache
    }

    /// Immutable access to the underlying cache.
    pub fn cache(&self) -> &StoreCache<S> {
        &self.cache
    }

    /// Commits the transaction.
    pub fn commit(mut self) -> DataCacheResult {
        self.cache.try_commit()
    }
}

impl<S> From<StoreTransaction<S>> for StoreCache<S>
where
    S: Store + 'static,
{
    fn from(tx: StoreTransaction<S>) -> Self {
        tx.cache
    }
}
