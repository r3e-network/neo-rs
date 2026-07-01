//! Neo-compatible Bloom filter adapter.
//!
//! Bit storage is delegated to `bitvec` and hashing is delegated through the
//! crate-local Murmur3 adapter. This module owns the Neo-specific contract:
//! little-endian bit layout, `i * 0xFBA4C795 + tweak` seed schedule, and the
//! exact payload bytes used by P2P filter messages.

use crate::error::{CryptoError, CryptoResult};
use crate::murmur;
use bitvec::prelude::{BitVec, Lsb0};

const SEED_MULTIPLIER: u32 = 0xFBA4_C795;
type BloomBits = BitVec<u8, Lsb0>;

/// Probabilistic data structure for testing set membership.
///
/// Uses Murmur3 hashing with configurable hash function count and tweak value.
#[derive(Clone, Debug)]
pub struct BloomFilter {
    hash_functions: usize,
    bits: BloomBits,
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

        let byte_len = bit_size.div_ceil(8);
        Ok(Self {
            hash_functions,
            bits: BloomBits::from_vec(vec![0u8; byte_len]),
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
        let mut bytes = vec![0u8; bit_size.div_ceil(8)];
        let copy_len = bytes.len().min(elements.len());
        bytes[..copy_len].copy_from_slice(&elements[..copy_len]);
        filter.bits = BloomBits::from_vec(bytes);
        Ok(filter)
    }

    /// Inserts an element into the bloom filter.
    pub fn add(&mut self, element: &[u8]) {
        for hash_index in 0..self.hash_functions {
            let seed = bloom_seed(hash_index, self.tweak);
            self.set_bit(bit_index(self.bit_size, element, seed));
        }
    }

    /// Tests whether an element is possibly in the filter. May return false positives.
    #[must_use]
    pub fn check(&self, element: &[u8]) -> bool {
        for hash_index in 0..self.hash_functions {
            let seed = bloom_seed(hash_index, self.tweak);
            if !self.test_bit(bit_index(self.bit_size, element, seed)) {
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
        self.hash_functions
    }

    /// Returns the tweak value used for seed generation.
    #[must_use]
    pub const fn tweak(&self) -> u32 {
        self.tweak
    }

    /// Returns a copy of the underlying bit array.
    #[must_use]
    pub fn bits(&self) -> Vec<u8> {
        self.bits.clone().into_vec()
    }

    fn set_bit(&mut self, index: usize) {
        self.bits.set(index, true);
    }

    fn test_bit(&self, index: usize) -> bool {
        self.bits.get(index).is_some_and(|bit| *bit)
    }
}

fn bit_index(bit_size: usize, element: &[u8], seed: u32) -> usize {
    (murmur::murmur32(element, seed) as usize) % bit_size
}

fn bloom_seed(hash_index: usize, tweak: u32) -> u32 {
    (hash_index as u32)
        .wrapping_mul(SEED_MULTIPLIER)
        .wrapping_add(tweak)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_bits(
        bit_size: usize,
        hash_functions: usize,
        tweak: u32,
        element: &[u8],
    ) -> Vec<u8> {
        let mut bits = BloomBits::from_vec(vec![0u8; bit_size.div_ceil(8)]);
        for hash_index in 0..hash_functions {
            let seed = (hash_index as u32)
                .wrapping_mul(SEED_MULTIPLIER)
                .wrapping_add(tweak);
            let bit = (murmur::murmur32(element, seed) as usize) % bit_size;
            bits.set(bit, true);
        }
        bits.into_vec()
    }

    #[test]
    fn add_uses_neo_murmur_seed_schedule_and_lsb_bit_layout() {
        let bit_size = 32;
        let hash_functions = 3;
        let tweak = 0x1234_5678;
        let element = b"neo-rs";

        let mut filter = BloomFilter::new(bit_size, hash_functions, tweak).expect("filter");
        filter.add(element);

        assert_eq!(
            filter.bits(),
            expected_bits(bit_size, hash_functions, tweak, element)
        );
        assert!(filter.check(element));
    }

    #[test]
    fn add_is_idempotent_for_same_element() {
        let mut filter = BloomFilter::new(32, 3, 0x1234_5678).expect("filter");
        filter.add(b"neo-rs");
        let first = filter.bits();

        filter.add(b"neo-rs");

        assert_eq!(filter.bits(), first);
    }

    #[test]
    fn check_returns_false_when_a_required_bit_is_missing() {
        let bit_size = 32;
        let hash_functions = 3;
        let tweak = 0x1234_5678;
        let element = b"neo-rs";
        let mut bits = expected_bits(bit_size, hash_functions, tweak, element);
        let first_set_bit = bits
            .iter()
            .enumerate()
            .find_map(|(byte_index, byte)| {
                (0..8)
                    .find(|bit_index| byte & (1 << bit_index) != 0)
                    .map(|bit_index| (byte_index, bit_index))
            })
            .expect("expected at least one set bit");
        bits[first_set_bit.0] &= !(1 << first_set_bit.1);

        let filter = BloomFilter::with_bits(bit_size, hash_functions, tweak, &bits)
            .expect("filter with one missing bit");

        assert!(!filter.check(element));
    }

    #[test]
    fn with_bits_preserves_wire_bytes_and_ignores_extra_input_bytes() {
        let filter = BloomFilter::with_bits(10, 2, 7, &[0b1000_0001, 0b1111_1111, 0xff])
            .expect("filter with bits");

        assert_eq!(filter.bits(), vec![0b1000_0001, 0b1111_1111]);
        assert_eq!(filter.bit_size(), 10);
        assert_eq!(filter.hash_functions(), 2);
        assert_eq!(filter.tweak(), 7);
    }

    #[test]
    fn with_bits_zero_pads_short_input_for_non_byte_aligned_filters() {
        let filter = BloomFilter::with_bits(10, 2, 7, &[0b1000_0001]).expect("filter");

        assert_eq!(filter.bits(), vec![0b1000_0001, 0]);
    }

    #[test]
    fn constructor_rejects_empty_dimensions() {
        assert!(BloomFilter::new(0, 1, 0).is_err());
        assert!(BloomFilter::new(8, 0, 0).is_err());
    }
}
