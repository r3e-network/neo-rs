//! HashSetCache - faithful port of Neo.IO.Caching.HashSetCache

use super::keyed_collection_slim::KeyedCollectionSlim;
use std::hash::Hash;

/// A cache that uses a hash set to store items (matches C# `HashSetCache<T>`).
pub struct HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    capacity: usize,
    items: KeyedCollectionSlim<T, T>,
}

impl<T> HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    const DEFAULT_CAPACITY: usize = 1024;

    /// Initializes a new instance with the given maximum capacity.
    ///
    /// # Arguments
    /// * `capacity` - The maximum capacity. If zero, uses DEFAULT_CAPACITY instead.
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

        let initial_capacity = effective_capacity.min(4096);
        Self {
            capacity: effective_capacity,
            items: KeyedCollectionSlim::with_selector(initial_capacity, |item: &T| item.clone()),
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

        let initial_capacity = capacity.min(4096);
        Ok(Self {
            capacity,
            items: KeyedCollectionSlim::with_selector(initial_capacity, |item: &T| item.clone()),
        })
    }

    /// Number of items currently in the cache (C# `Count`).
    #[inline]
    pub fn count(&self) -> usize {
        self.items.count()
    }

    /// Attempts to add an item; evicts the oldest when the capacity is exceeded (C# `TryAdd`).
    pub fn try_add(&mut self, item: T) -> bool {
        if !self.items.try_add(item) {
            return false;
        }
        if self.items.count() > self.capacity {
            self.items.remove_first();
        }
        true
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
            self.items.remove(&item);
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
    pub fn copy_to(&self, destination: &mut [T], start_index: usize) {
        if start_index > destination.len() {
            panic!("start_index exceeds destination length");
        }

        let count = self.count();
        if start_index + count > destination.len() {
            panic!(
                "start_index ({}) + count ({}) > destination length ({})",
                start_index,
                count,
                destination.len()
            );
        }

        for (offset, value) in self.items.iter().cloned().enumerate() {
            destination[start_index + offset] = value;
        }
    }

    /// Returns an iterator over the cached values (C# `GetEnumerator`).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }
}

impl<T> IntoIterator for HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter().cloned().collect::<Vec<_>>().into_iter()
    }
}
