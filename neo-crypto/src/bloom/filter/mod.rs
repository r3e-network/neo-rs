use alloc::{vec, vec::Vec};

mod bits;
mod hash;
mod seeds;
use bits::{copy_bits, set_bit, test_bit};
use hash::hash_element;
use seeds::SeedDeriver;

use crate::BloomError;

const N_HF_MULTIPLIER: u32 = 0xFBA4_C795;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BloomFilter {
    seeds: Vec<u32>,
    bits: Vec<u8>,
    bit_len: usize,
    tweak: u32,
}

impl BloomFilter {
    pub fn new(m: usize, k: usize, tweak: u32) -> Result<Self, BloomError> {
        if m == 0 {
            return Err(BloomError::InvalidBitLength);
        }
        if k == 0 {
            return Err(BloomError::InvalidHashFunctionCount);
        }
        let seeds = SeedDeriver::new(N_HF_MULTIPLIER, tweak).derive(k);
        let bytes = vec![0u8; bytes_for_bits(m)];
        Ok(Self {
            seeds,
            bits: bytes,
            bit_len: m,
            tweak,
        })
    }

    pub fn with_bits(m: usize, k: usize, tweak: u32, initial: &[u8]) -> Result<Self, BloomError> {
        let mut filter = Self::new(m, k, tweak)?;
        let needed = filter.bits.len();
        if initial.is_empty() {
            return Ok(filter);
        }
        let copy_len = core::cmp::min(initial.len(), needed);
        filter.bits[..copy_len].copy_from_slice(&initial[..copy_len]);
        Ok(filter)
    }

    pub fn bits(&self) -> &[u8] {
        &self.bits
    }

    pub fn k(&self) -> usize {
        self.seeds.len()
    }

    pub fn m(&self) -> usize {
        self.bit_len
    }

    pub fn tweak(&self) -> u32 {
        self.tweak
    }

    pub fn add(&mut self, element: &[u8]) {
        let bit_len = self.bit_len as u32;
        for &seed in &self.seeds {
            let position = hash_element(element, seed, bit_len);
            set_bit(&mut self.bits, position);
        }
    }

    pub fn check(&self, element: &[u8]) -> bool {
        let bit_len = self.bit_len as u32;
        self.seeds.iter().all(|&seed| {
            let position = hash_element(element, seed, bit_len);
            test_bit(&self.bits, position)
        })
    }

    pub fn copy_bits(&self, out: &mut [u8]) {
        copy_bits(&self.bits, out);
    }

    #[cfg(test)]
    pub(crate) fn bytes_for_bits(bits: usize) -> usize {
        bytes_for_bits(bits)
    }
}

#[inline]
fn bytes_for_bits(bits: usize) -> usize {
    (bits + 7) / 8
}
