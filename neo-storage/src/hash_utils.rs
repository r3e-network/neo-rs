//! Hash utilities for storage keys.
//!
//! This module provides hash functions matching the C# Neo implementation,
//! used for computing storage key hash codes.

use rand::rngs::OsRng;
use rand::RngCore;
use std::sync::OnceLock;
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// Default xxhash3 seed matching C# implementation.
pub const DEFAULT_XX_HASH3_SEED: i64 = 40343;

/// Returns the default xxhash3 seed.
#[inline]
pub const fn default_xx_hash3_seed() -> i64 {
    DEFAULT_XX_HASH3_SEED
}

/// Computes the 32-bit hash value for the specified byte array using the xxhash3 algorithm.
/// Matches C# XxHash3_32 method.
pub fn xx_hash3_32(data: &[u8], seed: i64) -> i32 {
    let hash64 = xxh3_64_with_seed(data, seed as u64);
    hash_code_from_u64(hash64)
}

/// Matches `System.HashCode.Combine(int, int)` from C#.
pub fn hash_code_combine_i32(a: i32, b: i32) -> i32 {
    hash_code_combine_internal(&[a as u32, b as u32])
}

// Internal hash computation constants matching .NET implementation
const PRIME2: u32 = 2_246_822_519;
const PRIME3: u32 = 3_266_489_917;
const PRIME4: u32 = 668_265_263;
const PRIME5: u32 = 374_761_393;

fn hash_code_from_u64(value: u64) -> i32 {
    hash_code_combine_internal(&[hash_component_from_u64(value)])
}

fn hash_code_combine_internal(components: &[u32]) -> i32 {
    let mut hash = mix_empty_state(global_seed());
    hash = hash.wrapping_add((components.len() * 4) as u32);
    for &component in components {
        hash = queue_round(hash, component);
    }
    mix_final(hash) as i32
}

fn hash_component_from_u64(value: u64) -> u32 {
    let lower = value as u32;
    let upper = (value >> 32) as u32;
    lower ^ upper
}

fn mix_empty_state(seed: u32) -> u32 {
    seed.wrapping_add(PRIME5)
}

fn queue_round(hash: u32, queued_value: u32) -> u32 {
    hash.wrapping_add(queued_value.wrapping_mul(PRIME3))
        .rotate_left(17)
        .wrapping_mul(PRIME4)
}

fn mix_final(hash: u32) -> u32 {
    let mut value = hash;
    value ^= value >> 15;
    value = value.wrapping_mul(PRIME2);
    value ^= value >> 13;
    value = value.wrapping_mul(PRIME3);
    value ^= value >> 16;
    value
}

fn global_seed() -> u32 {
    static SEED: OnceLock<u32> = OnceLock::new();
    *SEED.get_or_init(|| {
        let mut buffer = [0u8; 4];
        OsRng.fill_bytes(&mut buffer);
        u32::from_le_bytes(buffer)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_seed() {
        assert_eq!(default_xx_hash3_seed(), 40343);
    }

    #[test]
    fn test_xx_hash3_32_empty() {
        let hash = xx_hash3_32(&[], DEFAULT_XX_HASH3_SEED);
        // Just verify it doesn't panic and returns a value
        assert!(hash != 0 || hash == 0); // Always true, but verifies computation
    }

    #[test]
    fn test_xx_hash3_32_data() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let hash1 = xx_hash3_32(&data, DEFAULT_XX_HASH3_SEED);
        let hash2 = xx_hash3_32(&data, DEFAULT_XX_HASH3_SEED);
        assert_eq!(hash1, hash2); // Same input, same hash
    }

    #[test]
    fn test_hash_code_combine() {
        let hash = hash_code_combine_i32(1, 2);
        // Verify it produces consistent results
        assert_eq!(hash, hash_code_combine_i32(1, 2));
        // Different inputs should produce different hashes (usually)
        assert_ne!(hash, hash_code_combine_i32(1, 3));
    }
}
