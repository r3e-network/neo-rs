//! Implementation of `UInt256`, a 256-bit unsigned integer.

use crate::constants::HASH_SIZE;
use crate::error::{PrimitiveError, PrimitiveResult};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use tracing::error;

/// The length of `UInt256` values in bytes.
pub const UINT256_SIZE: usize = HASH_SIZE;

/// Represents a 256-bit unsigned integer.
///
/// This is implemented as a reference type to match the C# implementation.
#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct UInt256 {
    /// First 8 bytes of the `UInt256` (least significant).
    pub value1: u64,
    /// Next 8 bytes of the `UInt256`.
    pub value2: u64,
    /// Next 8 bytes of the `UInt256`.
    pub value3: u64,
    /// Last 8 bytes of the `UInt256` (most significant).
    pub value4: u64,
}

/// Zero value for `UInt256`.
pub static ZERO: UInt256 = UInt256 {
    value1: 0,
    value2: 0,
    value3: 0,
    value4: 0,
};

impl UInt256 {
    /// Alias matching C# `UInt256.Length`.
    pub const LENGTH: usize = UINT256_SIZE;

    /// Creates a new `UInt256` instance.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a zero `UInt256`.
    #[inline]
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Checks if this `UInt256` is zero.
    #[inline]
    #[must_use]
    pub const fn is_zero(&self) -> bool {
        self.value1 == 0 && self.value2 == 0 && self.value3 == 0 && self.value4 == 0
    }

    /// Returns the bytes representation of this `UInt256`.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> [u8; HASH_SIZE] {
        self.to_array()
    }

    /// Returns the bytes as a `Vec<u8>`
    #[inline]
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HASH_SIZE);
        bytes.extend_from_slice(&self.value1.to_le_bytes());
        bytes.extend_from_slice(&self.value2.to_le_bytes());
        bytes.extend_from_slice(&self.value3.to_le_bytes());
        bytes.extend_from_slice(&self.value4.to_le_bytes());
        bytes
    }

    /// Determines whether this instance and another specified `UInt256` object have the same value.
    #[inline]
    #[must_use]
    pub const fn equals(&self, other: Option<&Self>) -> bool {
        if let Some(other) = other {
            self.value1 == other.value1
                && self.value2 == other.value2
                && self.value3 == other.value3
                && self.value4 == other.value4
        } else {
            false
        }
    }

    /// Creates a new `UInt256` from a byte array.
    ///
    /// # Errors
    ///
    /// Returns `PrimitiveError::InvalidFormat` if the input length is not exactly 32 bytes.
    #[inline]
    pub fn from_bytes(value: &[u8]) -> PrimitiveResult<Self> {
        if value.len() != UINT256_SIZE {
            return Err(PrimitiveError::InvalidFormat {
                message: format!("Invalid length: {}", value.len()),
            });
        }

        let mut result = Self::new();

        let mut value1_bytes = [0u8; 8];
        let mut value2_bytes = [0u8; 8];
        let mut value3_bytes = [0u8; 8];
        let mut value4_bytes = [0u8; 8];

        value1_bytes.copy_from_slice(&value[0..8]);
        value2_bytes.copy_from_slice(&value[8..16]);
        value3_bytes.copy_from_slice(&value[16..24]);
        value4_bytes.copy_from_slice(&value[24..HASH_SIZE]);

        result.value1 = u64::from_le_bytes(value1_bytes);
        result.value2 = u64::from_le_bytes(value2_bytes);
        result.value3 = u64::from_le_bytes(value3_bytes);
        result.value4 = u64::from_le_bytes(value4_bytes);

        Ok(result)
    }

    /// Creates a new `UInt256` from a byte span with proper error handling.
    ///
    /// # Errors
    /// Returns `PrimitiveError::InvalidFormat` if the input length is not exactly 32 bytes.
    ///
    /// # Example
    /// ```
    /// use neo_primitives::UInt256;
    /// let bytes = [0u8; 32];
    /// let result = UInt256::try_from_span(&bytes);
    /// assert!(result.is_ok());
    /// ```
    pub fn try_from_span(value: &[u8]) -> PrimitiveResult<Self> {
        Self::from_bytes(value)
    }

    /// Creates a new `UInt256` from a byte span (returns zero on invalid input).
    ///
    /// # Deprecated
    /// This method silently returns zero on invalid input, which can mask errors
    /// and lead to security vulnerabilities (e.g., treating invalid block hashes as zero).
    /// Use `try_from_span()` or `from_bytes()` instead for proper error handling.
    #[deprecated(
        since = "0.7.1",
        note = "Use try_from_span() or from_bytes() instead - this method silently returns zero on invalid input which can mask errors"
    )]
    pub fn from_span(value: &[u8]) -> Self {
        match Self::from_bytes(value) {
            Ok(result) => result,
            Err(e) => {
                error!("Invalid UInt256 input: {}", e);
                Self::zero()
            }
        }
    }

    /// Gets a byte array representation of the `UInt256`.
    #[inline]
    #[must_use]
    pub fn to_array(&self) -> [u8; UINT256_SIZE] {
        let mut result = [0u8; UINT256_SIZE];

        let value1_bytes = self.value1.to_le_bytes();
        let value2_bytes = self.value2.to_le_bytes();
        let value3_bytes = self.value3.to_le_bytes();
        let value4_bytes = self.value4.to_le_bytes();

        result[0..8].copy_from_slice(&value1_bytes);
        result[8..16].copy_from_slice(&value2_bytes);
        result[16..24].copy_from_slice(&value3_bytes);
        result[24..HASH_SIZE].copy_from_slice(&value4_bytes);

        result
    }

    /// Gets a span that represents the current value in little-endian.
    #[inline]
    #[must_use]
    pub fn get_span(&self) -> [u8; UINT256_SIZE] {
        self.to_array()
    }

    /// Parses a `UInt256` from a hexadecimal string.
    ///
    /// # Errors
    ///
    /// Returns `PrimitiveError::InvalidFormat` if the input string is not a valid
    /// 64-character hexadecimal string.
    #[inline]
    pub fn parse(s: &str) -> PrimitiveResult<Self> {
        let mut result = None;
        if !Self::try_parse(s, &mut result) {
            return Err(PrimitiveError::InvalidFormat {
                message: "Invalid format".to_string(),
            });
        }

        match result {
            Some(value) => Ok(value),
            None => Err(PrimitiveError::InvalidFormat {
                message: "Failed to parse UInt256".to_string(),
            }),
        }
    }

    /// Tries to parse a `UInt256` from a hexadecimal string.
    pub fn try_parse(s: &str, result: &mut Option<Self>) -> bool {
        let s = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .unwrap_or(s);

        if s.len() != UINT256_SIZE * 2 {
            return false;
        }

        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }

        match hex::decode(s) {
            Ok(mut bytes) => {
                bytes.reverse();

                let mut uint = Self::new();

                let mut value1_bytes = [0u8; 8];
                let mut value2_bytes = [0u8; 8];
                let mut value3_bytes = [0u8; 8];
                let mut value4_bytes = [0u8; 8];

                value1_bytes.copy_from_slice(&bytes[0..8]);
                value2_bytes.copy_from_slice(&bytes[8..16]);
                value3_bytes.copy_from_slice(&bytes[16..24]);
                value4_bytes.copy_from_slice(&bytes[24..HASH_SIZE]);

                uint.value1 = u64::from_le_bytes(value1_bytes);
                uint.value2 = u64::from_le_bytes(value2_bytes);
                uint.value3 = u64::from_le_bytes(value3_bytes);
                uint.value4 = u64::from_le_bytes(value4_bytes);

                *result = Some(uint);

                true
            }
            Err(_) => false,
        }
    }

    /// Converts the `UInt256` to a hexadecimal string.
    #[must_use]
    pub fn to_hex_string(&self) -> String {
        let mut bytes = self.to_array();
        bytes.reverse();
        format!("0x{}", hex::encode(bytes))
    }

    /// Gets a hash code for the current `UInt256` instance.
    ///
    /// # Implementation Note
    /// This method properly combines all 256 bits by `XORing` the high and low
    /// 32-bit parts of each u64 field. This prevents hash collisions that would
    /// occur from only using the lowest 32 bits of value1.
    #[allow(clippy::cast_possible_truncation)]
    #[must_use]
    pub const fn get_hash_code(&self) -> i32 {
        // XOR high and low 32-bit parts of each u64 to preserve all bits
        let v1_hash = (self.value1 as i32) ^ ((self.value1 >> 32) as i32);
        let v2_hash = (self.value2 as i32) ^ ((self.value2 >> 32) as i32);
        let v3_hash = (self.value3 as i32) ^ ((self.value3 >> 32) as i32);
        let v4_hash = (self.value4 as i32) ^ ((self.value4 >> 32) as i32);

        // XOR all parts together for final hash
        v1_hash ^ v2_hash ^ v3_hash ^ v4_hash
    }
}

impl FromStr for UInt256 {
    type Err = PrimitiveError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for UInt256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex_string())
    }
}

impl fmt::Debug for UInt256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UInt256({})", self.to_hex_string())
    }
}

impl PartialOrd for UInt256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UInt256 {
    fn cmp(&self, other: &Self) -> Ordering {
        let result = self.value4.cmp(&other.value4);
        if result != Ordering::Equal {
            return result;
        }

        let result = self.value3.cmp(&other.value3);
        if result != Ordering::Equal {
            return result;
        }

        let result = self.value2.cmp(&other.value2);
        if result != Ordering::Equal {
            return result;
        }

        self.value1.cmp(&other.value1)
    }
}

impl From<[u8; UINT256_SIZE]> for UInt256 {
    fn from(data: [u8; UINT256_SIZE]) -> Self {
        Self::from_bytes(&data).unwrap_or_default()
    }
}

impl TryFrom<&[u8]> for UInt256 {
    type Error = PrimitiveError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(data)
    }
}

impl TryFrom<String> for UInt256 {
    type Error = PrimitiveError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl AsRef<[u8; UINT256_SIZE]> for UInt256 {
    #[inline]
    fn as_ref(&self) -> &[u8; UINT256_SIZE] {
        // SAFETY: UInt256 is repr(C) with four u64 fields that map to 32 bytes.
        // We can safely reinterpret the struct as a byte array.
        // This is safe because:
        // 1. UInt256 is #[derive(Copy, Clone)] and has no padding between fields
        // 2. We're only reading the bytes, not modifying them
        // 3. The layout is well-defined as four little-endian fields
        unsafe { &*(self as *const Self).cast::<[u8; UINT256_SIZE]>() }
    }
}

// NOTE: Serializable implementation moved to neo-io::serializable::primitives
// to keep neo-primitives as a Layer 0 crate with no neo-* dependencies

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_uint256_new() {
        let uint = UInt256::new();
        assert_eq!(uint.value1, 0);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);
    }

    #[test]
    fn test_uint256_from_bytes() {
        let mut data = [0u8; UINT256_SIZE];
        data[0] = 1;
        let uint = UInt256::from_bytes(&data).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        let result = UInt256::from_bytes(&[1u8; UINT256_SIZE - 1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_uint256_to_array() {
        let mut uint = UInt256::new();
        uint.value1 = 1;
        let array = uint.to_array();
        assert_eq!(array[0], 1);
        for &item in array.iter().take(UINT256_SIZE).skip(1) {
            assert_eq!(item, 0);
        }
    }

    #[test]
    fn test_uint256_parse() {
        let mut result = None;
        assert!(UInt256::try_parse(
            "0000000000000000000000000000000000000000000000000000000000000001",
            &mut result
        ));
        assert!(result.is_some());
        let uint = result.unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        let mut result = None;
        assert!(UInt256::try_parse(
            "0X0000000000000000000000000000000000000000000000000000000000000001",
            &mut result
        ));
        assert!(result.is_some());
        let uint = result.unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        let result = UInt256::parse("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_uint256_to_hex_string() {
        let mut uint = UInt256::new();
        uint.value1 = 1;
        assert_eq!(
            uint.to_hex_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn test_uint256_ordering() {
        let mut uint1 = UInt256::new();
        uint1.value4 = 1;
        let mut uint2 = UInt256::new();
        uint2.value4 = 2;
        assert!(uint1 < uint2);
    }

    #[test]
    fn test_uint256_equals() {
        let mut uint1 = UInt256::new();
        uint1.value1 = 1;
        uint1.value2 = 2;
        uint1.value3 = 3;
        uint1.value4 = 4;

        let mut uint2 = UInt256::new();
        uint2.value1 = 1;
        uint2.value2 = 2;
        uint2.value3 = 3;
        uint2.value4 = 4;

        let mut uint3 = UInt256::new();
        uint3.value1 = 5;

        assert!(uint1.equals(Some(&uint2)));
        assert!(!uint1.equals(Some(&uint3)));
        assert!(!uint1.equals(None));
    }

    // Property-based tests using proptest
    proptest! {
        #[test]
        fn test_roundtrip_from_bytes(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            let result = uint.to_array();
            prop_assert_eq!(bytes, result);
        }

        #[test]
        fn test_parse_hex_string(hex in "[0-9a-fA-F]{64}") {
            // Test that parsing is deterministic
            let uint = UInt256::parse(&format!("0x{}", hex)).unwrap();
            // Converting to hex string and re-parsing should give same Display representation
            let hex_str = uint.to_hex_string();
            let uint2 = UInt256::parse(&hex_str).unwrap();
            prop_assert_eq!(uint, uint2);
        }

        #[test]
        fn test_ordering_transitive(
            a in any::<[u8; UINT256_SIZE]>(),
            b in any::<[u8; UINT256_SIZE]>(),
            c in any::<[u8; UINT256_SIZE]>()
        ) {
            let a = UInt256::from_bytes(&a).unwrap();
            let b = UInt256::from_bytes(&b).unwrap();
            let c = UInt256::from_bytes(&c).unwrap();

            // Test transitivity of ordering
            if a < b && b < c {
                prop_assert!(a < c);
            }
            if a > b && b > c {
                prop_assert!(a > c);
            }
        }

        #[test]
        fn test_is_zero_correct(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            let is_zero = bytes.iter().all(|&b| b == 0);
            prop_assert_eq!(uint.is_zero(), is_zero);
        }

        #[test]
        fn test_as_ref_implementation(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            let ref_bytes: &[u8] = uint.as_ref();
            prop_assert_eq!(&bytes, ref_bytes);
        }

        #[test]
        fn test_get_hash_code_deterministic(bytes in any::<[u8; UINT256_SIZE]>()) {
            let uint = UInt256::from_bytes(&bytes).unwrap();
            let hash1 = uint.get_hash_code();
            let hash2 = uint.get_hash_code();
            prop_assert_eq!(hash1, hash2);
        }

        #[test]
        fn test_equals_is_symmetric(
            a in any::<[u8; UINT256_SIZE]>(),
            b in any::<[u8; UINT256_SIZE]>()
        ) {
            let uint_a = UInt256::from_bytes(&a).unwrap();
            let uint_b = UInt256::from_bytes(&b).unwrap();
            prop_assert_eq!(uint_a.equals(Some(&uint_b)), uint_b.equals(Some(&uint_a)));
        }

        #[test]
        fn test_parse_with_0x_prefix(hex in "[0-9a-fA-F]{64}") {
            let with_prefix = UInt256::parse(&format!("0x{}", hex)).unwrap();
            let without_prefix = UInt256::parse(&hex).unwrap();
            prop_assert_eq!(with_prefix, without_prefix);
        }
    }
}
