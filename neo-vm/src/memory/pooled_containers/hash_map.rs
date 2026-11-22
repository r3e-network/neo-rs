use super::*;

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
    pub(super) fn occupied_slots(&self) -> Vec<usize> {
        self.buckets
            .iter()
            .enumerate()
            .filter(|(_, bucket)| matches!(bucket, Bucket::Occupied(..)))
            .map(|(index, _)| index)
            .collect()
    }

    /// Bucket indices that currently hold a tombstone, in ascending order.
    #[cfg(test)]
    pub(super) fn tombstone_slots(&self) -> Vec<usize> {
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
