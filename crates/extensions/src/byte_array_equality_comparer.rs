// Copyright (C) 2015-2025 The Neo Project.
//
// byte_array_equality_comparer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::hash::{Hash, Hasher};
use xxhash_rust::xxh32::xxh32;

/// Byte array equality comparer matching C# ByteArrayEqualityComparer exactly
pub struct ByteArrayEqualityComparer;

impl Default for ByteArrayEqualityComparer {
    fn default() -> Self {
        Self::new()
    }
}

impl ByteArrayEqualityComparer {
    /// Default comparer
    /// Matches C# Default property
    pub const DEFAULT: ByteArrayEqualityComparer = ByteArrayEqualityComparer;

    /// Creates a new ByteArrayEqualityComparer
    /// Matches C# constructor
    pub fn new() -> Self {
        Self
    }

    /// Compares two byte arrays for equality
    /// Matches C# Equals method
    pub fn equals(&self, x: Option<&[u8]>, y: Option<&[u8]>) -> bool {
        match (x, y) {
            (None, None) => true,
            (Some(x_bytes), Some(y_bytes)) => {
                if std::ptr::eq(x_bytes, y_bytes) {
                    true
                } else {
                    x_bytes.len() == y_bytes.len() && x_bytes == y_bytes
                }
            }
            _ => false,
        }
    }

    /// Gets the hash code for a byte array
    /// Matches C# GetHashCode method
    pub fn get_hash_code(&self, obj: &[u8]) -> u32 {
        xxh32(obj, 40_343)
    }
}

impl PartialEq for ByteArrayEqualityComparer {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ByteArrayEqualityComparer {}

impl Hash for ByteArrayEqualityComparer {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(0);
    }
}
