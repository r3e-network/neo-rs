//! `HashSetCache` - faithful port of Neo.IO.Caching.HashSetCache

use crate::{IoError, IoResult};
use lru::LruCache;
use std::hash::Hash;
use std::num::NonZeroUsize;

/// A cache that uses a hash set to store items (matches C# `HashSetCache<T>`).
pub struct HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    capacity: usize,
    items: Option<LruCache<T, ()>>,
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
            items: NonZeroUsize::new(effective_capacity).map(LruCache::new),
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
            items: NonZeroUsize::new(capacity).map(LruCache::new),
        })
    }

    /// Number of items currently in the cache (C# `Count`).
    #[inline]
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.as_ref().map_or(0, LruCache::len)
    }

    /// Attempts to add an item; evicts the oldest when the capacity is exceeded (C# `TryAdd`).
    pub fn try_add(&mut self, item: T) -> bool {
        let inserted = !self.contains(&item);
        if self.capacity == 0 {
            self.items = None;
            return inserted;
        }

        self.ensure_cache_capacity();
        if inserted {
            self.items
                .as_mut()
                .expect("positive capacity creates backing cache")
                .put(item, ());
        }
        self.trim_to_capacity();
        inserted
    }

    /// Updates the maximum capacity. Existing overflow is trimmed on the next insertion attempt.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
    }

    fn ensure_cache_capacity(&mut self) {
        let Some(capacity) = NonZeroUsize::new(self.capacity) else {
            self.items = None;
            return;
        };

        match self.items.as_mut() {
            Some(items) if items.cap() != capacity => items.resize(capacity),
            Some(_) => {}
            None => self.items = Some(LruCache::new(capacity)),
        }
    }

    fn trim_to_capacity(&mut self) {
        if self.capacity == 0 {
            self.items = None;
            return;
        }

        self.ensure_cache_capacity();
        if let Some(items) = self.items.as_mut() {
            while items.len() > self.capacity {
                items.pop_lru();
            }
        }
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
            .is_some_and(|items| items.pop(item).is_some())
    }

    /// Copies the elements into the destination slice starting at `start_index` (C# `CopyTo`).
    pub fn copy_to(&self, destination: &mut [T], start_index: usize) -> IoResult<()> {
        if start_index > destination.len() {
            return Err(IoError::invalid_data(
                "start_index exceeds destination length",
            ));
        }

        let count = self.count();
        let end_index = start_index
            .checked_add(count)
            .ok_or_else(|| IoError::invalid_data("start_index + count overflows"))?;
        if end_index > destination.len() {
            return Err(IoError::invalid_data(format!(
                "start_index ({}) + count ({}) > destination length ({})",
                start_index,
                count,
                destination.len()
            )));
        }

        if let Some(items) = self.items.as_ref() {
            for (offset, value) in items.iter().rev().map(|(item, _)| item.clone()).enumerate() {
                destination[start_index + offset] = value;
            }
        }

        Ok(())
    }

    /// Returns an iterator over the cached values (C# `GetEnumerator`).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items
            .as_ref()
            .into_iter()
            .flat_map(|items| items.iter().rev().map(|(item, _)| item))
    }
}

impl<T> IntoIterator for HashSetCache<T>
where
    T: Eq + Hash + Clone,
{
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .rev()
                    .map(|(item, _)| item.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
            .into_iter()
    }
}
