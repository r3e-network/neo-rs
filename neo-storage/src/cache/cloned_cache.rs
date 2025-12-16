//! Cloned cache for copy-on-write semantics.
//!
//! Provides a lightweight wrapper around [`DataCache`] for isolated modifications.

use super::data_cache::DataCache;

/// Lightweight wrapper that provides a writable clone of an existing [`DataCache`].
///
/// The original cache remains untouched; changes are isolated to this clone.
/// Changes must be committed explicitly back to the underlying store by consumers
/// if desired.
///
/// # Use Cases
///
/// - Transaction verification with isolated state changes
/// - Speculative execution before commit
/// - Read-only queries with temporary modifications
///
/// # Example
///
/// ```rust,ignore
/// use neo_storage::cache::{DataCache, ClonedCache};
/// use neo_storage::types::{StorageKey, StorageItem};
///
/// let original = DataCache::new(false);
/// original.add(StorageKey::new(-1, vec![0x01]), StorageItem::new(vec![0xAA]));
///
/// // Create isolated clone
/// let mut cloned = ClonedCache::new(&original);
/// cloned.cache().delete(&StorageKey::new(-1, vec![0x01]));
///
/// // Original is unchanged
/// assert!(original.contains(&StorageKey::new(-1, vec![0x01])));
/// ```
#[derive(Debug, Clone)]
pub struct ClonedCache {
    inner: DataCache,
}

impl ClonedCache {
    /// Creates a cloned cache from an existing [`DataCache`].
    ///
    /// The clone inherits all entries from the original cache but
    /// modifications are isolated.
    pub fn new(cache: &DataCache) -> Self {
        Self {
            inner: cache.clone(),
        }
    }

    /// Borrows the cloned cache mutably.
    ///
    /// Returns a mutable reference to the inner cache for modifications.
    pub fn cache(&mut self) -> &mut DataCache {
        &mut self.inner
    }

    /// Borrows the cloned cache immutably.
    pub fn cache_ref(&self) -> &DataCache {
        &self.inner
    }

    /// Consumes the wrapper and returns the inner cache.
    ///
    /// Use this to transfer ownership of the modified cache.
    pub fn into_inner(self) -> DataCache {
        self.inner
    }

    /// Returns the number of items in the cloned cache.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether the cloned cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{StorageItem, StorageKey};

    #[test]
    fn test_cloned_cache_new() {
        let original = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        original.add(key.clone(), StorageItem::new(vec![0xAA]));

        let cloned = ClonedCache::new(&original);
        assert!(cloned.cache_ref().contains(&key));
    }

    #[test]
    fn test_cloned_cache_isolation() {
        let original = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        original.add(key.clone(), StorageItem::new(vec![0xAA]));

        let mut cloned = ClonedCache::new(&original);
        cloned.cache().delete(&key);

        // Original unchanged
        assert!(original.contains(&key));
        // Clone modified
        assert!(!cloned.cache_ref().contains(&key));
    }

    #[test]
    fn test_cloned_cache_add_to_clone() {
        let original = DataCache::new(false);
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        original.add(key1.clone(), StorageItem::new(vec![0xAA]));

        let mut cloned = ClonedCache::new(&original);
        cloned
            .cache()
            .add(key2.clone(), StorageItem::new(vec![0xBB]));

        // Original has only key1
        assert!(original.contains(&key1));
        assert!(!original.contains(&key2));

        // Clone has both
        assert!(cloned.cache_ref().contains(&key1));
        assert!(cloned.cache_ref().contains(&key2));
    }

    #[test]
    fn test_cloned_cache_update_in_clone() {
        let original = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        original.add(key.clone(), StorageItem::new(vec![0xAA]));

        let mut cloned = ClonedCache::new(&original);
        cloned
            .cache()
            .update(key.clone(), StorageItem::new(vec![0xBB]));

        // Original has original value
        assert_eq!(original.try_get(&key).unwrap().value(), &[0xAA]);

        // Clone has updated value
        assert_eq!(cloned.cache_ref().try_get(&key).unwrap().value(), &[0xBB]);
    }

    #[test]
    fn test_cloned_cache_into_inner() {
        let original = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        original.add(key.clone(), StorageItem::new(vec![0xAA]));

        let mut cloned = ClonedCache::new(&original);
        cloned.cache().add(
            StorageKey::new(-1, vec![0x02]),
            StorageItem::new(vec![0xBB]),
        );

        let inner = cloned.into_inner();
        assert_eq!(inner.len(), 2);
    }

    #[test]
    fn test_cloned_cache_len_and_is_empty() {
        let original = DataCache::new(false);
        let cloned = ClonedCache::new(&original);

        assert!(cloned.is_empty());
        assert_eq!(cloned.len(), 0);

        let mut cloned = ClonedCache::new(&original);
        cloned.cache().add(
            StorageKey::new(-1, vec![0x01]),
            StorageItem::new(vec![0xAA]),
        );

        assert!(!cloned.is_empty());
        assert_eq!(cloned.len(), 1);
    }

    #[test]
    fn test_cloned_cache_clone() {
        let original = DataCache::new(false);
        let key = StorageKey::new(-1, vec![0x01]);
        original.add(key.clone(), StorageItem::new(vec![0xAA]));

        let cloned1 = ClonedCache::new(&original);
        let cloned2 = cloned1.clone();

        assert!(cloned1.cache_ref().contains(&key));
        assert!(cloned2.cache_ref().contains(&key));
    }

    #[test]
    fn test_cloned_cache_debug() {
        let original = DataCache::new(false);
        let cloned = ClonedCache::new(&original);

        let debug = format!("{:?}", cloned);
        assert!(debug.contains("ClonedCache"));
    }

    #[test]
    fn test_cloned_cache_multiple_operations() {
        let original = DataCache::new(false);
        let key1 = StorageKey::new(-1, vec![0x01]);
        let key2 = StorageKey::new(-1, vec![0x02]);
        let key3 = StorageKey::new(-1, vec![0x03]);

        original.add(key1.clone(), StorageItem::new(vec![0x11]));
        original.add(key2.clone(), StorageItem::new(vec![0x22]));

        let mut cloned = ClonedCache::new(&original);

        // Delete one
        cloned.cache().delete(&key1);
        // Update one
        cloned
            .cache()
            .update(key2.clone(), StorageItem::new(vec![0xBB]));
        // Add one
        cloned
            .cache()
            .add(key3.clone(), StorageItem::new(vec![0x33]));

        // Verify clone state
        assert!(!cloned.cache_ref().contains(&key1));
        assert_eq!(cloned.cache_ref().try_get(&key2).unwrap().value(), &[0xBB]);
        assert!(cloned.cache_ref().contains(&key3));

        // Verify original unchanged
        assert!(original.contains(&key1));
        assert_eq!(original.try_get(&key2).unwrap().value(), &[0x22]);
        assert!(!original.contains(&key3));
    }
}
