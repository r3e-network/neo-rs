//! [`PoolIndex`] - the internal BTreeMap-backed priority queue used by
//! [`crate::MemoryPool`].

use crate::pool_item::PoolItem;
use neo_primitives::UInt256;
use std::collections::BTreeSet;

/// BTreeSet of [`PoolItem`] ordered by [`PoolItem::compare_to`]. The raw set is
/// ascending (lowest priority first); public snapshots reverse it for C#'s
/// highest-priority-first mempool views.
#[derive(Debug, Default, Clone)]
pub struct PoolIndex {
    /// Set of pool items.
    pub items: BTreeSet<PoolItem>,
    /// Hash-to-item secondary index for O(1) lookup.
    pub hashes: std::collections::HashMap<UInt256, PoolItem>,
}

impl PoolIndex {
    /// Constructs an empty `PoolIndex`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a `PoolIndex` with the given initial capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: BTreeSet::new(),
            hashes: std::collections::HashMap::with_capacity(capacity),
        }
    }

    /// Returns the number of items in the index.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Reserves capacity for additional items.
    pub fn reserve(&mut self, additional: usize) {
        self.hashes.reserve(additional);
    }

    /// Inserts a pool item, returning the previous one with the same
    /// transaction hash if present.
    pub fn insert(&mut self, item: PoolItem) -> Option<PoolItem> {
        let hash = item.hash();
        let prev = self.hashes.insert(hash, item.clone());
        if let Some(ref p) = prev {
            self.items.remove(p);
        }
        self.items.insert(item);
        prev
    }

    /// Removes the item with the given transaction hash, returning
    /// it if present.
    pub fn remove(&mut self, hash: &UInt256) -> Option<PoolItem> {
        let item = self.hashes.remove(hash)?;
        self.items.remove(&item);
        Some(item)
    }

    /// Returns the item with the given transaction hash, if present.
    pub fn get(&self, hash: &UInt256) -> Option<&PoolItem> {
        self.hashes.get(hash)
    }

    /// Returns whether the index contains an item with the given hash.
    pub fn contains(&self, hash: &UInt256) -> bool {
        self.hashes.contains_key(hash)
    }

    /// Returns an iterator over the items in raw set order (lowest priority
    /// first). Use [`Self::to_sorted_vec`] for the public mempool order.
    pub fn iter(&self) -> std::collections::btree_set::Iter<'_, PoolItem> {
        self.items.iter()
    }

    /// Returns a vector of all items in priority order (highest first).
    pub fn to_sorted_vec(&self) -> Vec<PoolItem> {
        self.items.iter().rev().cloned().collect()
    }
}
