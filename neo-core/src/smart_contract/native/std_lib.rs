//! StdLib native contract implementation.
//!
//! The StdLib contract provides standard utility functions for smart contracts,
//! including string manipulation, JSON operations, and mathematical functions.

use crate::UInt160;
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeMethod;
use crate::smart_contract::{BinarySerializer, ContractParameterType, JsonSerializer};
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use unicode_segmentation::UnicodeSegmentation;

/// The StdLib native contract.
pub struct StdLib {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl StdLib {
    const ID: i32 = -2;
    const MAX_INPUT_LENGTH: usize = 1024;

    /// Creates a new StdLib contract.
    pub fn new() -> Self {
        // StdLib contract hash: 0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0
        let hash = UInt160::parse("0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0")
            .expect("Valid StdLib contract hash");

        let methods = vec![
            NativeMethod::safe(
                "serialize".to_string(),
                1 << 12,
                vec![ContractParameterType::Any],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "deserialize".to_string(),
                1 << 14,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Any,
            ),
            // JSON operations
            NativeMethod::safe(
                "jsonSerialize".to_string(),
                1 << 12,
                vec![ContractParameterType::Any],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "jsonDeserialize".to_string(),
                1 << 14,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::Any,
            ),
            NativeMethod::safe(
                "itoa".to_string(),
                1 << 12,
                vec![ContractParameterType::Integer],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "itoa".to_string(),
                1 << 12,
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "atoi".to_string(),
                1 << 6,
                vec![ContractParameterType::String],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "atoi".to_string(),
                1 << 6,
                vec![
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "base64Encode".to_string(),
                1 << 5,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base64Decode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "base64UrlEncode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfEchidna),
            NativeMethod::safe(
                "base64UrlDecode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfEchidna),
            NativeMethod::safe(
                "base58Encode".to_string(),
                1 << 13,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base58Decode".to_string(),
                1 << 10,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "base58CheckEncode".to_string(),
                1 << 16,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "base58CheckDecode".to_string(),
                1 << 16,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            ),
            NativeMethod::safe(
                "hexEncode".to_string(),
                1 << 5,
                vec![ContractParameterType::ByteArray],
                ContractParameterType::String,
            )
            .with_active_in(Hardfork::HfFaun),
            NativeMethod::safe(
                "hexDecode".to_string(),
                1 << 5,
                vec![ContractParameterType::String],
                ContractParameterType::ByteArray,
            )
            .with_active_in(Hardfork::HfFaun),
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
        let methods = methods
            .into_iter()
            .map(
                |method| match (method.name.as_str(), method.parameters.len()) {
                    ("atoi", 1) => method.with_parameter_names(vec!["value".to_string()]),
                    ("atoi", 2) => {
                        method.with_parameter_names(vec!["value".to_string(), "base".to_string()])
                    }
                    ("base58CheckDecode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base58CheckEncode", 1) => {
                        method.with_parameter_names(vec!["data".to_string()])
                    }
                    ("base58Decode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base58Encode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("base64Decode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base64Encode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("base64UrlDecode", 1) => method.with_parameter_names(vec!["s".to_string()]),
                    ("base64UrlEncode", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("deserialize", 1) => method.with_parameter_names(vec!["data".to_string()]),
                    ("hexDecode", 1) => method.with_parameter_names(vec!["str".to_string()]),
                    ("hexEncode", 1) => method.with_parameter_names(vec!["bytes".to_string()]),
                    ("itoa", 1) => method.with_parameter_names(vec!["value".to_string()]),
                    ("itoa", 2) => {
                        method.with_parameter_names(vec!["value".to_string(), "base".to_string()])
                    }
                    ("jsonDeserialize", 1) => method.with_parameter_names(vec!["json".to_string()]),
                    ("jsonSerialize", 1) => method.with_parameter_names(vec!["item".to_string()]),
                    ("memoryCompare", 2) => {
                        method.with_parameter_names(vec!["str1".to_string(), "str2".to_string()])
                    }
                    ("memorySearch", 2) => {
                        method.with_parameter_names(vec!["mem".to_string(), "value".to_string()])
                    }
                    ("memorySearch", 3) => method.with_parameter_names(vec![
                        "mem".to_string(),
                        "value".to_string(),
                        "start".to_string(),
                    ]),
                    ("memorySearch", 4) => method.with_parameter_names(vec![
                        "mem".to_string(),
                        "value".to_string(),
                        "start".to_string(),
                        "backward".to_string(),
                    ]),
                    ("serialize", 1) => method.with_parameter_names(vec!["item".to_string()]),
                    ("strLen", 1) => method.with_parameter_names(vec!["str".to_string()]),
                    ("stringSplit", 2) => method
                        .with_parameter_names(vec!["str".to_string(), "separator".to_string()]),
                    ("stringSplit", 3) => method.with_parameter_names(vec![
                        "str".to_string(),
                        "separator".to_string(),
                        "removeEmptyEntries".to_string(),
                    ]),
                    _ => method,
                },
            )
            .collect();

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Invokes a method on the StdLib contract.
    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "serialize" => self.serialize(engine, args),
            "deserialize" => self.deserialize(engine, args),
            "jsonSerialize" => self.json_serialize(engine, args),
            "jsonDeserialize" => self.json_deserialize(engine, args),
            "atoi" => self.atoi(args),
            "itoa" => self.itoa(args),
            "base64Encode" => self.base64_encode(args),
            "base64Decode" => self.base64_decode(args),
            "base64UrlEncode" => self.base64_url_encode(args),
            "base64UrlDecode" => self.base64_url_decode(args),
            "base58Encode" => self.base58_encode(args),
            "base58Decode" => self.base58_decode(args),
            "base58CheckEncode" => self.base58_check_encode(args),
            "base58CheckDecode" => self.base58_check_decode(args),
            "hexEncode" => self.hex_encode(args),
            "hexDecode" => self.hex_decode(args),
            "memoryCompare" => self.memory_compare(args),
            "memorySearch" => self.memory_search(args),
            "stringSplit" => self.string_split(engine, args),
            "strLen" => self.str_len(args),
            // Legacy alias for backward compatibility
            "stringLen" => self.str_len(args),
            _ => Err(Error::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    /// Serializes a stack item using the binary serializer.
    fn serialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "serialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Serialize failed: {e}")))
    }

    /// Deserializes a binary-serialized stack item.
    fn deserialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "deserialize requires data argument".to_string(),
            ));
        }

        let item = BinarySerializer::deserialize(&args[0], engine.execution_limits(), None)
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("Deserialize failed: {e}")))
    }

    /// Serializes a stack item to JSON.
    fn json_serialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonSerialize requires data argument".to_string(),
            ));
        }

        let item = self.decode_stack_item(engine, &args[0])?;
        JsonSerializer::serialize_to_byte_array(&item, engine.execution_limits().max_item_size)
            .map_err(|e| Error::native_contract(format!("JSON serialization error: {e}")))
    }

    /// Deserializes JSON into a stack item.
    fn json_deserialize(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "jsonDeserialize requires JSON string argument".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "jsonDeserialize")?;
        let item = JsonSerializer::deserialize(&args[0], 10)
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))?;
        BinarySerializer::serialize(&item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("JSON deserialization error: {e}")))
    }

    /// Compares two memory regions.
    fn memory_compare(&self, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "memoryCompare requires two data arguments".to_string(),
            ));
        }

        self.ensure_max_input_len(&args[0], "memoryCompare")?;
        self.ensure_max_input_len(&args[1], "memoryCompare")?;
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

        self.ensure_max_input_len(mem, "memorySearch")?;
        self.ensure_max_input_len(value, "memorySearch")?;

        // Parse optional start parameter (default: 0)
        let start = if args.len() >= 3 {
            let start_value = BigInt::from_signed_bytes_le(&args[2]);
            start_value
                .to_i32()
                .ok_or_else(|| Error::native_contract("start parameter out of range"))?
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
    fn string_split(&self, engine: &ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "stringSplit requires at least 2 arguments (str, separator)".to_string(),
            ));
        }

        let string_data = String::from_utf8(args[0].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        let separator = String::from_utf8(args[1].clone())
            .map_err(|_| Error::native_contract("Invalid UTF-8 separator"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "stringSplit")?;
        self.ensure_max_input_len(separator.as_bytes(), "stringSplit")?;

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

        let items = parts
            .into_iter()
            .map(|part| StackItem::from_byte_string(part.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        let array_item = StackItem::from_array(items);
        BinarySerializer::serialize(&array_item, engine.execution_limits())
            .map_err(|e| Error::native_contract(format!("stringSplit failed: {e}")))
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
            .map_err(|_| Error::native_contract("Invalid UTF-8 string"))?;

        self.ensure_max_input_len(string_data.as_bytes(), "strLen")?;

        // Count grapheme clusters (extended grapheme clusters) to match C# TextElementEnumerator
        let length = string_data.graphemes(true).count() as i32;
        Ok(length.to_le_bytes().to_vec())
    }

    fn decode_stack_item(&self, engine: &ApplicationEngine, data: &[u8]) -> Result<StackItem> {
        let limits = engine.execution_limits();
        match BinarySerializer::deserialize(data, limits, None) {
            Ok(item) => Ok(item),
            Err(_) => Ok(StackItem::from_byte_string(data.to_vec())),
        }
    }
}

mod encoding;
mod native_impl;

#[cfg(test)]
mod tests;
