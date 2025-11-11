use core::convert::TryInto;

/// Compute the 32-bit Murmur3 hash (x86 variant) matching the C# helper.
#[inline]
pub fn murmur32<T: AsRef<[u8]>>(data: T, seed: u32) -> u32 {
    const C1: u32 = 0xcc9e_2d51;
    const C2: u32 = 0x1b87_3593;
    const R1: u32 = 15;
    const R2: u32 = 13;
    const M: u32 = 5;
    const N: u32 = 0xe654_6b64;

    let bytes = data.as_ref();
    let mut hash = seed;

    let mut chunks = bytes.chunks_exact(4);
    for chunk in &mut chunks {
        let mut k = u32::from_le_bytes(chunk.try_into().expect("chunk length"));
        k = k.wrapping_mul(C1);
        k = k.rotate_left(R1);
        k = k.wrapping_mul(C2);
        hash ^= k;
        hash = hash.rotate_left(R2);
        hash = hash.wrapping_mul(M).wrapping_add(N);
    }

    let mut k1 = 0u32;
    let remainder = chunks.remainder();
    match remainder.len() {
        3 => {
            k1 ^= (remainder[2] as u32) << 16;
            k1 ^= (remainder[1] as u32) << 8;
            k1 ^= remainder[0] as u32;
        }
        2 => {
            k1 ^= (remainder[1] as u32) << 8;
            k1 ^= remainder[0] as u32;
        }
        1 => {
            k1 ^= remainder[0] as u32;
        }
        _ => {}
    }
    if !remainder.is_empty() {
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(R1);
        k1 = k1.wrapping_mul(C2);
        hash ^= k1;
    }

    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85eb_ca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae35);
    hash ^ (hash >> 16)
}
