//! `IndexedQueue` - matches C# Neo.IO.Caching.IndexedQueue exactly

use crate::{IoError, IoResult};
use std::collections::VecDeque;

/// Represents a queue with indexed access to the items (matches C# `IndexedQueue<T>`).
#[derive(Debug, Clone)]
pub struct IndexedQueue<T> {
    items: VecDeque<T>,
}

impl<T> IndexedQueue<T> {
    const DEFAULT_CAPACITY: usize = 16;
    const GROWTH_FACTOR: usize = 2;
    const TRIM_THRESHOLD: f32 = 0.9;

    /// Creates a queue with the default capacity.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Creates a queue with the specified capacity.
    ///
    /// # Arguments
    /// * `capacity` - The initial capacity. If zero, uses `DEFAULT_CAPACITY` instead.
    ///
    /// # Note
    /// Zero capacity is handled gracefully by using the default capacity.
    /// This prevents panics from configuration-driven capacity values.
    pub fn with_capacity(capacity: usize) -> Self {
        let effective_capacity = if capacity == 0 {
            #[cfg(feature = "tracing")]
            tracing::warn!(
                "IndexedQueue created with zero capacity, using default: {}",
                Self::DEFAULT_CAPACITY
            );
            #[cfg(not(feature = "tracing"))]
            eprintln!(
                "[WARN] IndexedQueue created with zero capacity, using default: {}",
                Self::DEFAULT_CAPACITY
            );
            Self::DEFAULT_CAPACITY
        } else {
            capacity
        };
        Self {
            items: VecDeque::with_capacity(effective_capacity),
        }
    }

    /// Creates a queue with the specified capacity, returning an error if capacity is zero.
    ///
    /// # Errors
    /// Returns an error if capacity is zero.
    pub fn try_with_capacity(capacity: usize) -> Result<Self, &'static str> {
        if capacity == 0 {
            return Err("capacity must be greater than zero");
        }
        Ok(Self {
            items: VecDeque::with_capacity(capacity),
        })
    }

    /// Gets the number of items in the queue (C# Count property).
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Indicates whether the queue is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Gets the value at the index (C# this[int index]).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    /// Gets a mutable reference to the value at the index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.items.get_mut(index)
    }

    /// Inserts an item at the rear of the queue (C# Enqueue).
    pub fn enqueue(&mut self, item: T) {
        if self.items.len() == self.items.capacity() {
            self.grow();
        }
        self.items.push_back(item);
    }

    /// Provides access to the item at the front of the queue without dequeuing it (C# Peek).
    ///
    /// # Panics
    /// Panics if the queue is empty. Use `try_peek()` for a non-panicking alternative.
    ///
    /// # Deprecated
    /// This method panics on empty queue. Prefer `try_peek()` which returns `Option<&T>`.
    #[deprecated(
        since = "0.7.1",
        note = "Use try_peek() instead. This method panics on empty queue."
    )]
    #[must_use]
    pub fn peek(&self) -> &T {
        self.items.front().expect("queue is empty")
    }

    /// Attempts to return an item from the front of the queue without removing it (C# `TryPeek`).
    #[must_use]
    pub fn try_peek(&self) -> Option<&T> {
        self.items.front()
    }

    /// Removes an item from the front of the queue, returning it (C# Dequeue).
    ///
    /// # Panics
    /// Panics if the queue is empty. Use `try_dequeue()` for a non-panicking alternative.
    ///
    /// # Deprecated
    /// This method panics on empty queue. Prefer `try_dequeue()` which returns `Option<T>`.
    #[deprecated(
        since = "0.7.1",
        note = "Use try_dequeue() instead. This method panics on empty queue."
    )]
    pub fn dequeue(&mut self) -> T {
        self.items.pop_front().expect("queue is empty")
    }

    /// Attempts to remove an item from the front of the queue (C# `TryDequeue`).
    pub fn try_dequeue(&mut self) -> Option<T> {
        self.items.pop_front()
    }

    /// Clears the items from the queue (C# Clear).
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Trims the extra capacity that isn't being used (C# `TrimExcess`).
    pub fn trim_excess(&mut self) {
        if self.items.is_empty() {
            self.items = VecDeque::with_capacity(Self::DEFAULT_CAPACITY);
            return;
        }
        let capacity = self.items.capacity() as f32;
        let count = self.items.len() as f32;
        if capacity * Self::TRIM_THRESHOLD >= count {
            let mut new_items = VecDeque::with_capacity(self.items.len());
            new_items.extend(self.items.drain(..));
            self.items = new_items;
        }
    }

    /// Copy the queue's items to a destination slice (C# `CopyTo`).
    pub fn copy_to(&self, destination: &mut [T], array_index: usize) -> IoResult<()>
    where
        T: Clone,
    {
        if array_index > destination.len() {
            return Err(IoError::invalid_data("array_index out of range"));
        }
        if destination.len() - array_index < self.items.len() {
            return Err(IoError::invalid_data(format!(
                "destination slice does not have sufficient space: {} remaining, {} required",
                destination.len() - array_index,
                self.items.len()
            )));
        }
        for (offset, item) in self.items.iter().cloned().enumerate() {
            destination[array_index + offset] = item;
        }

        Ok(())
    }

    /// Returns an array of the items in the queue (C# `ToArray`).
    #[must_use]
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.items.iter().cloned().collect()
    }

    /// Returns an iterator over the queue (C# `GetEnumerator`).
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.items.iter()
    }

    /// Returns the last item if present (matches C# Last property usage).
    #[must_use]
    pub fn last(&self) -> Option<&T> {
        self.items.back()
    }

    fn grow(&mut self) {
        let new_capacity = (self.items.capacity().max(1)) * Self::GROWTH_FACTOR;
        let mut new_items = VecDeque::with_capacity(new_capacity);
        new_items.extend(self.items.drain(..));
        self.items = new_items;
    }
}

impl<T> IntoIterator for IndexedQueue<T> {
    type Item = T;
    type IntoIter = std::collections::vec_deque::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<T> Default for IndexedQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for IndexedQueue<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            items: VecDeque::from_iter(iter),
        }
    }
}

impl<T> IndexedQueue<T> {
    /// Creates a queue filled with the specified items (C# constructor from `IEnumerable`).
    pub fn from_iterable<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        iter.into_iter().collect()
    }
}
