use alloc::{vec, vec::Vec};

use core::cmp::min;

use neo_base::hash::murmur32;

const N_HF_MULTIPLIER: u32 = 0xFBA4_C795;

/// Bloom filter implementation mirroring the Neo C# node semantics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BloomFilter {
    seeds: Vec<u32>,
    bits: Vec<u8>,
    bit_len: usize,
    tweak: u32,
}

impl BloomFilter {
    /// Create a bloom filter with the provided parameters.
    pub fn new(m: usize, k: usize, tweak: u32) -> Result<Self, BloomError> {
        if m == 0 {
            return Err(BloomError::InvalidBitLength);
        }
        if k == 0 {
            return Err(BloomError::InvalidHashFunctionCount);
        }
        let seeds = (0..k)
            .map(|i| (i as u32).wrapping_mul(N_HF_MULTIPLIER).wrapping_add(tweak))
            .collect::<Vec<_>>();
        let bytes = vec![0u8; Self::bytes_for_bits(m)];
        Ok(Self {
            seeds,
            bits: bytes,
            bit_len: m,
            tweak,
        })
    }

    /// Create a bloom filter with existing bit data (truncating or padding as needed).
    pub fn with_bits(m: usize, k: usize, tweak: u32, initial: &[u8]) -> Result<Self, BloomError> {
        let mut filter = Self::new(m, k, tweak)?;
        let needed = filter.bits.len();
        if initial.is_empty() {
            return Ok(filter);
        }
        let copy_len = min(initial.len(), needed);
        filter.bits[..copy_len].copy_from_slice(&initial[..copy_len]);
        if initial.len() < needed {
            // remaining bytes already zero
        }
        Ok(filter)
    }

    /// Borrow the raw bit buffer.
    pub fn bits(&self) -> &[u8] {
        &self.bits
    }

    /// Number of hash functions.
    pub fn k(&self) -> usize {
        self.seeds.len()
    }

    /// Number of bits maintained by the filter.
    pub fn m(&self) -> usize {
        self.bit_len
    }

    /// Tweak value used to derive hash seeds.
    pub fn tweak(&self) -> u32 {
        self.tweak
    }

    /// Insert an element into the filter.
    pub fn add(&mut self, element: &[u8]) {
        let bit_len = self.bit_len as u32;
        let seeds = self.seeds.clone();
        for seed in seeds {
            let hash = murmur32(element, seed);
            let position = (hash % bit_len) as usize;
            self.set_bit(position);
        }
    }

    /// Check whether the element could be present in the filter.
    pub fn check(&self, element: &[u8]) -> bool {
        let bit_len = self.bit_len as u32;
        for &seed in &self.seeds {
            let hash = murmur32(element, seed);
            let position = (hash % bit_len) as usize;
            if !self.test_bit(position) {
                return false;
            }
        }
        true
    }

    /// Copy the raw bits into the provided buffer.
    pub fn copy_bits(&self, out: &mut [u8]) {
        let copy_len = min(out.len(), self.bits.len());
        out[..copy_len].copy_from_slice(&self.bits[..copy_len]);
        if out.len() > copy_len {
            for byte in &mut out[copy_len..] {
                *byte = 0;
            }
        }
    }

    #[inline]
    fn set_bit(&mut self, index: usize) {
        let byte = index / 8;
        let offset = (index % 8) as u8;
        if let Some(slot) = self.bits.get_mut(byte) {
            *slot |= 1 << offset;
        }
    }

    #[inline]
    fn test_bit(&self, index: usize) -> bool {
        let byte = index / 8;
        let offset = (index % 8) as u8;
        self.bits
            .get(byte)
            .map(|slot| (slot >> offset) & 1 == 1)
            .unwrap_or(false)
    }

    #[inline]
    fn bytes_for_bits(bits: usize) -> usize {
        (bits + 7) / 8
    }
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum BloomError {
    #[error("bloom filter: bit length must be greater than zero")]
    InvalidBitLength,
    #[error("bloom filter: hash function count must be greater than zero")]
    InvalidHashFunctionCount,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_rejects_invalid_values() {
        assert_eq!(
            BloomFilter::new(0, 3, 123).unwrap_err(),
            BloomError::InvalidBitLength
        );
        assert_eq!(
            BloomFilter::new(3, 0, 123).unwrap_err(),
            BloomError::InvalidHashFunctionCount
        );
    }

    #[test]
    fn constructor_sets_properties() {
        let filter = BloomFilter::new(7, 10, 123456).unwrap();
        assert_eq!(filter.m(), 7);
        assert_eq!(filter.k(), 10);
        assert_eq!(filter.tweak(), 123456);
    }

    #[test]
    fn with_bits_handles_short_and_long_inputs() {
        let shorter = [0u8; 5];
        let filter = BloomFilter::with_bits(7, 10, 123456, &shorter).unwrap();
        assert_eq!(filter.m(), 7);
        assert_eq!(filter.k(), 10);

        let longer = [1u8; 16];
        let filter = BloomFilter::with_bits(7, 10, 123456, &longer).unwrap();
        assert_eq!(filter.bits().len(), BloomFilter::bytes_for_bits(7));
        assert!(filter.bits().iter().all(|&byte| byte == 1));
    }

    #[test]
    fn add_and_check_behaves_like_csharp() {
        let mut filter = BloomFilter::new(7, 10, 123456).unwrap();
        let element = [0u8, 1, 2, 3, 4];
        filter.add(&element);
        assert!(filter.check(&element));
        let another = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
        assert!(!filter.check(&another));
    }

    #[test]
    fn copy_bits_returns_raw_buffer() {
        let mut filter = BloomFilter::new(7, 10, 123456).unwrap();
        filter.add(&[1, 2, 3, 4]);
        let mut buffer = [0u8; 7];
        filter.copy_bits(&mut buffer);
        // Ensure copy matches internal bits prefix.
        assert_eq!(&buffer[..filter.bits().len()], filter.bits());
    }
}
