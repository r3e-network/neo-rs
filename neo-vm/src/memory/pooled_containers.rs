// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Reusable container utilities for reducing allocation overhead.
//!
//! This module provides specialized container types designed to eliminate heap
//! reallocations during typical transaction processing:
//! - `PooledVec`: Fixed-capacity vector that maintains allocated memory across clears
//! - `PooledHashMap`: Open-addressing hash map with pre-allocated buckets
//!
//! ## Performance Benefits
//!
//! Every transaction typically allocates:
//! - ~16 event notifications (Vec grows: 0→8→16 allocations)
//! - ~64 storage operations (HashMap insertions)
//! - Multiple temporary collections during script execution
//!
//! At 10k TPS → hundreds of thousands of allocations/sec → fragmentation + GC pressure.
//! These pooled containers maintain capacity, eliminating repeated malloc/free cycles.
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use neo_vm::memory::{PooledVec, PooledHashMap};
//!
//! // Create a pooled vector for notifications
//! let mut notifications = PooledVec::<String>::with_capacity(64);
//!
//! // First pass: allocates capacity 64
//! for i in 0..50 {
//!     notifications.push(format!("Event {}", i));
//! }
//! assert_eq!(notifications.len(), 50);
//!
//! // Second pass: ZERO allocations! Just resets length
//! notifications.clear_keep_capacity();
//! for i in 0..30 {
//!     notifications.push(format!("New Event {}", i));
//! }
//!
//! // PooledHashMap for storage operations
//! let mut storage_cache = PooledHashMap::<u64, Vec<u8>>::new(256);
//!
//! // Insert without bucket reallocations
//! storage_cache.insert_no_alloc(12345, vec![1, 2, 3]);
//! storage_cache.insert_no_alloc(67890, vec![4, 5, 6]);
//!
//! // Clear but keep all bucket slots available
//! storage_cache.clear_no_dealloc();
//! ```
//!
//! ## Design Tradeoffs
//!
//! ### PooledVec vs Vec
//!
//! - **Pro**: No reallocations up to max capacity
//! - **Con**: Cannot exceed capacity (panics instead of reallocating)
//! - **Best for**: Predictable-length collections (notifications, events)
//!
//! ### PooledHashMap vs HashMap
//!
//! - **Pro**: Fixed bucket array, no rehashing, cache-friendly layout
//! - **Con**: Lower collision tolerance (fixed size)
//! - **Best for**: Known key ranges (storage keys by prefix, syscall handlers)
//!
//! ## Thread Safety
//!
//! Both types are `!Sync` by design - intended for single-threaded transaction execution.
//! For cross-thread sharing, wrap in `Arc<Mutex<_>>` or use message passing.

use std::hash::{Hash, Hasher};
use std::ops::{Deref, DerefMut};

/// FNV-1a hash function for fast integer hashing.
///
/// FNV-1a provides excellent distribution characteristics and is extremely fast.
/// Uses the prime offset basis 14695981039346656037 and prime multiplier 1099511628211.
///
/// # Arguments
///
/// * `value` - The u64 value to hash
///
/// # Returns
///
/// A hashed u64 value with good bit distribution
#[inline]
fn fnv1a_hash_u64(value: u64) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
    const FNV_PRIME: u64 = 1099511628211;

    let hash = FNV_OFFSET_BASIS ^ value;
    hash.wrapping_mul(FNV_PRIME)
}

/// Hashes an arbitrary `Hash` key to a `u64` using the standard library's
/// writer, so `PooledHashMap` works with any key type (not just integers).
#[inline]
fn hash_key<K: Hash>(key: &K) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// Reusable vector that maintains capacity across clears.
///
/// Unlike `std::vec::Vec`, this type guarantees zero reallocations when the
/// number of elements stays within its configured maximum capacity. After
/// `clear_keep_capacity()` is called, the internal buffer remains allocated
/// and ready for immediate reuse.
///
/// ## Key Differences from Vec
///
/// | Behavior | Vec | PooledVec |
/// |----------|-----|-----------|
/// | push beyond capacity | reallocates (slow) | panics (fail-fast) |
/// | clear() | may deallocate | keeps capacity |
/// | predictable performance | ❌ | ✅ |
/// | max_elements enforced | ❌ | ✅ |
///
/// # Type Parameters
///
/// * `T` - Element type
///
/// # Examples
///
/// ```rust,no_run
/// use neo_vm::memory::PooledVec;
///
/// // Pre-allocate capacity for 100 events
/// let mut events = PooledVec::<&str>::with_capacity(100);
///
/// // First batch: allocates memory for 100 elements
/// events.push("Event 1");
/// events.push("Event 2");
/// // ... push up to 100 items
///
/// // Process events...
///
/// // Clear without freeing memory
/// events.clear_keep_capacity();
///
/// // Next batch: ZERO allocations!
/// events.push("Event 101");
/// events.push("Event 102");
/// ```
pub struct PooledVec<T> {
    inner: Vec<T>,
    max_capacity: usize,
}

impl<T> PooledVec<T> {
    /// Creates a new empty `PooledVec` with the specified capacity.
    ///
    /// The returned vector will never allocate more than `capacity` elements,
    /// even if pushed beyond initial size. Attempting to exceed capacity will
    /// panic at runtime.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Maximum number of elements (enforced at runtime)
    ///
    /// # Panics
    ///
    /// Panics if `capacity` exceeds system memory limits or is excessively large.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// // Capacity guaranteed: will never exceed 64 elements
    /// let mut pool = PooledVec::<i32>::with_capacity(64);
    ///
    /// // Safe: within capacity
    /// pool.push(42);
    ///
    /// // Panics: exceeds maximum capacity
    /// // for i in 0..100 { pool.push(i); } // Would panic at i=64
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Vec::with_capacity(capacity),
            max_capacity: capacity,
        }
    }

    /// Returns the current allocation capacity.
    ///
    /// This is preserved across [`Self::clear_keep_capacity`].
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    /// Creates a new empty `PooledVec` with default capacity (0).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut pool = PooledVec::<i32>::new();
    /// assert!(pool.is_empty());
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self {
            inner: Vec::new(),
            max_capacity: 0,
        }
    }

    /// Sets maximum capacity after construction.
    ///
    /// Allows dynamic adjustment of capacity constraints. Should be called before
    /// any pushes to enforce safety.
    ///
    /// # Arguments
    ///
    /// * `capacity` - New maximum element count
    pub fn set_max_capacity(&mut self, capacity: usize) {
        self.max_capacity = capacity;
    }

    /// Appends an element to the vector, enforcing capacity limits.
    ///
    /// Unlike `Vec::push`, this method panics if adding the element would exceed
    /// the maximum capacity. This fail-fast behavior prevents unexpected reallocations.
    ///
    /// # Arguments
    ///
    /// * `value` - Element to append
    ///
    /// # Panics
    ///
    /// Panics if `len() == max_capacity`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(2);
    /// vec.push(1);
    /// vec.push(2);
    /// assert_eq!(vec.len(), 2);
    ///
    /// // This will panic!
    /// // vec.push(3); // PANIC: capacity exceeded
    /// ```
    #[inline]
    pub fn push(&mut self, value: T) {
        if self.inner.len() >= self.max_capacity && self.max_capacity > 0 {
            panic!(
                "PooledVec capacity exceeded: len={} max={}",
                self.inner.len(),
                self.max_capacity
            );
        }
        self.inner.push(value);
    }

    /// Clears all elements while keeping allocated capacity.
    ///
    /// This is the preferred clear method as it:
    /// - Sets length to 0 without deallocating memory
    /// - Preserves the internal buffer's capacity
    /// - Guarantees next insertion doesn't trigger reallocation
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(64);
    /// for i in 0..50 {
    ///     vec.push(i);
    /// }
    ///
    // Zero-copy clear - capacity preserved!
    /// vec.clear_keep_capacity();
    /// assert_eq!(vec.len(), 0);
    /// assert_eq!(vec.capacity(), 64);
    /// ```
    #[inline]
    pub fn clear_keep_capacity(&mut self) {
        self.inner.clear();
    }

    /// Gets the number of elements in the vector.
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Checks if the vector is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Gets mutable reference to underlying Vec (unsafe usage).
    ///
    /// # Safety
    ///
    /// Caller must ensure modifications don't exceed `max_capacity`.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.inner
    }

    /// Gets immutable reference to underlying Vec.
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.inner
    }

    /// Tries to push an element, returning error if capacity exceeded.
    ///
    /// Unlike `push()`, this method returns `Err(T)` if the vector has reached
    /// maximum capacity, allowing graceful handling instead of panic.
    ///
    /// # Arguments
    ///
    /// * `item` - Element to try pushing
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Successfully pushed
    /// * `Err(T)` - Capacity exceeded, item returned unchanged
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledVec;
    ///
    /// let mut vec = PooledVec::<i32>::with_capacity(2);
    /// vec.try_push(1).unwrap();
    /// vec.try_push(2).unwrap();
    /// assert_eq!(vec.try_push(3), Err(3)); // Capacity exceeded
    /// ```
    #[inline]
    pub fn try_push(&mut self, item: T) -> Result<(), T> {
        if self.inner.len() >= self.max_capacity && self.max_capacity > 0 {
            Err(item)
        } else {
            self.inner.push(item);
            Ok(())
        }
    }

    /// Reserves capacity for minimum additional elements.
    ///
    /// Does nothing if capacity is already sufficient. If max_capacity is set
    /// and requested reservation would exceed it, sets max_capacity accordingly.
    ///
    /// # Arguments
    ///
    /// * `additional` - Minimum number of elements to reserve space for
    pub fn reserve(&mut self, additional: usize) {
        let required = self.inner.len().saturating_add(additional);

        if self.max_capacity > 0 && required > self.max_capacity {
            // Adjust max_capacity if needed (shouldn't happen in normal usage)
            self.max_capacity = required;
        }

        self.inner.reserve(additional);
    }
}

impl<T> Default for PooledVec<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Deref for PooledVec<T> {
    type Target = [T];

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for PooledVec<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: Clone> Clone for PooledVec<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            max_capacity: self.max_capacity,
        }
    }

    #[inline]
    fn clone_from(&mut self, other: &Self) {
        self.inner.clone_from(&other.inner);
        self.max_capacity = other.max_capacity;
    }
}

/// One slot of the fixed bucket array used by [`PooledHashMap`].
///
/// A tombstone records that a slot used to hold an entry. A linear probe that
/// walks into a tombstone keeps looking, exactly as it would for an occupied
/// slot, so removing an entry can never hide the rest of its collision chain.
/// Only an `Empty` slot terminates a probe.
#[derive(Clone, Debug)]
enum Bucket<K, V> {
    /// Never used, or cleared: this slot terminates a probe chain.
    Empty,
    /// Holds a live entry.
    Occupied(K, V),
    /// Held an entry that was removed: the probe chain continues past it.
    Tombstone,
}

impl<K, V> Bucket<K, V> {
    /// Returns the value when this bucket is occupied.
    #[inline]
    fn value(&self) -> Option<&V> {
        match self {
            Self::Occupied(_, value) => Some(value),
            Self::Empty | Self::Tombstone => None,
        }
    }
}

/// Outcome of a linear probe for one key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Probe {
    /// The key already occupies this bucket index.
    Found(usize),
    /// The key is absent; this bucket index can receive it.
    Vacant(usize),
    /// The key is absent and no bucket can receive it.
    Full,
}

/// Simple open-addressing hash map with fixed bucket array.
///
/// Unlike `std::collections::HashMap`, this type uses a fixed-size bucket array
/// that never reallocates. Keys are hashed and mapped directly to bucket slots
/// using open addressing (linear probing for collisions).
///
/// ## Characteristics
///
/// - **O(1)** average case for insert/lookup (when load factor < 0.7)
/// - **Zero allocations** after initial construction
/// - **No rehashing** - bucket count is fixed
/// - **Linear probing** resolves collisions
/// - **Tombstone deletion** - removal never hides a collision chain
///
/// ## Probe Termination
///
/// Every probe walks at most `capacity` buckets. A lookup therefore terminates
/// even when all buckets are occupied by other keys, and an insertion can find
/// and update an existing key in a completely full table before the capacity
/// check is applied.
///
/// ## Load Factor Management
///
/// Maintains high performance by limiting occupancy to 70% of bucket count.
/// When filled_count approaches capacity*0.7, consider growing the map.
///
/// # Type Parameters
///
/// * `K` - Key type (must implement `Eq` + `Hash`)
/// * `V` - Value type
///
/// # Examples
///
/// ```rust,no_run
/// use neo_vm::memory::PooledHashMap;
///
/// // Create map with 256 bucket slots
/// let mut cache = PooledHashMap::<u64, Vec<u8>>::new(256);
///
/// // Insert without allocating buckets
/// cache.insert_no_alloc(12345, vec![1, 2, 3]);
/// cache.insert_no_alloc(67890, vec![4, 5, 6]);
///
/// // Get value
/// if let Some(data) = cache.get_no_alloc(&12345) {
///     println!("Found {} bytes", data.len());
/// }
///
/// // Clear without deallocating buckets
/// cache.clear_no_dealloc();
/// assert_eq!(cache.len(), 0);
/// ```
pub struct PooledHashMap<K, V>
where
    K: Eq + Hash,
{
    buckets: Vec<Bucket<K, V>>,
    capacity: usize,
    filled_count: usize,
}

impl<K, V> PooledHashMap<K, V>
where
    K: Eq + Hash,
{
    /// Creates a new empty `PooledHashMap` with the specified capacity.
    ///
    /// The hash map initializes with exactly `capacity` bucket slots that never
    /// reallocate. For optimal performance, choose capacity as a power of two
    /// between 64 and 4096 depending on expected key count.
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of bucket slots (power of 2 recommended)
    ///
    /// # Performance Tip
    ///
    /// Recommended capacities based on expected entries:
    /// - ~16 entries: capacity 32
    /// - ~64 entries: capacity 128
    /// - ~256 entries: capacity 512
    /// - ~1024 entries: capacity 2048
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledHashMap;
    ///
    /// // Optimal for ~100 storage keys per contract
    /// let mut storage_cache = PooledHashMap::<[u8; 32], Vec<u8>>::new(256);
    /// ```
    pub fn new(capacity: usize) -> Self {
        // Round capacity up to nearest power of 2 for efficient masking
        let rounded_capacity = capacity.next_power_of_two();

        Self {
            buckets: (0..rounded_capacity).map(|_| Bucket::Empty).collect(),
            capacity: rounded_capacity,
            filled_count: 0,
        }
    }

    /// Creates a new empty `PooledHashMap` with default capacity (1).
    #[inline]
    pub fn with_capacity_default() -> Self {
        Self::new(1)
    }

    /// Inserts a key-value pair without reallocating buckets.
    ///
    /// Uses FNV-1a hash function for fast computation and linear probing for
    /// collision resolution.
    ///
    /// The probe searches for the key first and only then asks for a free
    /// bucket, so updating a key that is already present succeeds even when
    /// every bucket is occupied.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to insert
    /// * `value` - Value associated with key
    ///
    /// # Returns
    ///
    /// * `Some(old_value)` - Replaced existing value for this key
    /// * `None` - A new entry was inserted
    ///
    /// # Panics
    ///
    /// Panics when the key is absent and the table has no free bucket left
    /// (`filled_count == capacity` and no tombstone to reuse).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledHashMap;
    ///
    /// let mut map = PooledHashMap::<u32, String>::new(64);
    ///
    /// // First insert: adds new entry
    /// assert!(map.insert_no_alloc(1, "One".to_string()).is_none());
    ///
    /// // Second insert with same key: replaces value
    /// let old = map.insert_no_alloc(1, "ONE".to_string()).unwrap();
    /// assert_eq!(old, "One");
    /// ```
    pub fn insert_no_alloc(&mut self, key: K, value: V) -> Option<V> {
        match self.probe(&key) {
            Probe::Found(index) => {
                // Key exists: replace the value in place. This happens before
                // any capacity check, so a full table still accepts updates.
                let previous =
                    std::mem::replace(&mut self.buckets[index], Bucket::Occupied(key, value));
                match previous {
                    Bucket::Occupied(_, old_value) => Some(old_value),
                    // `Probe::Found` guarantees an occupied bucket.
                    Bucket::Empty | Bucket::Tombstone => None,
                }
            }
            Probe::Vacant(index) => {
                self.buckets[index] = Bucket::Occupied(key, value);
                self.filled_count += 1;
                None
            }
            Probe::Full => panic!(
                "PooledHashMap full: filled_count={} capacity={}",
                self.filled_count, self.capacity
            ),
        }
    }

    /// Walks the linear probe chain for `key` and stops after at most
    /// `capacity` buckets, so a missing key terminates on a completely
    /// occupied table.
    ///
    /// The first tombstone seen is remembered as a candidate insertion slot;
    /// it is only used when the key itself is absent.
    fn probe(&self, key: &K) -> Probe {
        let mask = self.capacity - 1;
        let hash = fnv1a_hash_u64(hash_key(key));
        let mut index = hash as usize & mask; // Fast modulo with power-of-2
        let mut first_tombstone: Option<usize> = None;

        for _ in 0..self.capacity {
            match &self.buckets[index] {
                Bucket::Occupied(existing_key, _) => {
                    if existing_key == key {
                        return Probe::Found(index);
                    }
                }
                Bucket::Tombstone => {
                    first_tombstone.get_or_insert(index);
                }
                Bucket::Empty => {
                    return Probe::Vacant(first_tombstone.unwrap_or(index));
                }
            }
            index = (index + 1) & mask;
        }

        // Every bucket was visited: no empty slot terminates the chain.
        match first_tombstone {
            Some(index) => Probe::Vacant(index),
            None => Probe::Full,
        }
    }

    /// Gets a value by key without reallocating.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to look up
    ///
    /// # Returns
    ///
    /// `Some(&V)` if key found, `None` otherwise
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledHashMap;
    ///
    /// let mut map = PooledHashMap::<u64, i32>::new(32);
    /// map.insert_no_alloc(42, 100);
    ///
    /// assert_eq!(map.get_no_alloc(&42), Some(&100));
    /// assert_eq!(map.get_no_alloc(&999), None);
    /// ```
    pub fn get_no_alloc(&self, key: &K) -> Option<&V> {
        match self.probe(key) {
            Probe::Found(index) => self.buckets[index].value(),
            // Absent, or the whole table was visited without a match.
            Probe::Vacant(_) | Probe::Full => None,
        }
    }

    /// Removes a key-value pair without reallocating buckets.
    ///
    /// Deletion leaves a tombstone so the rest of the collision chain stays
    /// reachable. Whenever the bucket ahead is already `Empty`, the tombstone
    /// is repaired back to `Empty` and the same check walks backwards over
    /// neighbouring tombstones, which keeps tombstones from accumulating.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to remove
    ///
    /// # Returns
    ///
    /// `Some(V)` if key existed, `None` otherwise
    pub fn remove_no_alloc(&mut self, key: &K) -> Option<V> {
        let Probe::Found(index) = self.probe(key) else {
            return None;
        };

        let removed = std::mem::replace(&mut self.buckets[index], Bucket::Tombstone);
        let value = match removed {
            Bucket::Occupied(_, value) => value,
            // `Probe::Found` guarantees an occupied bucket.
            Bucket::Empty | Bucket::Tombstone => return None,
        };
        self.filled_count -= 1;
        self.repair_tombstone(index);
        Some(value)
    }

    /// Turns a fresh tombstone back into an `Empty` bucket when nothing after
    /// it in the counter-clockwise direction needs it, then repeats the check
    /// for the previous bucket.
    ///
    /// A probe chain that would have terminated at one of the cleared
    /// tombstones terminates at the `Empty` bucket that now sits behind it
    /// instead, so no live entry becomes unreachable. The walk is bounded by
    /// `capacity`, and it stops immediately on a table without an empty slot.
    fn repair_tombstone(&mut self, start: usize) {
        let mask = self.capacity - 1;
        let mut index = start;

        for _ in 0..self.capacity {
            let next = (index + 1) & mask;
            if !matches!(self.buckets[next], Bucket::Empty) {
                return;
            }
            self.buckets[index] = Bucket::Empty;

            let previous = (index + self.capacity - 1) & mask;
            if !matches!(self.buckets[previous], Bucket::Tombstone) {
                return;
            }
            index = previous;
        }
    }

    /// Clears all entries without deallocating buckets.
    ///
    /// Resets all occupied slots to `None` while preserving the bucket array
    /// structure. O(n) where n is bucket capacity (not filled count).
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use neo_vm::memory::PooledHashMap;
    ///
    /// let mut map = PooledHashMap::<u32, u32>::new(64);
    /// map.insert_no_alloc(1, 100);
    /// map.insert_no_alloc(2, 200);
    /// assert_eq!(map.len(), 2);
    ///
    /// // Clear but keep 64 buckets available
    /// map.clear_no_dealloc();
    /// assert_eq!(map.len(), 0);
    /// ```
    #[inline]
    pub fn clear_no_dealloc(&mut self) {
        for slot in &mut self.buckets {
            *slot = Bucket::Empty;
        }
        self.filled_count = 0;
    }

    /// Gets the number of entries in the map.
    #[inline]
    pub fn len(&self) -> usize {
        self.filled_count
    }

    /// Checks if the map is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.filled_count == 0
    }

    /// Returns the total bucket capacity.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Calculates current load factor (0.0 to 1.0).
    ///
    /// # Returns
    ///
    /// Ratio of filled slots to total capacity
    #[inline]
    pub fn load_factor(&self) -> f64 {
        self.filled_count as f64 / self.capacity as f64
    }

    /// Checks if map is approaching capacity (>70% full).
    ///
    /// When load_factor > 0.7, lookup performance degrades due to collision chains.
    /// Consider creating a larger map and migrating entries.
    #[inline]
    pub fn is_nearly_full(&self) -> bool {
        self.filled_count > (self.capacity as f64 * 0.7) as usize
    }

    /// Bucket indices that currently hold an entry, in ascending order.
    #[cfg(test)]
    fn occupied_slots(&self) -> Vec<usize> {
        self.buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| matches!(bucket, Bucket::Occupied(..)))
            .map(|(index, _)| index)
            .collect()
    }

    /// Bucket indices that currently hold a tombstone, in ascending order.
    #[cfg(test)]
    fn tombstone_slots(&self) -> Vec<usize> {
        self.buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| matches!(bucket, Bucket::Tombstone))
            .map(|(index, _)| index)
            .collect()
    }
}

impl<K, V> Default for PooledHashMap<K, V>
where
    K: Eq + Hash,
{
    #[inline]
    fn default() -> Self {
        Self::new(64)
    }
}

impl<K, V> Clone for PooledHashMap<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Self {
            buckets: self.buckets.clone(),
            capacity: self.capacity,
            filled_count: self.filled_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Home bucket a key maps to for a fixed bucket count.
    fn home_slot(key: &u64, capacity: usize) -> usize {
        fnv1a_hash_u64(hash_key(key)) as usize & (capacity - 1)
    }

    /// Returns `count` distinct keys whose home bucket is `home`.
    fn keys_with_home(home: usize, capacity: usize, count: usize) -> Vec<u64> {
        (0u64..)
            .filter(|candidate| home_slot(candidate, capacity) == home)
            .take(count)
            .collect()
    }

    /// Returns a key whose home bucket is `home` but that is not in `occupied`.
    fn absent_key_with_home(home: usize, capacity: usize, occupied: &[u64]) -> u64 {
        (0u64..)
            .find(|candidate| {
                home_slot(candidate, capacity) == home && !occupied.contains(candidate)
            })
            .expect("a key outside the occupied set always exists")
    }

    /// Fills a map of `capacity` buckets with distinct keys.
    fn full_map(capacity: usize) -> (PooledHashMap<u64, u64>, Vec<u64>) {
        let mut map = PooledHashMap::<u64, u64>::new(capacity);
        let mut keys = Vec::new();
        let mut candidate = 0u64;
        while keys.len() < map.capacity() {
            if map.insert_no_alloc(candidate, candidate).is_none() {
                keys.push(candidate);
            }
            candidate += 1;
        }
        assert_eq!(map.len(), map.capacity());
        (map, keys)
    }

    #[test]
    fn test_pooled_vec_basic() {
        let mut vec = PooledVec::<i32>::with_capacity(10);

        assert!(vec.is_empty());
        assert_eq!(vec.len(), 0);

        for i in 0..10 {
            vec.push(i);
        }

        assert_eq!(vec.len(), 10);

        // Verify we can access elements
        for (i, &val) in vec.iter().enumerate() {
            assert_eq!(val, i as i32);
        }
    }

    #[test]
    #[should_panic(expected = "capacity exceeded")]
    fn test_pooled_vec_capacity_exceeded() {
        let mut vec = PooledVec::<i32>::with_capacity(5);

        for i in 0..6 {
            vec.push(i);
        }
    }

    #[test]
    fn test_pooled_vec_clear_preserves_capacity() {
        let mut vec = PooledVec::<i32>::with_capacity(100);

        for i in 0..50 {
            vec.push(i);
        }

        let len_before = vec.len();
        vec.clear_keep_capacity();

        assert_eq!(vec.len(), 0);
        // Capacity should remain allocated internally
        assert!(vec.inner.capacity() >= len_before);
    }

    #[test]
    fn test_pooled_map_basic() {
        let mut map = PooledHashMap::<u64, String>::new(64);

        assert!(map.is_empty());

        map.insert_no_alloc(1, "one".to_string());
        map.insert_no_alloc(2, "two".to_string());

        assert_eq!(map.len(), 2);
        assert_eq!(map.get_no_alloc(&1), Some(&"one".to_string()));
        assert_eq!(map.get_no_alloc(&2), Some(&"two".to_string()));
        assert_eq!(map.get_no_alloc(&3), None);
    }

    #[test]
    fn test_pooled_map_collision_handling() {
        // Create a map where hashes collide (same bucket slot)
        let mut map = PooledHashMap::<u64, i32>::new(4);

        // Use keys that hash to same bucket
        map.insert_no_alloc(0x0000_0000_0000_0000, 1);
        map.insert_no_alloc(0x0000_0000_0000_8000, 2); // Likely collides

        assert_eq!(map.len(), 2);
        assert_eq!(map.get_no_alloc(&0x0000_0000_0000_0000), Some(&1));
        assert_eq!(map.get_no_alloc(&0x0000_0000_0000_8000), Some(&2));
    }

    #[test]
    fn test_pooled_map_clear_preserves_buckets() {
        let mut map = PooledHashMap::<u64, u64>::new(256);

        for i in 0..100 {
            map.insert_no_alloc(i, i * 2);
        }

        assert_eq!(map.len(), 100);

        map.clear_no_dealloc();

        assert_eq!(map.len(), 0);
        assert_eq!(map.capacity(), 256);
    }

    #[test]
    fn test_fnv1a_hash_consistency() {
        let hash1 = fnv1a_hash_u64(42);
        let hash2 = fnv1a_hash_u64(42);

        // Same input should always produce same output
        assert_eq!(hash1, hash2);

        // Different inputs should likely produce different outputs
        let hash_different = fnv1a_hash_u64(43);
        assert_ne!(hash1, hash_different);
    }

    #[test]
    fn missing_key_lookup_terminates_when_every_bucket_is_occupied() {
        let (mut map, keys) = full_map(4);
        assert_eq!(map.capacity(), 4);
        assert_eq!(map.load_factor(), 1.0);
        assert_eq!(map.occupied_slots(), vec![0, 1, 2, 3]);

        // Every bucket is taken by another key: the probe must stop after at
        // most `capacity` steps instead of spinning forever.
        for home in 0..map.capacity() {
            let absent = absent_key_with_home(home, map.capacity(), &keys);
            assert_eq!(map.get_no_alloc(&absent), None);
            assert_eq!(map.remove_no_alloc(&absent), None);
        }
        assert_eq!(map.len(), 4);
        assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
    }

    #[test]
    fn deleting_a_collided_entry_keeps_the_rest_of_the_chain_reachable() {
        let mut map = PooledHashMap::<u64, u64>::new(8);
        let chain = keys_with_home(home_slot(&0, 8), 8, 3);
        for key in &chain {
            assert_eq!(map.insert_no_alloc(*key, *key), None);
        }
        assert_eq!(map.len(), 3);
        // The three keys occupy three consecutive buckets: home, home+1, home+2.
        let head = home_slot(&chain[0], 8);
        assert_eq!(
            map.occupied_slots(),
            vec![head, (head + 1) & 7, (head + 2) & 7]
        );

        // Removing the head of the chain must not hide the tail behind it.
        assert_eq!(map.remove_no_alloc(&chain[0]), Some(chain[0]));
        assert_eq!(map.get_no_alloc(&chain[0]), None);
        assert_eq!(map.get_no_alloc(&chain[1]), Some(&chain[1]));
        assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
        assert_eq!(map.len(), 2);
        assert_eq!(map.tombstone_slots(), vec![head]);

        // Removing the new head again leaves the last entry reachable.
        assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
        assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
        assert_eq!(map.len(), 1);

        // The final removal empties the chain, so the trailing tombstones are
        // repaired away instead of lingering.
        assert_eq!(map.remove_no_alloc(&chain[2]), Some(chain[2]));
        assert!(map.is_empty());
        assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
    }

    #[test]
    fn updating_an_existing_key_succeeds_when_every_bucket_is_occupied() {
        let (mut map, keys) = full_map(4);
        assert_eq!(map.load_factor(), 1.0);

        // The existence check runs before the capacity check, so a full table
        // still accepts updates for keys it already holds.
        for key in &keys {
            assert_eq!(map.insert_no_alloc(*key, key + 100), Some(*key));
            assert_eq!(map.get_no_alloc(key), Some(&(key + 100)));
        }
        assert_eq!(map.len(), 4);
    }

    #[test]
    #[should_panic(expected = "PooledHashMap full")]
    fn inserting_a_new_key_into_a_full_table_panics() {
        let (mut map, keys) = full_map(4);
        let mut candidate = 1_000_000u64;
        while keys.contains(&candidate) {
            candidate += 1;
        }
        map.insert_no_alloc(candidate, candidate);
    }

    #[test]
    fn repeated_delete_and_reinsert_keeps_every_entry_reachable() {
        let mut map = PooledHashMap::<u64, u64>::new(8);
        let chain = keys_with_home(home_slot(&0, 8), 8, 4);
        for key in &chain {
            map.insert_no_alloc(*key, *key);
        }

        for round in 0..64u64 {
            assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
            assert_eq!(map.len(), 3);
            // The entries behind the removed one stay reachable in every round.
            assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
            assert_eq!(map.get_no_alloc(&chain[3]), Some(&chain[3]));

            // A brand new value proves the reinsert really happened.
            let new_value = chain[1] + round + 1;
            assert_eq!(map.insert_no_alloc(chain[1], new_value), None);
            assert_eq!(map.len(), 4);
            assert_eq!(map.get_no_alloc(&chain[1]), Some(&new_value));
            assert_eq!(map.get_no_alloc(&chain[0]), Some(&chain[0]));
            // The previous value must be gone; restore the stable one.
            assert_ne!(map.get_no_alloc(&chain[1]), Some(&chain[1]));
            map.insert_no_alloc(chain[1], chain[1]);
        }

        assert_eq!(map.len(), 4);
        assert_eq!(map.tombstone_slots(), Vec::<usize>::new());
    }

    #[test]
    fn probing_wraps_around_the_end_of_the_bucket_array() {
        let mut map = PooledHashMap::<u64, u64>::new(4);
        let last = map.capacity() - 1;
        let chain = keys_with_home(last, 4, 3);
        for key in &chain {
            map.insert_no_alloc(*key, *key);
        }

        // The chain starts in the final bucket and overflows into bucket 0.
        assert_eq!(home_slot(&chain[0], 4), last);
        assert_eq!(map.occupied_slots(), vec![0, 1, last]);
        for key in &chain {
            assert_eq!(map.get_no_alloc(key), Some(key));
        }

        // A key homed at the last bucket but absent must still terminate.
        let absent = keys_with_home(last, 4, 4)[3];
        assert_eq!(map.get_no_alloc(&absent), None);

        // Deleting the wrapped entry must keep the slot after the wrap reachable.
        assert_eq!(map.remove_no_alloc(&chain[1]), Some(chain[1]));
        assert_eq!(map.tombstone_slots(), vec![0]);
        assert_eq!(map.get_no_alloc(&chain[2]), Some(&chain[2]));
        assert_eq!(map.get_no_alloc(&chain[0]), Some(&chain[0]));

        // Reinsert reuses the tombstone that wrapped around.
        assert_eq!(map.insert_no_alloc(chain[1], chain[1]), None);
        assert_eq!(map.occupied_slots(), vec![0, 1, last]);
        assert_eq!(map.len(), 3);
    }
}
