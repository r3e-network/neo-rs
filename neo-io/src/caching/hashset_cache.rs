//! `HashSetCache` - faithful port of Neo.IO.Caching.HashSetCache

use super::ordered_cache::OrderedCache;
use crate::IoResult;
use std::hash::Hash;

/// A cache that uses a hash set to store items (matches C# `HashSetCache<T>`).
pub struct HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    capacity: usize,
    items: OrderedCache<T, ()>,
}

impl<T> HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    const DEFAULT_CAPACITY: usize = 1024;

    /// Initializes a new instance with the given maximum capacity.
    ///
    /// # Arguments
    /// * `capacity` - The maximum capacity. If zero, uses `DEFAULT_CAPACITY` instead.
    ///
    /// # Note
    /// Zero capacity is handled gracefully by using the default capacity.
    /// This prevents panics from configuration-driven capacity values.
    pub fn new(capacity: usize) -> Self {
        let effective_capacity = if capacity == 0 {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                "HashSetCache created with zero capacity, using default: {}",
                Self::DEFAULT_CAPACITY
            );
            #[cfg(not(feature = "tracing"))]
            eprintln!(
                "[WARN] HashSetCache created with zero capacity, using default: {}",
                Self::DEFAULT_CAPACITY
            );
            Self::DEFAULT_CAPACITY
        } else {
            capacity
        };

        Self {
            capacity: effective_capacity,
            items: OrderedCache::new(effective_capacity),
        }
    }

    /// Initializes a new instance with the given maximum capacity, returning an error if zero.
    ///
    /// # Errors
    /// Returns an error if capacity is zero.
    pub fn try_new(capacity: usize) -> Result<Self, &'static str> {
        if capacity == 0 {
            return Err("capacity must be greater than zero");
        }

        Ok(Self {
            capacity,
            items: OrderedCache::new(capacity),
        })
    }

    /// Number of items currently in the cache (C# `Count`).
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Attempts to add an item; evicts the oldest when the capacity is exceeded (C# `TryAdd`).
    pub fn try_add(&mut self, item: T) -> bool {
        let inserted = !self.items.contains(&item);

        self.items.resize(self.capacity);
        if inserted {
            self.items.insert_if_absent(item, ());
        }
        inserted
    }

    /// Updates the maximum capacity. Existing overflow is trimmed on the next insertion attempt.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    /// Checks whether the cache already contains the item (C# `Contains`).
    #[inline]
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    /// Clears all items (C# `Clear`).
    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Removes a collection of items from the cache (C# `ExceptWith`).
    pub fn except_with<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        for item in items {
            self.remove(&item);
        }
    }

    /// Adds an item ignoring the return flag (C# `ICollection<T>.Add`).
    #[inline]
    pub fn add(&mut self, item: T) {
        let _ = self.try_add(item);
    }

    /// Removes an item from the cache (C# `Remove`).
    #[inline]
    pub fn remove(&mut self, item: &T) -> bool {
        self.items.remove(item)
    }

    /// Copies the elements into the destination slice starting at `start_index` (C# `CopyTo`).
    pub fn copy_to(&self, destination: &mut [T], start_index: usize) -> IoResult<()> {
        self.items.copy_keys_to(destination, start_index)
    }

    /// Returns an iterator over the cached values (C# `GetEnumerator`).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.keys()
    }
}

impl<T> IntoIterator for HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.keys().cloned().collect::<Vec<_>>().into_iter()
    }
}
