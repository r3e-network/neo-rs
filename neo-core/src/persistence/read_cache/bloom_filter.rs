use crate::smart_contract::StorageKey;
use fastbloom::AtomicBloomFilter;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A concurrent bloom filter for probabilistic membership testing.
/// Used to avoid expensive store lookups for keys that definitely don't exist.
pub struct BloomFilter {
    filter: AtomicBloomFilter,
    count: AtomicUsize,
    capacity: usize,
}

impl BloomFilter {
    /// Creates a new bloom filter with the specified capacity and false positive rate.
    ///
    /// Capacity is the expected number of elements.
    /// False positive rate should be between 0 and 1 (e.g., 0.01 for 1%).
    pub fn new(capacity: usize, false_positive_rate: f64) -> Self {
        let capacity = capacity.max(1);
        let false_positive_rate = normalize_false_positive_rate(false_positive_rate);
        let filter =
            AtomicBloomFilter::with_false_pos(false_positive_rate).expected_items(capacity);

        Self {
            filter,
            count: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Creates a bloom filter sized for typical storage workloads.
    pub fn for_storage() -> Self {
        Self::new(100_000, 0.01) // 100K entries, 1% FP rate
    }

    /// Insert a key into the bloom filter using raw bytes.
    pub fn insert_bytes(&self, key: &[u8]) {
        self.insert_hash(xxhash_rust::xxh3::xxh3_64(key));
    }

    /// Insert a key into the bloom filter using a pre-computed hash.
    pub fn insert_hash(&self, hash: u64) {
        self.filter.insert_hash(hash);
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if a key might be in the set using raw bytes.
    /// Returns false if the key is definitely not present.
    /// Returns true if the key might be present (with some false positive probability).
    #[inline]
    pub fn might_contain_bytes(&self, key: &[u8]) -> bool {
        self.might_contain_hash(xxhash_rust::xxh3::xxh3_64(key))
    }

    /// Check if a key might be in the set using a pre-computed hash.
    #[inline]
    pub fn might_contain_hash(&self, hash: u64) -> bool {
        self.filter.contains_hash(hash)
    }

    /// Returns the approximate number of elements inserted.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Returns true if no elements have been inserted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears the bloom filter.
    pub fn clear(&self) {
        self.filter.clear();
        self.count.store(0, Ordering::Relaxed);
    }

    /// Returns true if the filter is approaching capacity (recommend rebuilding).
    pub fn should_rebuild(&self) -> bool {
        self.count.load(Ordering::Relaxed) >= self.capacity
    }
}

fn normalize_false_positive_rate(false_positive_rate: f64) -> f64 {
    if !false_positive_rate.is_finite() || false_positive_rate <= 0.0 {
        return 0.01;
    }

    false_positive_rate.min(0.999_999)
}

/// Trait for keys that can be hashed for bloom filter operations.
pub trait BloomFilterKey {
    /// Hashes the key using xxh3 and returns the hash value.
    fn hash_for_bloom(&self) -> u64;
}

impl BloomFilterKey for Vec<u8> {
    fn hash_for_bloom(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(self)
    }
}

impl BloomFilterKey for String {
    fn hash_for_bloom(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(self.as_bytes())
    }
}

impl BloomFilterKey for StorageKey {
    fn hash_for_bloom(&self) -> u64 {
        // Combine id and key bytes for hashing
        let id_bytes = self.id().to_le_bytes();
        let key_bytes = self.key();

        // Use xxh3 with a seed for consistent hashing
        let mut hasher = xxhash_rust::xxh3::Xxh3::new();
        hasher.update(&id_bytes);
        hasher.update(key_bytes);
        hasher.digest()
    }
}
