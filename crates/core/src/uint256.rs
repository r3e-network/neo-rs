// Copyright (C) 2015-2025 The Neo Project.
//
// uint256.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Implementation of UInt256, a 256-bit unsigned integer.

use crate::CoreError;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// The length of UInt256 values in bytes.
pub const UINT256_SIZE: usize = 32;

/// Represents a 256-bit unsigned integer.
///
/// This is implemented as a reference type to match the C# implementation.
#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct UInt256 {
    /// First 8 bytes of the UInt256 (least significant).
    pub value1: u64,
    /// Next 8 bytes of the UInt256.
    pub value2: u64,
    /// Next 8 bytes of the UInt256.
    pub value3: u64,
    /// Last 8 bytes of the UInt256 (most significant).
    pub value4: u64,
}

/// Zero value for UInt256.
pub static ZERO: UInt256 = UInt256 {
    value1: 0,
    value2: 0,
    value3: 0,
    value4: 0,
};

impl UInt256 {
    /// Creates a new UInt256 instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a zero UInt256.
    pub fn zero() -> Self {
        Self::default()
    }

    /// Checks if this UInt256 is zero.
    pub fn is_zero(&self) -> bool {
        self.value1 == 0 && self.value2 == 0 && self.value3 == 0 && self.value4 == 0
    }

    /// Returns the bytes representation of this UInt256.
    pub fn as_bytes(&self) -> &[u8; 32] {
        unsafe { std::mem::transmute(self) }
    }

    /// Determines whether this instance and another specified UInt256 object have the same value.
    ///
    /// # Arguments
    ///
    /// * `other` - The UInt256 to compare to this instance.
    ///
    /// # Returns
    ///
    /// true if the value of the value parameter is the same as this instance; otherwise, false.
    pub fn equals(&self, other: Option<&Self>) -> bool {
        if let Some(other) = other {
            self.value1 == other.value1
                && self.value2 == other.value2
                && self.value3 == other.value3
                && self.value4 == other.value4
        } else {
            false
        }
    }

    /// Creates a new UInt256 from a byte array.
    ///
    /// # Arguments
    ///
    /// * `value` - The byte array to create the UInt256 from.
    ///
    /// # Returns
    ///
    /// A new UInt256 instance.
    pub fn from_bytes(value: &[u8]) -> Result<Self, CoreError> {
        if value.len() != UINT256_SIZE {
            return Err(CoreError::InvalidFormat {
                message: format!("Invalid length: {}", value.len()),
            });
        }

        let mut result = Self::new();

        // Convert bytes to u64 values to match C# implementation
        let mut value1_bytes = [0u8; 8];
        let mut value2_bytes = [0u8; 8];
        let mut value3_bytes = [0u8; 8];
        let mut value4_bytes = [0u8; 8];

        // In the integration test, we're creating a byte array with the first byte set to 1
        // This means we need to set value1 to 1 to match
        value1_bytes.copy_from_slice(&value[0..8]);
        value2_bytes.copy_from_slice(&value[8..16]);
        value3_bytes.copy_from_slice(&value[16..24]);
        value4_bytes.copy_from_slice(&value[24..32]);

        result.value1 = u64::from_le_bytes(value1_bytes);
        result.value2 = u64::from_le_bytes(value2_bytes);
        result.value3 = u64::from_le_bytes(value3_bytes);
        result.value4 = u64::from_le_bytes(value4_bytes);

        Ok(result)
    }

    /// Creates a new UInt256 from a byte span.
    ///
    /// # Arguments
    ///
    /// * `value` - The byte span to create the UInt256 from.
    ///
    /// # Returns
    ///
    /// A new UInt256 instance.
    pub fn from_span(value: &[u8]) -> Self {
        if value.len() != UINT256_SIZE {
            panic!("Invalid length: {}", value.len());
        }

        let mut result = Self::new();

        // Convert bytes to u64 values to match C# implementation
        let mut value1_bytes = [0u8; 8];
        let mut value2_bytes = [0u8; 8];
        let mut value3_bytes = [0u8; 8];
        let mut value4_bytes = [0u8; 8];

        value1_bytes.copy_from_slice(&value[0..8]);
        value2_bytes.copy_from_slice(&value[8..16]);
        value3_bytes.copy_from_slice(&value[16..24]);
        value4_bytes.copy_from_slice(&value[24..32]);

        result.value1 = u64::from_le_bytes(value1_bytes);
        result.value2 = u64::from_le_bytes(value2_bytes);
        result.value3 = u64::from_le_bytes(value3_bytes);
        result.value4 = u64::from_le_bytes(value4_bytes);

        result
    }

    /// Gets a byte array representation of the UInt256.
    ///
    /// # Returns
    ///
    /// A byte array representation of the UInt256.
    pub fn to_array(&self) -> [u8; UINT256_SIZE] {
        let mut result = [0u8; UINT256_SIZE];

        let value1_bytes = self.value1.to_le_bytes();
        let value2_bytes = self.value2.to_le_bytes();
        let value3_bytes = self.value3.to_le_bytes();
        let value4_bytes = self.value4.to_le_bytes();

        result[0..8].copy_from_slice(&value1_bytes);
        result[8..16].copy_from_slice(&value2_bytes);
        result[16..24].copy_from_slice(&value3_bytes);
        result[24..32].copy_from_slice(&value4_bytes);

        result
    }

    /// Gets a span that represents the current value in little-endian.
    ///
    /// # Returns
    ///
    /// A byte array that represents the current value in little-endian.
    pub fn get_span(&self) -> [u8; UINT256_SIZE] {
        self.to_array()
    }

    /// Parses a UInt256 from a hexadecimal string.
    ///
    /// # Arguments
    ///
    /// * `s` - The hexadecimal string to parse.
    ///
    /// # Returns
    ///
    /// A Result containing either a new UInt256 instance or an error.
    pub fn parse(s: &str) -> Result<Self, CoreError> {
        let mut result = None;
        if !Self::try_parse(s, &mut result) {
            return Err(CoreError::InvalidFormat {
                message: "Invalid format".to_string(),
            });
        }

        match result {
            Some(value) => Ok(value),
            None => Err(CoreError::InvalidFormat {
                message: "Failed to parse UInt256".to_string(),
            }),
        }
    }

    /// Tries to parse a UInt256 from a hexadecimal string.
    ///
    /// # Arguments
    ///
    /// * `s` - The hexadecimal string to parse.
    /// * `result` - Optional mutable reference to store the result.
    ///
    /// # Returns
    ///
    /// A boolean indicating whether the parsing was successful.
    pub fn try_parse(s: &str, result: &mut Option<Self>) -> bool {
        let s = s.strip_prefix("0x").unwrap_or(s);

        if s.len() != UINT256_SIZE * 2 {
            return false;
        }

        // Check if all characters are valid hex
        if !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return false;
        }

        // Try to parse the hex string to bytes
        match hex::decode(s) {
            Ok(mut bytes) => {
                // Reverse the bytes to match C# HexToBytesReversed
                bytes.reverse();

                // Create a new UInt256 from the bytes
                let mut uint = Self::new();

                // Convert bytes to u64 values
                let mut value1_bytes = [0u8; 8];
                let mut value2_bytes = [0u8; 8];
                let mut value3_bytes = [0u8; 8];
                let mut value4_bytes = [0u8; 8];

                value1_bytes.copy_from_slice(&bytes[0..8]);
                value2_bytes.copy_from_slice(&bytes[8..16]);
                value3_bytes.copy_from_slice(&bytes[16..24]);
                value4_bytes.copy_from_slice(&bytes[24..32]);

                uint.value1 = u64::from_le_bytes(value1_bytes);
                uint.value2 = u64::from_le_bytes(value2_bytes);
                uint.value3 = u64::from_le_bytes(value3_bytes);
                uint.value4 = u64::from_le_bytes(value4_bytes);

                // Always set the result
                *result = Some(uint);

                true
            }
            Err(_) => false,
        }
    }

    /// Converts the UInt256 to a hexadecimal string.
    ///
    /// # Returns
    ///
    /// A hexadecimal string representation of the UInt256.
    pub fn to_hex_string(&self) -> String {
        let mut bytes = self.to_array();
        bytes.reverse(); // Reverse to match C# ToHexString behavior
        format!("0x{}", hex::encode(bytes))
    }

    /// Gets a hash code for the current UInt256 instance.
    ///
    /// # Returns
    ///
    /// A 32-bit signed integer hash code.
    pub fn get_hash_code(&self) -> i32 {
        // In C# implementation, it just returns (int)value1
        self.value1 as i32
    }
}

impl Serializable for UInt256 {
    fn size(&self) -> usize {
        UINT256_SIZE
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> neo_io::IoResult<()> {
        writer.write_u64(self.value1)?;
        writer.write_u64(self.value2)?;
        writer.write_u64(self.value3)?;
        writer.write_u64(self.value4)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> neo_io::IoResult<Self> {
        let value1 = reader.read_u64()?;
        let value2 = reader.read_u64()?;
        let value3 = reader.read_u64()?;
        let value4 = reader.read_u64()?;
        Ok(Self {
            value1,
            value2,
            value3,
            value4,
        })
    }
}

impl FromStr for UInt256 {
    type Err = CoreError;

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
        // Compare in the same order as C#: value4, value3, value2, value1
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
    type Error = CoreError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        Self::from_bytes(data)
    }
}

// Implicit conversion from string
impl From<&str> for UInt256 {
    fn from(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }
}

// Implicit conversion from byte array
impl From<Vec<u8>> for UInt256 {
    fn from(data: Vec<u8>) -> Self {
        Self::from_bytes(&data).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for i in 1..UINT256_SIZE {
            assert_eq!(array[i], 0);
        }
    }

    #[test]
    fn test_uint256_parse() {
        // Create a UInt256 with value1 = 1
        let mut expected = UInt256::new();
        expected.value1 = 1;

        // Parse a hex string
        let mut result = None;
        assert!(UInt256::try_parse(
            "0000000000000000000000000000000000000000000000000000000000000001",
            &mut result
        ));
        assert!(result.is_some());

        // Compare the parsed value with the expected value
        let uint = result.unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
        assert_eq!(uint.value4, 0);

        // Test invalid input
        let result = UInt256::parse("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_uint256_try_parse() {
        let mut uint1 = UInt256::new();
        uint1.value1 = 1;

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

        assert!(!UInt256::try_parse("invalid", &mut None));
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
    fn test_uint256_serialization() {
        let mut uint = UInt256::new();
        uint.value1 = 1;
        uint.value2 = 2;
        uint.value3 = 3;
        uint.value4 = 4;

        // Test serialization
        let mut writer = BinaryWriter::new();
        <UInt256 as Serializable>::serialize(&uint, &mut writer).unwrap();
        let bytes = writer.to_bytes();

        // Test deserialization
        let mut reader = MemoryReader::new(&bytes);
        let deserialized = <UInt256 as Serializable>::deserialize(&mut reader).unwrap();

        assert_eq!(uint, deserialized);
    }

    #[test]
    fn test_uint256_ordering() {
        let mut uint1 = UInt256::new();
        uint1.value4 = 1;

        let mut uint2 = UInt256::new();
        uint2.value4 = 2;

        assert!(uint1 < uint2);

        let mut uint3 = UInt256::new();
        uint3.value3 = 1;

        let mut uint4 = UInt256::new();
        uint4.value3 = 2;

        assert!(uint3 < uint4);

        let mut uint5 = UInt256::new();
        uint5.value2 = 1;

        let mut uint6 = UInt256::new();
        uint6.value2 = 2;

        assert!(uint5 < uint6);

        let mut uint7 = UInt256::new();
        uint7.value1 = 1;

        let mut uint8 = UInt256::new();
        uint8.value1 = 2;

        assert!(uint7 < uint8);
    }

    #[test]
    fn test_uint256_from_string() {
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
        uint3.value2 = 6;
        uint3.value3 = 7;
        uint3.value4 = 8;

        assert!(uint1.equals(Some(&uint2)));
        assert!(!uint1.equals(Some(&uint3)));
        assert!(!uint1.equals(None));
    }

    #[test]
    fn test_uint256_get_hash_code() {
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
        uint3.value2 = 6;
        uint3.value3 = 7;
        uint3.value4 = 8;

        // Equal objects should have equal hash codes
        assert_eq!(uint1.get_hash_code(), uint2.get_hash_code());

        // Different objects with different value1 should have different hash codes
        assert_ne!(uint1.get_hash_code(), uint3.get_hash_code());

        // Objects with same value1 but different other values should have same hash code
        let mut uint4 = UInt256::new();
        uint4.value1 = 1;
        uint4.value2 = 10;
        uint4.value3 = 11;
        uint4.value4 = 12;

        assert_eq!(uint1.get_hash_code(), uint4.get_hash_code());
    }
}
