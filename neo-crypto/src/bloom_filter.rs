use crate::error::{CryptoError, CryptoResult};
use murmur3::murmur3_32;
use std::io::Cursor;

const SEED_MULTIPLIER: u32 = 0xFBA4_C795;

/// Probabilistic data structure for testing set membership.
///
/// Uses Murmur3 hashing with configurable hash function count and tweak value.
#[derive(Clone, Debug)]
pub struct BloomFilter {
    seeds: Vec<u32>,
    bits: Vec<u8>,
    bit_size: usize,
    tweak: u32,
}

impl BloomFilter {
    /// Creates a new empty bloom filter with the given bit size, hash function count, and tweak.
    pub fn new(bit_size: usize, hash_functions: usize, tweak: u32) -> CryptoResult<Self> {
        if hash_functions == 0 {
            return Err(CryptoError::invalid_argument(
                "Bloom filter hash function count must be greater than zero",
            ));
        }
        if bit_size == 0 {
            return Err(CryptoError::invalid_argument(
                "Bloom filter bit array size must be greater than zero",
            ));
        }

        let seeds = (0..hash_functions)
            .map(|i| (i as u32).wrapping_mul(SEED_MULTIPLIER).wrapping_add(tweak))
            .collect();
        let byte_len = bit_size.div_ceil(8);
        Ok(Self {
            seeds,
            bits: vec![0u8; byte_len],
            bit_size,
            tweak,
        })
    }

    /// Creates a bloom filter pre-populated with the given bit array.
    pub fn with_bits(
        bit_size: usize,
        hash_functions: usize,
        tweak: u32,
        elements: &[u8],
    ) -> CryptoResult<Self> {
        let mut filter = Self::new(bit_size, hash_functions, tweak)?;
        let copy_len = filter.bits.len().min(elements.len());
        filter.bits[..copy_len].copy_from_slice(&elements[..copy_len]);
        Ok(filter)
    }

    /// Inserts an element into the bloom filter.
    pub fn add(&mut self, element: &[u8]) {
        let seeds = self.seeds.clone();
        for seed in seeds {
            let mut cursor = Cursor::new(element);
            let hash = murmur3_32(&mut cursor, seed).expect("murmur3 hashing should not fail");
            self.set_bit((hash as usize) % self.bit_size);
        }
    }

    /// Tests whether an element is possibly in the filter. May return false positives.
    #[must_use]
    pub fn check(&self, element: &[u8]) -> bool {
        for seed in &self.seeds {
            let mut cursor = Cursor::new(element);
            let hash = murmur3_32(&mut cursor, *seed).expect("murmur3 hashing should not fail");
            if !self.test_bit((hash as usize) % self.bit_size) {
                return false;
            }
        }
        true
    }

    /// Returns the size of the bit array.
    #[must_use]
    pub const fn bit_size(&self) -> usize {
        self.bit_size
    }

    /// Returns the number of hash functions used by this filter.
    #[must_use]
    pub fn hash_functions(&self) -> usize {
        self.seeds.len()
    }

    /// Returns the tweak value used for seed generation.
    #[must_use]
    pub const fn tweak(&self) -> u32 {
        self.tweak
    }

    /// Returns a copy of the underlying bit array.
    #[must_use]
    pub fn bits(&self) -> Vec<u8> {
        self.bits.clone()
    }

    fn set_bit(&mut self, index: usize) {
        let byte = index / 8;
        let offset = index % 8;
        if let Some(entry) = self.bits.get_mut(byte) {
            *entry |= 1 << offset;
        }
    }

    fn test_bit(&self, index: usize) -> bool {
        let byte = index / 8;
        let offset = index % 8;
        match self.bits.get(byte) {
            Some(entry) => (*entry & (1 << offset)) != 0,
            None => false,
        }
    }
}
