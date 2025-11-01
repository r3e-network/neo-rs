//! RelayCache - aligns with C# Neo.IO.Caching.RelayCache

use super::fifo_cache::FIFOCache;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// Trait representing inventory payloads that expose a hash suitable for use as a cache key.
pub trait InventoryHash<TKey>
where
    TKey: Clone + Eq + Hash,
{
    /// Returns the hash associated with the inventory item (matches C# `IInventory.Hash`).
    fn inventory_hash(&self) -> &TKey;
}

/// FIFO cache specialising in inventory payloads, keyed by their hash.
pub struct RelayCache<TKey, TInventory>
where
    TKey: Clone + Eq + Hash,
    TInventory: InventoryHash<TKey> + Clone,
{
    inner: FIFOCache<TKey, TInventory>,
}

impl<TKey, TInventory> RelayCache<TKey, TInventory>
where
    TKey: Clone + Eq + Hash,
    TInventory: InventoryHash<TKey> + Clone,
{
    /// Creates a new relay cache with the specified capacity.
    pub fn new(max_capacity: usize) -> Self {
        Self {
            inner: FIFOCache::new(max_capacity, |item: &TInventory| {
                item.inventory_hash().clone()
            }),
        }
    }
}

impl<TKey, TInventory> Deref for RelayCache<TKey, TInventory>
where
    TKey: Clone + Eq + Hash,
    TInventory: InventoryHash<TKey> + Clone,
{
    type Target = FIFOCache<TKey, TInventory>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<TKey, TInventory> DerefMut for RelayCache<TKey, TInventory>
where
    TKey: Clone + Eq + Hash,
    TInventory: InventoryHash<TKey> + Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
