use core::convert::TryInto;

use sha2::{Digest, Sha512};

use crate::uint::{UInt160, UInt256};

pub type Hash160 = UInt160;
pub type Hash256 = UInt256;

/// Compute a single round SHA-256 hash over the provided bytes.
#[inline]
pub fn sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    let mut h = sha2::Sha256::new();
    h.update(data.as_ref());
    h.finalize().into()
}

/// Compute two rounds of SHA-256 – the default block hashing strategy in Neo.
#[inline]
pub fn double_sha256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    sha256(sha256(data))
}

/// Compute RIPEMD-160 hash.
#[inline]
pub fn ripemd160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let mut ripemd = ripemd::Ripemd160::new();
    ripemd.update(data.as_ref());
    ripemd.finalize().into()
}

/// Compute RIPEMD-160(SHA-256(data)) which is used for script hashes.
#[inline]
pub fn hash160<T: AsRef<[u8]>>(data: T) -> [u8; 20] {
    let sha = sha256(data);
    ripemd160(sha)
}

/// Produce a typed `Hash160` from the provided payload.
#[inline]
pub fn hash160_typed<T: AsRef<[u8]>>(data: T) -> Hash160 {
    Hash160::from_slice(&hash160(data)).expect("hash160 output is 20 bytes")
}

/// Compute Keccak-256 hash.
#[inline]
pub fn keccak256<T: AsRef<[u8]>>(data: T) -> [u8; 32] {
    use sha3::Digest as _;
    let mut hasher = sha3::Keccak256::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

/// Compute a single round SHA-512 hash.
#[inline]
pub fn sha512<T: AsRef<[u8]>>(data: T) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(data.as_ref());
    hasher.finalize().into()
}

/// Produce a typed `Hash256` from the provided payload.
#[inline]
pub fn double_sha256_typed<T: AsRef<[u8]>>(data: T) -> Hash256 {
    Hash256::from_slice(&double_sha256(data)).expect("double sha256 output is 32 bytes")
}

/// Compute the 128-bit Murmur3 hash (x64 variant) used throughout the Neo stack.
///
/// The implementation mirrors the C# reference (little-endian output with the same
/// rotation/mixing constants) so results align with the canonical node.
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
    if remainder.len() > 0 {
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

#[inline]
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^ (k >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn murmur128_matches_csharp_vectors() {
        let cases = [
            ("hello", "0bc59d0ad25fde2982ed65af61227a0e"),
            ("world", "3d3810fed480472bd214a14023bb407f"),
            ("hello world", "e0a0632d4f51302c55e3b3e48d28795d"),
        ];

        for (input, expected_hex) in cases {
            let digest = murmur128(input.as_bytes(), 123u32);
            assert_eq!(hex::encode(digest), expected_hex);
        }

        let bytes = hex::decode("718f952132679baa9c5c2aa0d329fd2a").expect("hex fixture");
        let digest = murmur128(bytes, 123u32);
        assert_eq!(hex::encode(digest), "9b4aa747ff0cf4e41b3d96251551c8ae");
    }

    #[test]
    fn murmur32_matches_csharp_vectors() {
        let array = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1];
        assert_eq!(murmur32(&array, 10), 378_574_820);

        let data = b"hello worldhello world";
        assert_eq!(murmur32(data.as_slice(), 10), 60_539_726);

        assert_eq!(murmur32(b"he".as_slice(), 10), 972_873_329);
    }

    #[test]
    fn sha512_matches_csharp_vector() {
        let digest = sha512(b"hello world");
        assert_eq!(
            hex::encode(digest),
            "309ecc489c12d6eb4cc40f50c902f2b4d0ed77ee511a7c7a9bcd3ca86d4cd86f\
             989dd35bc5ff499670da34255b45b0cfd830e81f605dcf7dc5542e93ae9cd76f"
        );
    }

    #[test]
    fn keccak256_matches_csharp_vector() {
        let digest = keccak256(b"Hello, world!");
        assert_eq!(
            hex::encode(digest),
            "b6e16d27ac5ab427a7f68900ac5559ce272dc6c37c82b3e052246c82244c50e4"
        );
    }
}
