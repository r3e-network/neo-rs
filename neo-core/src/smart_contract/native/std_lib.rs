//! StdLib native contract implementation.
//!
//! The StdLib contract provides standard utility functions for smart contracts,
//! including string manipulation, JSON operations, and mathematical functions.

use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::ContractParameterType;
use crate::UInt160;
use base64::{engine::general_purpose, Engine as _};
use serde_json::Value as JsonValue;
use std::str::FromStr;
use unicode_segmentation::UnicodeSegmentation;

/// The StdLib native contract.
pub struct StdLib {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl StdLib {
    const ID: i32 = -2;

    /// Creates a new StdLib contract.
    pub fn new() -> Self {
        // StdLib contract hash: 0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0
        let hash = UInt160::from_bytes(&[
            0xac, 0xce, 0x6f, 0xd8, 0x0d, 0x44, 0xe1, 0x79, 0x6a, 0xa0, 0xc2, 0xc6, 0x25, 0xe9,
            0xe4, 0xe0, 0xce, 0x39, 0xef, 0xc0,
        ])
        .expect("Operation failed");

        let methods = vec![
            NativeMethod::safe(
                "atoi".to_string(),
                1 << 12,
                vec![ContractParameterType::String],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "itoa".to_string(),
                1 << 12,
                vec![ContractParameterType::Integer],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base64Encode".to_string(),
                1 << 12,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base64Decode".to_string(),
                1 << 12,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            // JSON operations
            NativeMethod::safe(
                "jsonSerialize".to_string(),
                1 << 12,
                vec![ContractParameterType::Any],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "jsonDeserialize".to_string(),
                1 << 12,
                vec![ContractParameterType::String],
                ContractParameterType::Any,
            ),
            // Memory operations
            NativeMethod::safe(
                "memoryCompare".to_string(),
                1 << 5,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (2 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (3 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Integer,
            ),
            // memorySearch overloads (4 params)
            NativeMethod::safe(
                "memorySearch".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::ByteArray,
                    ContractParameterType::ByteArray,
                    ContractParameterType::Integer,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Integer,
            ),
            // stringSplit overloads (2 params)
            NativeMethod::safe(
                "stringSplit".to_string(),
                1 << 8,
                vec![ContractParameterType::String, ContractParameterType::String],
                ContractParameterType::Array,
            ),
            // stringSplit overloads (3 params)
            NativeMethod::safe(
                "stringSplit".to_string(),
                1 << 8,
                vec![
                    ContractParameterType::String,
                    ContractParameterType::String,
                    ContractParameterType::Boolean,
                ],
                ContractParameterType::Array,
            ),
            NativeMethod::safe(
                "strLen".to_string(),
                1 << 8,
                vec![ContractParameterType::String],
                ContractParameterType::Integer,
            ),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Invokes a method on the StdLib contract.
    pub fn invoke_method(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "atoi" => self.atoi(args),
            "itoa" => self.itoa(args),
            "base64Encode" => self.base64_encode(args),
            "base64Decode" => self.base64_decode(args),
            "jsonSerialize" => self.json_serialize(args),
            "jsonDeserialize" => self.json_deserialize(args),
            "memoryCompare" => self.memory_compare(args),
            "memorySearch" => self.memory_search(args),
            "stringSplit" => self.string_split(args),
            "strLen" => self.str_len(args),
            // Legacy alias for backward compatibility
            "stringLen" => self.str_len(args),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    /// Converts a string to an integer.
    fn atoi(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "atoi requires string argument".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string".to_string()))?;

        let number = i64::from_str(&string_data)
            .map_err(|_| Error::native_contract("Invalid number format".to_string()))?;

        Ok(number.to_le_bytes().to_vec())
    }

    /// Converts an integer to a string.
    fn itoa(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() || args[0].len() != 8 {
            return Err(Error::native_contract(
                "itoa requires integer argument".to_string(),
            ));
        }

        let number = i64::from_le_bytes([
            args[0][0], args[0][1], args[0][2], args[0][3], args[0][4], args[0][5], args[0][6],
            args[0][7],
        ]);

        Ok(number.to_string().into_bytes())
    }

    /// Encodes data to base64.
    fn base64_encode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Encode requires data argument".to_string(),
            ));
        }

        let encoded = general_purpose::STANDARD.encode(&args[0]);
        Ok(encoded.into_bytes())
    }

    /// Decodes data from base64.
    fn base64_decode(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "base64Decode requires string argument".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string".to_string()))?;

        let decoded = general_purpose::STANDARD
            .decode(&string_data)
            .map_err(|_| Error::native_contract("Invalid base64 data".to_string()))?;

        Ok(decoded)
    }

    /// Serializes data to JSON.
    fn json_serialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonSerialize requires data argument".to_string(),
            ));
        }

        let string_data =
            String::from_utf8(args[0].clone()).unwrap_or_else(|_| hex::encode(&args[0]));

        let json_value = JsonValue::String(string_data);
        let json_string = serde_json::to_string(&json_value)
            .map_err(|e| Error::native_contract(format!("JSON serialization error: {}", e)))?;

        Ok(json_string.into_bytes())
    }

    /// Deserializes data from JSON.
    fn json_deserialize(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonDeserialize requires JSON string argument".to_string(),
            ));
        }

        let json_string = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string".to_string()))?;

        let json_value: JsonValue = serde_json::from_str(&json_string)
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {}", e)))?;

        // Convert JSON value back to bytes
        match json_value {
            JsonValue::String(s) => Ok(s.into_bytes()),
            JsonValue::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(i.to_le_bytes().to_vec())
                } else {
                    Ok(n.to_string().into_bytes())
                }
            }
            JsonValue::Bool(b) => Ok(vec![if b { 1 } else { 0 }]),
            JsonValue::Null => Ok(vec![]),
            _ => Ok(json_value.to_string().into_bytes()),
        }
    }

    /// Compares two memory regions.
    fn memory_compare(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memoryCompare requires two data arguments".to_string(),
            ));
        }

        let result = match args[0].cmp(&args[1]) {
            std::cmp::Ordering::Less => -1i32,
            std::cmp::Ordering::Equal => 0i32,
            std::cmp::Ordering::Greater => 1i32,
        };

        Ok(result.to_le_bytes().to_vec())
    }

    /// Searches for a pattern in memory.
    /// Supports 3 overloads:
    /// - memorySearch(mem, value) -> searches from start, forward
    /// - memorySearch(mem, value, start) -> searches from start index, forward
    /// - memorySearch(mem, value, start, backward) -> searches with direction control
    fn memory_search(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memorySearch requires at least 2 arguments (mem, value)".to_string(),
            ));
        }

        let mem = &args[0];
        let value = &args[1];

        // Parse optional start parameter (default: 0)
        let start = if args.len() >= 3 {
            if args[2].len() != 4 {
                return Err(Error::native_contract(
                    "start parameter must be a 4-byte integer".to_string(),
                ));
            }
            i32::from_le_bytes([args[2][0], args[2][1], args[2][2], args[2][3]])
        } else {
            0
        };

        // Parse optional backward parameter (default: false)
        let backward = if args.len() >= 4 {
            if args[3].is_empty() {
                false
            } else {
                args[3][0] != 0
            }
        } else {
            false
        };

        // Validate start index
        if start < 0 || start as usize > mem.len() {
            return Err(Error::native_contract(format!(
                "start index {} out of range [0, {}]",
                start,
                mem.len()
            )));
        }

        let start_usize = start as usize;

        // Handle empty pattern
        if value.is_empty() {
            return Ok(start.to_le_bytes().to_vec());
        }

        let result = if backward {
            // Backward search: search in mem[0..start] from end to beginning
            if start_usize < value.len() {
                -1
            } else {
                let search_range = &mem[0..start_usize];
                match search_range
                    .windows(value.len())
                    .rposition(|window| window == value)
                {
                    Some(pos) => pos as i32,
                    None => -1,
                }
            }
        } else {
            // Forward search: search in mem[start..] from beginning to end
            if start_usize + value.len() > mem.len() {
                -1
            } else {
                let search_range = &mem[start_usize..];
                match search_range
                    .windows(value.len())
                    .position(|window| window == value)
                {
                    Some(pos) => (start_usize + pos) as i32,
                    None => -1,
                }
            }
        };

        Ok(result.to_le_bytes().to_vec())
    }

    /// Splits a string by a delimiter.
    /// Supports 2 overloads:
    /// - stringSplit(str, separator) -> splits string, keeps empty entries
    /// - stringSplit(str, separator, removeEmptyEntries) -> splits with option to remove empty entries
    fn string_split(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "stringSplit requires at least 2 arguments (str, separator)".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string".to_string()))?;

        let separator = String::from_utf8(args[1].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 separator".to_string()))?;

        // Parse optional removeEmptyEntries parameter (default: false)
        let remove_empty_entries = if args.len() >= 3 {
            if args[2].is_empty() {
                false
            } else {
                args[2][0] != 0
            }
        } else {
            false
        };

        // Split the string
        let parts: Vec<&str> = if remove_empty_entries {
            string_data
                .split(&separator)
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            string_data.split(&separator).collect()
        };

        // Serialize as a simple array format: [count][length1][data1][length2][data2]...
        let mut result = Vec::new();
        result.extend_from_slice(&(parts.len() as u32).to_le_bytes());

        for part in parts {
            let part_bytes = part.as_bytes();
            result.extend_from_slice(&(part_bytes.len() as u32).to_le_bytes());
            result.extend_from_slice(part_bytes);
        }

        Ok(result)
    }

    /// Gets the length of a string in grapheme clusters (text elements).
    /// This matches C#'s TextElementEnumerator behavior, correctly counting
    /// complex Unicode characters like emojis as single elements.
    /// For example: "🦆" = 1, "ã" = 1
    fn str_len(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "strLen requires string argument".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string".to_string()))?;

        // Count grapheme clusters (extended grapheme clusters) to match C# TextElementEnumerator
        let length = string_data.graphemes(true).count() as i32;
        Ok(length.to_le_bytes().to_vec())
    }
}

impl NativeContract for StdLib {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "StdLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Default for StdLib {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_stdlib() -> StdLib {
        StdLib::new()
    }

    #[test]
    fn test_memory_compare() {
        let stdlib = create_stdlib();

        // Equal arrays
        let result = stdlib
            .memory_compare(&[vec![1, 2, 3], vec![1, 2, 3]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );

        // First less than second
        let result = stdlib
            .memory_compare(&[vec![1, 2, 3], vec![1, 2, 4]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );

        // First greater than second
        let result = stdlib
            .memory_compare(&[vec![1, 2, 4], vec![1, 2, 3]])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Different lengths
        let result = stdlib.memory_compare(&[vec![1, 2], vec![1, 2, 3]]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_memory_search_basic() {
        let stdlib = create_stdlib();

        // Basic forward search
        let mem = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let pattern = vec![4, 5, 6];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Pattern not found
        let pattern = vec![9, 10];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );

        // Empty pattern
        let pattern = vec![];
        let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );
    }

    #[test]
    fn test_memory_search_with_start() {
        let stdlib = create_stdlib();

        let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
        let pattern = vec![4, 5];

        // Search from start=0
        let start = 0i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Search from start=4 (should find second occurrence)
        let start = 4i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Search from start=6 (should not find)
        let start = 6i32.to_le_bytes().to_vec();
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_memory_search_backward() {
        let stdlib = create_stdlib();

        let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
        let pattern = vec![4, 5];

        // Backward search from start=8 (search in [0..8])
        let start = 8i32.to_le_bytes().to_vec();
        let backward = vec![1u8]; // true
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Backward search from start=5 (search in [0..5], should find first occurrence)
        let start = 5i32.to_le_bytes().to_vec();
        let backward = vec![1u8];
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );

        // Backward search from start=3 (search in [0..3], should not find)
        let start = 3i32.to_le_bytes().to_vec();
        let backward = vec![1u8];
        let result = stdlib
            .memory_search(&[mem.clone(), pattern.clone(), start, backward])
            .unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            -1
        );
    }

    #[test]
    fn test_string_split_basic() {
        let stdlib = create_stdlib();

        let string = "hello,world,test".as_bytes().to_vec();
        let separator = ",".as_bytes().to_vec();
        let result = stdlib.string_split(&[string, separator]).unwrap();

        // Parse result: [count][len1][data1][len2][data2][len3][data3]
        let count = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(count, 3);

        let mut offset = 4;
        let len1 = u32::from_le_bytes([
            result[offset],
            result[offset + 1],
            result[offset + 2],
            result[offset + 3],
        ]) as usize;
        offset += 4;
        let part1 = String::from_utf8(result[offset..offset + len1].to_vec()).unwrap();
        assert_eq!(part1, "hello");
        offset += len1;

        let len2 = u32::from_le_bytes([
            result[offset],
            result[offset + 1],
            result[offset + 2],
            result[offset + 3],
        ]) as usize;
        offset += 4;
        let part2 = String::from_utf8(result[offset..offset + len2].to_vec()).unwrap();
        assert_eq!(part2, "world");
        offset += len2;

        let len3 = u32::from_le_bytes([
            result[offset],
            result[offset + 1],
            result[offset + 2],
            result[offset + 3],
        ]) as usize;
        offset += 4;
        let part3 = String::from_utf8(result[offset..offset + len3].to_vec()).unwrap();
        assert_eq!(part3, "test");
    }

    #[test]
    fn test_string_split_with_empty_entries() {
        let stdlib = create_stdlib();

        let string = "hello,,world,,test".as_bytes().to_vec();
        let separator = ",".as_bytes().to_vec();

        // Without removeEmptyEntries (default: false)
        let result = stdlib
            .string_split(&[string.clone(), separator.clone()])
            .unwrap();
        let count = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(count, 5); // hello, "", world, "", test

        // With removeEmptyEntries = true
        let remove_empty = vec![1u8];
        let result = stdlib
            .string_split(&[string.clone(), separator.clone(), remove_empty])
            .unwrap();
        let count = u32::from_le_bytes([result[0], result[1], result[2], result[3]]);
        assert_eq!(count, 3); // hello, world, test
    }

    #[test]
    fn test_str_len_basic() {
        let stdlib = create_stdlib();

        // ASCII string
        let string = "hello".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            5
        );

        // Empty string
        let string = "".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            0
        );
    }

    #[test]
    fn test_str_len_unicode() {
        let stdlib = create_stdlib();

        // Emoji (should count as 1 grapheme cluster)
        let string = "🦆".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Combining character (should count as 1 grapheme cluster)
        let string = "ã".as_bytes().to_vec(); // a + combining tilde
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            1
        );

        // Mixed ASCII and emoji
        let string = "hello🦆world".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            11
        );

        // Multiple emojis
        let string = "🦆🦆🦆".as_bytes().to_vec();
        let result = stdlib.str_len(&[string]).unwrap();
        assert_eq!(
            i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
            3
        );
    }

    #[test]
    fn test_atoi_itoa() {
        let stdlib = create_stdlib();

        // Test itoa
        let number = 12345i64.to_le_bytes().to_vec();
        let result = stdlib.itoa(&[number]).unwrap();
        let string = String::from_utf8(result).unwrap();
        assert_eq!(string, "12345");

        // Test atoi
        let string = "12345".as_bytes().to_vec();
        let result = stdlib.atoi(&[string]).unwrap();
        let number = i64::from_le_bytes([
            result[0], result[1], result[2], result[3], result[4], result[5], result[6], result[7],
        ]);
        assert_eq!(number, 12345);

        // Test negative number
        let number = (-12345i64).to_le_bytes().to_vec();
        let result = stdlib.itoa(&[number]).unwrap();
        let string = String::from_utf8(result).unwrap();
        assert_eq!(string, "-12345");
    }

    #[test]
    fn test_base64_encode_decode() {
        let stdlib = create_stdlib();

        let data = b"Hello, World!".to_vec();

        // Encode
        let encoded = stdlib.base64_encode(std::slice::from_ref(&data)).unwrap();
        let encoded_str = String::from_utf8(encoded.clone()).unwrap();
        assert_eq!(encoded_str, "SGVsbG8sIFdvcmxkIQ==");

        // Decode
        let decoded = stdlib.base64_decode(&[encoded]).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_json_serialize_deserialize() {
        let stdlib = create_stdlib();

        let data = "test string".as_bytes().to_vec();

        // Serialize
        let serialized = stdlib.json_serialize(std::slice::from_ref(&data)).unwrap();
        let json_str = String::from_utf8(serialized.clone()).unwrap();
        assert!(json_str.contains("test string"));

        // Deserialize
        let deserialized = stdlib.json_deserialize(&[serialized]).unwrap();
        assert_eq!(deserialized, data);
    }
}
