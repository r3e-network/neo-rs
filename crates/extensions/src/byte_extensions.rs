// Copyright (C) 2015-2025 The Neo Project.
//
// byte_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use rand::rngs::OsRng;
use rand::RngCore;
use std::sync::OnceLock;
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// Byte extensions matching C# ByteExtensions exactly
pub trait ByteExtensions {
    /// Computes the 32-bit hash value for the specified byte array using the xxhash3 algorithm.
    /// Matches C# XxHash3_32 method
    fn xx_hash3_32(&self, seed: i64) -> i32;

    /// Converts a byte array to hex string.
    /// Matches C# ToHexString method
    fn to_hex_string(&self) -> String;

    /// Converts a byte array to hex string with reverse option.
    /// Matches C# ToHexString method with reverse parameter
    fn to_hex_string_reverse(&self, reverse: bool) -> String;

    /// Converts a byte array to a read-only span.
    /// Matches C# AsReadOnlySpan method
    fn as_read_only_span(&self) -> &[u8];

    /// All bytes are zero or not in a byte array
    /// Matches C# NotZero method
    fn not_zero(&self) -> bool;
}

impl ByteExtensions for Vec<u8> {
    fn xx_hash3_32(&self, seed: i64) -> i32 {
        self.as_slice().xx_hash3_32(seed)
    }

    fn to_hex_string(&self) -> String {
        self.as_slice().to_hex_string()
    }

    fn to_hex_string_reverse(&self, reverse: bool) -> String {
        self.as_slice().to_hex_string_reverse(reverse)
    }

    fn as_read_only_span(&self) -> &[u8] {
        self.as_slice()
    }

    fn not_zero(&self) -> bool {
        self.as_slice().not_zero()
    }
}

impl ByteExtensions for &[u8] {
    fn xx_hash3_32(&self, seed: i64) -> i32 {
        let hash64 = xxh3_64_with_seed(self, seed as u64);
        hash_code_from_u64(hash64)
    }

    fn to_hex_string(&self) -> String {
        hex::encode(self)
    }

    fn to_hex_string_reverse(&self, reverse: bool) -> String {
        if !reverse {
            return self.to_hex_string();
        }

        let mut result = String::with_capacity(self.len() * 2);
        for &byte in self.iter().rev() {
            result.push_str(&format!("{:02x}", byte));
        }
        result
    }

    fn as_read_only_span(&self) -> &[u8] {
        self
    }

    fn not_zero(&self) -> bool {
        self.iter().any(|&b| b != 0)
    }
}

impl ByteExtensions for std::ops::Range<usize> {
    fn xx_hash3_32(&self, _seed: i64) -> i32 {
        0 // Not applicable for ranges
    }

    fn to_hex_string(&self) -> String {
        String::new() // Not applicable for ranges
    }

    fn to_hex_string_reverse(&self, _reverse: bool) -> String {
        String::new() // Not applicable for ranges
    }

    fn as_read_only_span(&self) -> &[u8] {
        &[] // Not applicable for ranges
    }

    fn not_zero(&self) -> bool {
        false // Not applicable for ranges
    }
}

const PRIME2: u32 = 2_246_822_519;
const PRIME3: u32 = 3_266_489_917;
const PRIME4: u32 = 668_265_263;
const PRIME5: u32 = 374_761_393;

fn hash_code_from_u64(value: u64) -> i32 {
    hash_code_combine_internal(&[hash_component_from_u64(value)])
}

/// Matches `System.HashCode.Combine(int, int)` from C#.
pub fn hash_code_combine_i32(a: i32, b: i32) -> i32 {
    hash_code_combine_internal(&[a as u32, b as u32])
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

/// Constants matching C# constants
const DEFAULT_XX_HASH3_SEED: i64 = 40343;
const HEX_CHARS: &str = "0123456789abcdef";

/// Default xxhash3 seed
pub const fn default_xx_hash3_seed() -> i64 {
    DEFAULT_XX_HASH3_SEED
}

/// Hex characters for conversion
pub const fn hex_chars() -> &'static str {
    HEX_CHARS
}
