//! Implementation of UInt160, a 160-bit unsigned integer.

use crate::constants::ADDRESS_SIZE;
use crate::error::{PrimitiveError, PrimitiveResult};
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use tracing::error;

/// The length of UInt160 values in bytes.
pub const UINT160_SIZE: usize = ADDRESS_SIZE;

/// Represents a 160-bit unsigned integer.
///
/// This is implemented as a reference type to match the C# implementation.
#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct UInt160 {
    /// First 8 bytes of the UInt160 (least significant).
    pub value1: u64,
    /// Next 8 bytes of the UInt160.
    pub value2: u64,
    /// Last 4 bytes of the UInt160 (most significant).
    pub value3: u32,
}

/// Zero value for UInt160.
pub static ZERO: UInt160 = UInt160 {
    value1: 0,
    value2: 0,
    value3: 0,
};

impl UInt160 {
    /// Alias matching C# `UInt160.Length`.
    pub const LENGTH: usize = UINT160_SIZE;

    /// Creates a new UInt160 instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a zero UInt160.
    pub fn zero() -> Self {
        Self::default()
    }

    /// Checks if this UInt160 is zero (matches C# IsZero property).
    pub fn is_zero(&self) -> bool {
        self.value1 == 0 && self.value2 == 0 && self.value3 == 0
    }

    /// Returns the bytes representation of this UInt160.
    pub fn as_bytes(&self) -> [u8; ADDRESS_SIZE] {
        self.to_array()
    }

    /// Returns the bytes as a Vec<u8>
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(ADDRESS_SIZE);
        bytes.extend_from_slice(&self.value1.to_le_bytes());
        bytes.extend_from_slice(&self.value2.to_le_bytes());
        bytes.extend_from_slice(&self.value3.to_le_bytes());
        bytes
    }

    /// Determines whether this instance and another specified UInt160 object have the same value.
    pub fn equals(&self, other: Option<&Self>) -> bool {
        if let Some(other) = other {
            self.value1 == other.value1
                && self.value2 == other.value2
                && self.value3 == other.value3
        } else {
            false
        }
    }

    /// Creates a new UInt160 from a byte array.
    pub fn from_bytes(value: &[u8]) -> PrimitiveResult<Self> {
        if value.len() != UINT160_SIZE {
            return Err(PrimitiveError::InvalidFormat {
                message: format!("Invalid length: {}", value.len()),
            });
        }

        let mut result = Self::new();

        let mut value1_bytes = [0u8; 8];
        let mut value2_bytes = [0u8; 8];
        let mut value3_bytes = [0u8; 4];

        value1_bytes.copy_from_slice(&value[0..8]);
        value2_bytes.copy_from_slice(&value[8..16]);
        value3_bytes.copy_from_slice(&value[16..ADDRESS_SIZE]);

        result.value1 = u64::from_le_bytes(value1_bytes);
        result.value2 = u64::from_le_bytes(value2_bytes);
        result.value3 = u32::from_le_bytes(value3_bytes);

        Ok(result)
    }

    /// Creates a new UInt160 from a byte span with proper error handling.
    ///
    /// # Errors
    /// Returns `PrimitiveError::InvalidFormat` if the input length is not exactly 20 bytes.
    ///
    /// # Example
    /// ```
    /// use neo_primitives::UInt160;
    /// let bytes = [0u8; 20];
    /// let result = UInt160::try_from_span(&bytes);
    /// assert!(result.is_ok());
    /// ```
    pub fn try_from_span(value: &[u8]) -> PrimitiveResult<Self> {
        Self::from_bytes(value)
    }

    /// Creates a new UInt160 from a byte span (returns zero on invalid input).
    ///
    /// # Deprecated
    /// This method silently returns zero on invalid input, which can mask errors
    /// and lead to security vulnerabilities (e.g., treating invalid script hashes as zero).
    /// Use `try_from_span()` or `from_bytes()` instead for proper error handling.
    #[deprecated(
        since = "0.7.1",
        note = "Use try_from_span() or from_bytes() instead - this method silently returns zero on invalid input which can mask errors"
    )]
    pub fn from_span(value: &[u8]) -> Self {
        match Self::from_bytes(value) {
            Ok(result) => result,
            Err(e) => {
                error!("Invalid UInt160 input: {}", e);
                Self::zero()
            }
        }
    }

    /// Gets a byte array representation of the UInt160.
    pub fn to_array(&self) -> [u8; UINT160_SIZE] {
        let mut result = [0u8; UINT160_SIZE];

        let value1_bytes = self.value1.to_le_bytes();
        let value2_bytes = self.value2.to_le_bytes();
        let value3_bytes = self.value3.to_le_bytes();

        result[0..8].copy_from_slice(&value1_bytes);
        result[8..16].copy_from_slice(&value2_bytes);
        result[16..ADDRESS_SIZE].copy_from_slice(&value3_bytes);

        result
    }

    /// Gets a span that represents the current value in little-endian.
    pub fn get_span(&self) -> [u8; UINT160_SIZE] {
        self.to_array()
    }

    /// Parses a UInt160 from a hexadecimal string.
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
                message: "Failed to parse UInt160".to_string(),
            }),
        }
    }

    /// Tries to parse a UInt160 from a hexadecimal string.
    pub fn try_parse(s: &str, result: &mut Option<Self>) -> bool {
        let s = s.strip_prefix("0x").unwrap_or(s);

        if s.len() != UINT160_SIZE * 2 {
            return false;
        }

        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }

        let Ok(mut bytes) = hex::decode(s) else {
            return false;
        };

        bytes.reverse();

        match Self::from_bytes(&bytes) {
            Ok(uint) => {
                *result = Some(uint);
                true
            }
            Err(_) => false,
        }
    }

    /// Converts the UInt160 to a hexadecimal string.
    pub fn to_hex_string(&self) -> String {
        let mut bytes = self.to_array();
        bytes.reverse();
        format!("0x{}", hex::encode(bytes))
    }

    /// Gets a hash code for the current UInt160 instance.
    ///
    /// # Implementation Note
    /// This method properly combines all 160 bits by XORing the high and low
    /// 32-bit parts of each u64 field before combining. This prevents hash
    /// collisions that would occur from simple truncation.
    pub fn get_hash_code(&self) -> i32 {
        // XOR high and low 32-bit parts of each u64 to preserve all bits
        let v1_hash = (self.value1 as i32) ^ ((self.value1 >> 32) as i32);
        let v2_hash = (self.value2 as i32) ^ ((self.value2 >> 32) as i32);
        let v3_hash = self.value3 as i32;

        // Combine using prime multiplication with wrapping arithmetic
        let mut hash = 17i32;
        hash = hash.wrapping_mul(31).wrapping_add(v1_hash);
        hash = hash.wrapping_mul(31).wrapping_add(v2_hash);
        hash = hash.wrapping_mul(31).wrapping_add(v3_hash);
        hash
    }

    /// Creates a UInt160 from a script by computing its hash.
    pub fn from_script(script: &[u8]) -> Self {
        let mut sha256_hasher = Sha256::new();
        sha256_hasher.update(script);
        let sha256_hash = sha256_hasher.finalize();

        let mut ripemd_hasher = Ripemd160::new();
        ripemd_hasher.update(sha256_hash);
        let hash160 = ripemd_hasher.finalize();

        Self::from_bytes(&hash160).unwrap_or_default()
    }

    /// Converts this UInt160 to a Neo address string.
    pub fn to_address(&self) -> String {
        let version_byte = 0x35u8; // Neo N3 address version
        let mut data = Vec::with_capacity(21);
        data.push(version_byte);
        data.extend_from_slice(&self.to_array());

        let mut hasher = Sha256::new();
        hasher.update(&data);
        let first_hash = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(first_hash);
        let second_hash = hasher.finalize();

        let checksum = &second_hash[0..4];
        data.extend_from_slice(checksum);

        bs58::encode(data).into_string()
    }

    /// Parses a Neo address string to a UInt160.
    pub fn from_address(address: &str) -> PrimitiveResult<Self> {
        let decoded =
            bs58::decode(address)
                .into_vec()
                .map_err(|_| PrimitiveError::InvalidFormat {
                    message: "Invalid Base58 address".to_string(),
                })?;

        if decoded.len() != 25 {
            return Err(PrimitiveError::InvalidFormat {
                message: "Invalid address length".to_string(),
            });
        }

        if decoded[0] != 0x35 {
            return Err(PrimitiveError::InvalidFormat {
                message: "Invalid address version".to_string(),
            });
        }

        let data = &decoded[0..21];
        let checksum = &decoded[21..25];

        let mut hasher = Sha256::new();
        hasher.update(data);
        let first_hash = hasher.finalize();

        let mut hasher = Sha256::new();
        hasher.update(first_hash);
        let second_hash = hasher.finalize();

        let computed_checksum = &second_hash[0..4];
        if checksum != computed_checksum {
            return Err(PrimitiveError::InvalidFormat {
                message: "Invalid address checksum".to_string(),
            });
        }

        let script_hash = &decoded[1..21];
        Self::from_bytes(script_hash)
    }
}

impl FromStr for UInt160 {
    type Err = PrimitiveError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for UInt160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex_string())
    }
}

impl fmt::Debug for UInt160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UInt160({})", self.to_hex_string())
    }
}

impl PartialOrd for UInt160 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UInt160 {
    fn cmp(&self, other: &Self) -> Ordering {
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

impl From<[u8; UINT160_SIZE]> for UInt160 {
    fn from(data: [u8; UINT160_SIZE]) -> Self {
        Self::from_bytes(&data).expect("Fixed-size array should always be valid")
    }
}

impl TryFrom<&[u8]> for UInt160 {
    type Error = PrimitiveError;

    fn try_from(data: &[u8]) -> std::result::Result<Self, Self::Error> {
        Self::from_bytes(data)
    }
}

/// **DEPRECATED**: Use `FromStr` trait (via `str::parse()`) instead for proper error handling.
///
/// This implementation silently returns zero on parse failure, which can mask errors.
/// Prefer using `str::parse::<UInt160>()` or `UInt160::parse()` instead.
impl From<&str> for UInt160 {
    fn from(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }
}

impl TryFrom<String> for UInt160 {
    type Error = PrimitiveError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

/// **DEPRECATED**: Use `TryFrom<&[u8]>` or `from_bytes()` instead for proper error handling.
///
/// This implementation silently returns zero on invalid input, which can mask errors.
/// Prefer using `UInt160::from_bytes()` or `TryFrom<&[u8]>` instead.
impl From<Vec<u8>> for UInt160 {
    fn from(data: Vec<u8>) -> Self {
        Self::from_bytes(&data).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint160_new() {
        let uint = UInt160::new();
        assert_eq!(uint.value1, 0);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_from_bytes() {
        let mut bytes = [0u8; ADDRESS_SIZE];
        bytes[0] = 1;
        let uint = UInt160::from_bytes(&bytes).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_to_array() {
        let mut uint = UInt160::new();
        uint.value1 = 1;
        let bytes = uint.to_array();
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 0);
    }

    #[test]
    fn test_uint160_parse() {
        let hex_str = "0x0000000000000000000000000000000000000001";
        let uint = UInt160::parse(hex_str).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_to_hex_string() {
        let mut uint = UInt160::new();
        uint.value3 = 0x01000000;
        let hex_str = uint.to_hex_string();
        assert_eq!(hex_str, "0x0100000000000000000000000000000000000000");
    }

    #[test]
    fn test_uint160_ordering() {
        let uint1 = UInt160 {
            value1: 1,
            value2: 0,
            value3: 0,
        };
        let uint2 = UInt160 {
            value1: 0,
            value2: 1,
            value3: 0,
        };
        let uint3 = UInt160 {
            value1: 0,
            value2: 0,
            value3: 1,
        };
        assert!(uint3 > uint2);
        assert!(uint2 > uint1);
        assert!(uint3 > uint1);
    }

    #[test]
    fn test_uint160_equals() {
        let uint1 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 3,
        };
        let uint2 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 3,
        };
        let uint3 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 4,
        };
        assert!(uint1.equals(Some(&uint2)));
        assert!(!uint1.equals(Some(&uint3)));
        assert!(!uint1.equals(None));
    }

    #[test]
    fn test_uint160_from_script() {
        let script = b"Hello, Neo!";
        let uint = UInt160::from_script(script);
        assert!(!uint.is_zero());
    }
}
