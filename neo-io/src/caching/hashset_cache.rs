//! `HashSetCache` - faithful port of Neo.IO.Caching.HashSetCache

use super::cache_entries::check_copy_range;
use crate::IoResult;
use indexmap::{IndexSet, set::IntoIter as IndexSetIntoIter};
use std::hash::Hash;

/// A cache that uses a hash set to store items (matches C# `HashSetCache<T>`).
pub struct HashSetCache<T>
where
    T: Eq + Hash,
{
    capacity: usize,
    items: Option<IndexSet<T>>,
}

impl<T> HashSetCache<T>
where
    T: Eq + Hash,
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
            items: Self::new_items(effective_capacity),
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
            items: Self::new_items(capacity),
        })
    }

    fn new_items(capacity: usize) -> Option<IndexSet<T>> {
        (capacity > 0).then(|| IndexSet::with_capacity(capacity))
    }

    fn ensure_backing_capacity(&mut self) -> Option<&mut IndexSet<T>> {
        (self.capacity > 0).then(|| {
            self.items
                .get_or_insert_with(|| IndexSet::with_capacity(self.capacity))
        })
    }

    /// Number of items currently in the cache (C# `Count`).
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.as_ref().map_or(0, IndexSet::len)
    }

    /// Attempts to add an item; evicts the oldest when the capacity is exceeded (C# `TryAdd`).
    pub fn try_add(&mut self, item: T) -> bool {
        let inserted = !self.contains(&item);

        self.trim_to_capacity();
        if !inserted || self.capacity == 0 {
            return inserted;
        }

        let capacity = self.capacity;
        if let Some(items) = self.ensure_backing_capacity() {
            items.insert(item);
            while items.len() > capacity {
                items.shift_remove_index(0);
            }
        }

        inserted
    }

    fn trim_to_capacity(&mut self) {
        if self.capacity == 0 {
            self.items = None;
            return;
        }

        let capacity = self.capacity;
        if let Some(items) = self.ensure_backing_capacity() {
            while items.len() > capacity {
                items.shift_remove_index(0);
            }
        }
    }

    /// Updates the maximum capacity. Existing overflow is trimmed on the next insertion attempt.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    /// Checks whether the cache already contains the item (C# `Contains`).
    #[inline]
    pub fn contains(&self, item: &T) -> bool {
        self.items
            .as_ref()
            .is_some_and(|items| items.contains(item))
    }

    /// Clears all items (C# `Clear`).
    #[inline]
    pub fn clear(&mut self) {
        if let Some(items) = self.items.as_mut() {
            items.clear();
        }
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
        self.items
            .as_mut()
            .is_some_and(|items| items.shift_remove(item))
    }

    /// Copies the elements into the destination slice starting at `start_index` (C# `CopyTo`).
    pub fn copy_to(&self, destination: &mut [T], start_index: usize) -> IoResult<()>
    where
        T: Clone,
    {
        check_copy_range("copy_to", start_index, self.count(), destination.len())?;
        for (offset, item) in self.iter().cloned().enumerate() {
            destination[start_index + offset] = item;
        }
        Ok(())
    }

    /// Returns an iterator over the cached values (C# `GetEnumerator`).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter().flat_map(|items| items.iter())
    }
}

/// Consuming iterator over `HashSetCache` values in FIFO order.
pub struct HashSetCacheIntoIter<T>
where
    T: Eq + Hash,
{
    inner: Option<IndexSetIntoIter<T>>,
}

impl<T> Iterator for HashSetCacheIntoIter<T>
where
    T: Eq + Hash,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.as_mut().and_then(Iterator::next)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner
            .as_ref()
            .map_or((0, Some(0)), Iterator::size_hint)
    }
}

impl<T> ExactSizeIterator for HashSetCacheIntoIter<T> where T: Eq + Hash {}

impl<T> IntoIterator for HashSetCache<T>
where
    T: Eq + Hash,
{
    type Item = T;
    type IntoIter = HashSetCacheIntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        HashSetCacheIntoIter {
            inner: self.items.map(IntoIterator::into_iter),
        }
    }
}
