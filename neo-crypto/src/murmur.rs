//! Murmur3 hash helpers used by Neo runtime and native contracts.

use murmur3::murmur3_32;
use std::convert::TryInto;
use std::io::Cursor;

/// Computes a 32-bit Murmur3 hash of the given data with the specified seed.
#[must_use]
pub fn murmur32(data: &[u8], seed: u32) -> u32 {
    murmur3_32(&mut Cursor::new(data), seed).expect("murmur32 hashing should not fail")
}

/// Computes a 128-bit Murmur3 hash of the given data with the specified seed.
#[must_use]
pub fn murmur128(data: &[u8], seed: u32) -> [u8; 16] {
    const C1: u64 = 0x87c3_7b91_1142_53d5;
    const C2: u64 = 0x4cf5_ad43_2745_937f;
    const R1: u32 = 31;
    const R2: u32 = 33;
    const M: u64 = 5;
    const N1: u64 = 0x52dc_e729;
    const N2: u64 = 0x3849_5ab5;

    const fn fmix(mut h: u64) -> u64 {
        h = (h ^ (h >> 33)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        h = (h ^ (h >> 33)).wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^ (h >> 33)
    }

    let mut h1 = u64::from(seed);
    let mut h2 = u64::from(seed);

    let mut chunks = data.chunks_exact(16);
    for chunk in &mut chunks {
        let k1 = u64::from_le_bytes(chunk[0..8].try_into().unwrap());
        let k2 = u64::from_le_bytes(chunk[8..16].try_into().unwrap());

        h1 ^= (k1.wrapping_mul(C1)).rotate_left(R1).wrapping_mul(C2);
        h1 = h1.rotate_left(27).wrapping_add(h2);
        h1 = h1.wrapping_mul(M).wrapping_add(N1);

        h2 ^= (k2.wrapping_mul(C2)).rotate_left(R2).wrapping_mul(C1);
        h2 = h2.rotate_left(31).wrapping_add(h1);
        h2 = h2.wrapping_mul(M).wrapping_add(N2);
    }

    let remainder = chunks.remainder();
    if !remainder.is_empty() {
        let mut tail = [0u8; 16];
        tail[..remainder.len()].copy_from_slice(remainder);
        let k1 = u64::from_le_bytes(tail[0..8].try_into().unwrap());
        let k2 = u64::from_le_bytes(tail[8..16].try_into().unwrap());

        h2 ^= (k2.wrapping_mul(C2)).rotate_left(R2).wrapping_mul(C1);
        h1 ^= (k1.wrapping_mul(C1)).rotate_left(R1).wrapping_mul(C2);
    }

    let length = data.len() as u64;
    h1 ^= length;
    h2 ^= length;

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    h1 = fmix(h1);
    h2 = fmix(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    let mut output = [0u8; 16];
    output[..8].copy_from_slice(&h1.to_le_bytes());
    output[8..].copy_from_slice(&h2.to_le_bytes());
    output
}
