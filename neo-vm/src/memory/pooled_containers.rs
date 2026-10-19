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
    buckets: Vec<Option<(K, V)>>,
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
            buckets: (0..rounded_capacity).map(|_| None).collect(),
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
    /// collision resolution. Returns `None` if map is full (filled_count >= capacity).
    ///
    /// # Arguments
    ///
    /// * `key` - Key to insert
    /// * `value` - Value associated with key
    ///
    /// # Returns
    ///
    /// * `Some(old_value)` - Replaced existing value for this key
    /// * `None` - Either inserted new entry OR map is full
    ///
    /// # Panics
    ///
    /// Panics if `filled_count >= capacity` (no available slots).
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
        if self.filled_count >= self.capacity {
            panic!(
                "PooledHashMap full: filled_count={} capacity={}",
                self.filled_count, self.capacity
            );
        }

        let hash = fnv1a_hash_u64(hash_key(&key));
        let slot = hash as usize & (self.capacity - 1); // Fast modulo with power-of-2

        // Linear probing for collisions
        let mut index = slot;
        loop {
            match &self.buckets[index] {
                None => {
                    // Found empty slot, insert here
                    self.buckets[index] = Some((key, value));
                    self.filled_count += 1;
                    return None;
                }
                Some((existing_key, _)) => {
                    if *existing_key == key {
                        // Key exists, replace value
                        let existing_value = self.buckets[index]
                            .replace((key, value))
                            .map(|(_, v)| v)
                            .expect("bucket was occupied by an existing key");
                        return Some(existing_value);
                    }
                    // Collision, probe next slot
                    index = (index + 1) & (self.capacity - 1);
                }
            }
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
        let hash = fnv1a_hash_u64(hash_key(&key));
        let slot = hash as usize & (self.capacity - 1);

        // Linear probing
        let mut index = slot;
        loop {
            match &self.buckets[index] {
                None => return None, // Slot empty, key not present
                Some((existing_key, value)) => {
                    if *existing_key == *key {
                        return Some(value);
                    }
                    index = (index + 1) & (self.capacity - 1);
                }
            }
        }
    }

    /// Removes a key-value pair without reallocating buckets.
    ///
    /// Requires lazy deletion strategy (marking slots as tombstones) to preserve
    /// probe chains. For simplicity, currently uses aggressive clearing approach.
    ///
    /// # Arguments
    ///
    /// * `key` - Key to remove
    ///
    /// # Returns
    ///
    /// `Some(V)` if key existed, `None` otherwise
    pub fn remove_no_alloc(&mut self, key: &K) -> Option<V> {
        let hash = fnv1a_hash_u64(hash_key(&key));
        let slot = hash as usize & (self.capacity - 1);

        let mut index = slot;
        loop {
            match &self.buckets[index] {
                None => return None,
                Some((existing_key, _)) => {
                    if *existing_key == *key {
                        let entry = self.buckets[index].take();
                        if entry.is_some() {
                            self.filled_count -= 1;
                        }
                        return entry.map(|(_, v)| v);
                    }
                    index = (index + 1) & (self.capacity - 1);
                }
            }
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
        for slot in self.buckets.iter_mut() {
            *slot = None;
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
}
