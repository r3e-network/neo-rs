use crate::smart_contract::StorageKey;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// A simple bloom filter for probabilistic membership testing.
/// Used to avoid expensive store lookups for keys that definitely don't exist.
pub struct BloomFilter {
    /// Bit array
    bits: Vec<AtomicU64>,
    /// Number of hash functions
    num_hashes: usize,
    /// Number of bits
    num_bits: usize,
    /// Number of elements inserted
    count: AtomicUsize,
    /// Maximum capacity before false positive rate increases significantly
    capacity: usize,
}

impl BloomFilter {
    /// Creates a new bloom filter with the specified capacity and false positive rate.
    ///
    /// Capacity is the expected number of elements.
    /// False positive rate should be between 0 and 1 (e.g., 0.01 for 1%).
    pub fn new(capacity: usize, false_positive_rate: f64) -> Self {
        // Calculate optimal size: m = -n * ln(p) / (ln(2)^2)
        let num_bits = ((-(capacity as f64) * false_positive_rate.ln()) / (2.0_f64.ln().powi(2)))
            .ceil() as usize;
        // Calculate optimal number of hash functions: k = m/n * ln(2)
        let num_hashes = ((num_bits as f64 / capacity as f64) * 2.0_f64.ln()).ceil() as usize;

        // Round up to nearest 64 bits for the bit vector
        let num_u64s = num_bits.div_ceil(64);
        let mut bits = Vec::with_capacity(num_u64s);
        for _ in 0..num_u64s {
            bits.push(AtomicU64::new(0));
        }

        Self {
            bits,
            num_hashes: num_hashes.clamp(1, 7),
            num_bits: num_u64s * 64,
            count: AtomicUsize::new(0),
            capacity,
        }
    }

    /// Creates a bloom filter sized for typical storage workloads.
    pub fn for_storage() -> Self {
        Self::new(100_000, 0.01) // 100K entries, 1% FP rate
    }

    /// Hash function using double hashing technique
    #[inline]
    #[allow(dead_code)]
    fn hash_bytes(&self, key: &[u8], seed: usize) -> usize {
        let h1 = xxhash_rust::xxh3::xxh3_64(key);
        let h2 = h1.wrapping_add(seed as u64);
        ((h1.wrapping_add(h2.wrapping_mul(seed as u64))) as usize) % self.num_bits
    }

    /// Hash function using pre-computed hash
    #[inline]
    fn hash_with_seed(&self, base_hash: u64, seed: usize) -> usize {
        let h2 = base_hash.wrapping_add(seed as u64);
        ((base_hash.wrapping_add(h2.wrapping_mul(seed as u64))) as usize) % self.num_bits
    }

    /// Insert a key into the bloom filter using raw bytes.
    pub fn insert_bytes(&self, key: &[u8]) {
        let base_hash = xxhash_rust::xxh3::xxh3_64(key);
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(base_hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;

            self.bits[word_idx].fetch_or(1u64 << bit_idx, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Insert a key into the bloom filter using a pre-computed hash.
    pub fn insert_hash(&self, hash: u64) {
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;

            self.bits[word_idx].fetch_or(1u64 << bit_idx, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    /// Check if a key might be in the set using raw bytes.
    /// Returns false if the key is definitely not present.
    /// Returns true if the key might be present (with some false positive probability).
    #[inline]
    pub fn might_contain_bytes(&self, key: &[u8]) -> bool {
        let base_hash = xxhash_rust::xxh3::xxh3_64(key);
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(base_hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;

            let word = self.bits[word_idx].load(Ordering::Relaxed);
            if (word & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }

    /// Check if a key might be in the set using a pre-computed hash.
    #[inline]
    pub fn might_contain_hash(&self, hash: u64) -> bool {
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;

            let word = self.bits[word_idx].load(Ordering::Relaxed);
            if (word & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        true
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
        for word in &self.bits {
            word.store(0, Ordering::Relaxed);
        }
        self.count.store(0, Ordering::Relaxed);
    }

    /// Returns true if the filter is approaching capacity (recommend rebuilding).
    pub fn should_rebuild(&self) -> bool {
        self.count.load(Ordering::Relaxed) >= self.capacity
    }
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
