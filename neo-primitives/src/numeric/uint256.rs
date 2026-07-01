//! Implementation of `UInt256`, a 256-bit unsigned integer.

use serde::{Deserialize, Serialize};

/// The length of `UInt256` values in bytes.
pub const UINT256_SIZE: usize = crate::constants::HASH_SIZE;

crate::uint_type! {
    /// Represents a 256-bit unsigned integer.
    #[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
    #[repr(C)]
    pub struct UInt256 {
        size = UINT256_SIZE;
        size_const = UINT256_SIZE;
        /// Zero value for `UInt256`.
        ZERO;
        as_ref = true;
        fields: [value1: u64, value2: u64, value3: u64, value4: u64];
    }
}

impl UInt256 {
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    /// Returns the Neo-compatible 32-bit hash code for this value.
    pub const fn hash_code(&self) -> i32 {
        let v1_hash = (self.value1 as i32) ^ ((self.value1 >> 32) as i32);
        let v2_hash = (self.value2 as i32) ^ ((self.value2 >> 32) as i32);
        let v3_hash = (self.value3 as i32) ^ ((self.value3 >> 32) as i32);
        let v4_hash = (self.value4 as i32) ^ ((self.value4 >> 32) as i32);
        v1_hash ^ v2_hash ^ v3_hash ^ v4_hash
    }
}

#[cfg(test)]
#[path = "../tests/numeric/uint256.rs"]
mod tests;
