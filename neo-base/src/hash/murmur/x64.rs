use core::convert::TryInto;

use super::mix::fmix64;

/// Compute the 128-bit Murmur3 hash (x64 variant) used throughout the Neo stack.
#[inline]
pub fn murmur128<T: AsRef<[u8]>>(data: T, seed: u32) -> [u8; 16] {
    const C1: u64 = 0x87c3_7b91_1142_53d5;
    const C2: u64 = 0x4cf5_ad43_2745_937f;
    const R1: u32 = 31;
    const R2: u32 = 33;
    const M: u64 = 5;
    const N1: u64 = 0x52dc_e729;
    const N2: u64 = 0x3849_5ab5;

    let bytes = data.as_ref();
    let blocks = bytes.len() / 16;

    let mut h1 = seed as u64;
    let mut h2 = seed as u64;

    for i in 0..blocks {
        let offset = i * 16;
        let block = &bytes[offset..offset + 16];
        let mut k1 = u64::from_le_bytes(block[0..8].try_into().expect("block slice length"));
        let mut k2 = u64::from_le_bytes(block[8..16].try_into().expect("block slice length"));

        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(R1);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;

        h1 = h1.rotate_left(27);
        h1 = h1.wrapping_add(h2);
        h1 = h1.wrapping_mul(M).wrapping_add(N1);

        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(R2);
        k2 = k2.wrapping_mul(C1);
        h2 ^= k2;

        h2 = h2.rotate_left(31);
        h2 = h2.wrapping_add(h1);
        h2 = h2.wrapping_mul(M).wrapping_add(N2);
    }

    let tail = &bytes[blocks * 16..];
    if !tail.is_empty() {
        let mut buffer = [0u8; 16];
        buffer[..tail.len()].copy_from_slice(tail);

        let mut k1 = u64::from_le_bytes(buffer[0..8].try_into().expect("buffer slice length"));
        let mut k2 = u64::from_le_bytes(buffer[8..16].try_into().expect("buffer slice length"));

        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(R2);
        k2 = k2.wrapping_mul(C1);
        h2 ^= k2;

        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(R1);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;
    }

    let length = bytes.len() as u64;
    h1 ^= length;
    h2 ^= length;

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    h1 = fmix64(h1);
    h2 = fmix64(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    let mut output = [0u8; 16];
    output[..8].copy_from_slice(&h1.to_le_bytes());
    output[8..].copy_from_slice(&h2.to_le_bytes());
    output
}
