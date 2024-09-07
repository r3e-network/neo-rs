
use std::collections::VecDeque;

/// Represents a queue with indexed access to the items
pub struct IndexedQueue<T> {
    queue: VecDeque<T>,
}

impl<T> IndexedQueue<T> {
    const DEFAULT_CAPACITY: usize = 16;
    const GROWTH_FACTOR: usize = 2;
    const TRIM_THRESHOLD: f32 = 0.9;

    /// Creates a queue with the default capacity
    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    /// Creates a queue with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity == 0 {
            panic!("The capacity must be greater than zero.");
        }
        Self {
            queue: VecDeque::with_capacity(capacity),
        }
    }

    /// Creates a queue filled with the specified items
    pub fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            queue: VecDeque::from_iter(iter),
        }
    }

    /// Gets the value at the index
    pub fn get(&self, index: usize) -> Option<&T> {
        self.queue.get(index)
    }

    /// Gets a mutable reference to the value at the index
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.queue.get_mut(index)
    }

    /// Inserts an item at the rear of the queue
    pub fn enqueue(&mut self, item: T) {
        if self.queue.len() == self.queue.capacity() {
            let new_capacity = self.queue.capacity() * Self::GROWTH_FACTOR;
            self.queue.reserve(new_capacity - self.queue.capacity());
        }
        self.queue.push_back(item);
    }

    /// Provides access to the item at the front of the queue without dequeuing it
    pub fn peek(&self) -> Option<&T> {
        self.queue.front()
    }

    /// Removes an item from the front of the queue, returning it
    pub fn dequeue(&mut self) -> Option<T> {
        self.queue.pop_front()
    }

    /// Clears the items from the queue
    pub fn clear(&mut self) {
        self.queue.clear();
    }

    /// Trims the extra array space that isn't being used.
    pub fn trim_excess(&mut self) {
        if self.queue.is_empty() {
            self.queue = VecDeque::with_capacity(Self::DEFAULT_CAPACITY);
        } else if self.queue.capacity() as f32 * Self::TRIM_THRESHOLD >= self.queue.len() as f32 {
            self.queue.shrink_to_fit();
        }
    }

    /// Returns a vector of the items in the queue
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.queue.iter().cloned().collect()
    }

    /// Returns the number of items in the queue
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

impl<T> IntoIterator for IndexedQueue<T> {
    type Item = T;
    type IntoIter = std::collections::vec_deque::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.queue.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a IndexedQueue<T> {
    type Item = &'a T;
    type IntoIter = std::collections::vec_deque::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.queue.iter()
    }
}

impl<T> FromIterator<T> for IndexedQueue<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_iter(iter)
    }
}
