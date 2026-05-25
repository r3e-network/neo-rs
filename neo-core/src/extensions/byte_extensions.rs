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
        neo_storage::xx_hash3_32(self, seed)
    }

    fn to_hex_string(&self) -> String {
        hex::encode(self)
    }

    fn to_hex_string_reverse(&self, reverse: bool) -> String {
        if !reverse {
            return self.to_hex_string();
        }

        hex::encode(self.iter().rev().copied().collect::<Vec<_>>())
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

/// Matches `System.HashCode.Combine(int, int)` from C#.
pub fn hash_code_combine_i32(a: i32, b: i32) -> i32 {
    neo_storage::hash_code_combine_i32(a, b)
}

const HEX_CHARS: &str = "0123456789abcdef";

/// Default xxhash3 seed
pub const fn default_xx_hash3_seed() -> i64 {
    neo_storage::default_xx_hash3_seed()
}

/// Hex characters for conversion
pub const fn hex_chars() -> &'static str {
    HEX_CHARS
}
